//! Shared harness for the ported rquickjs tests.
//!
//! Bun runs one VM per thread and never tears it down, so every test runs
//! its JavaScript on a single dedicated thread: `test_with` / `js_thread`
//! ship a closure there and block for the result (panics are propagated so
//! `#[should_panic]` keeps working). Tests therefore share one global scope —
//! use distinct global names.

#![allow(dead_code)]

use std::path::PathBuf;

/// Workspace repository root (`rbun/`), two levels above this crate.
pub fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap_or_else(|_| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.."))
}

use std::sync::mpsc;
use std::sync::{Mutex, OnceLock};
use std::thread;

use rbun::{Context, Ctx, Runtime};

type Job = Box<dyn FnOnce() + Send + 'static>;

fn sender() -> &'static Mutex<mpsc::Sender<Job>> {
    static SENDER: OnceLock<Mutex<mpsc::Sender<Job>>> = OnceLock::new();
    SENDER.get_or_init(|| {
        let (tx, rx) = mpsc::channel::<Job>();
        thread::Builder::new()
            .name("rbun-test-js".into())
            .stack_size(16 * 1024 * 1024)
            .spawn(move || {
                while let Ok(job) = rx.recv() {
                    job();
                }
            })
            .expect("spawn JS thread");
        Mutex::new(tx)
    })
}

/// Run `f` on the JS thread and return its result (re-panicking here if it
/// panicked there).
pub fn js_thread<F, R>(f: F) -> R
where
    F: FnOnce() -> R + Send + 'static,
    R: Send + 'static,
{
    let (tx, rx) = mpsc::channel();
    sender()
        .lock()
        .unwrap()
        .send(Box::new(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));
            let _ = tx.send(result);
        }))
        .expect("JS thread alive");
    match rx.recv().expect("JS thread result") {
        Ok(value) => value,
        Err(payload) => std::panic::resume_unwind(payload),
    }
}

/// rquickjs' `test_with`: a runtime + full context, on the JS thread.
pub fn test_with<F, R>(f: F) -> R
where
    F: for<'js> FnOnce(Ctx<'js>) -> R + Send + 'static,
    R: Send + 'static,
{
    js_thread(move || {
        let rt = Runtime::new().unwrap();
        let ctx = Context::full(&rt).unwrap();
        ctx.with(f)
    })
}

/// Run an async block on the JS thread inside a current-thread tokio
/// runtime (rquickjs' `async_test_case!` shape).
#[macro_export]
macro_rules! async_test_case {
    ($name:ident => ($rt:ident, $ctx:ident) { $($t:tt)* }) => {
        #[test]
        fn $name() {
            $crate::common::js_thread(|| {
                let tokio = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
                let set = tokio::task::LocalSet::new();
                set.block_on(&tokio, async {
                    let $rt = rbun::AsyncRuntime::new().unwrap();
                    let $ctx = rbun::AsyncContext::full(&$rt).await.unwrap();
                    $($t)*
                })
            })
        }
    };
}

/// Run an async block on the JS thread (for `#[tokio::test]`-style tests).
pub fn block_on<F, Fut, R>(f: F) -> R
where
    F: FnOnce() -> Fut + Send + 'static,
    Fut: std::future::Future<Output = R>,
    R: Send + 'static,
{
    js_thread(move || {
        let tokio = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let set = tokio::task::LocalSet::new();
        set.block_on(&tokio, f())
    })
}

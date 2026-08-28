//! Async integration: [`AsyncRuntime`] / [`AsyncContext`] (mirroring
//! rquickjs' `full-async` API), the executor that runs host futures,
//! [`PromiseFuture`] and [`Promised`].
//!
//! Bun owns the event loop, so "async" here means: a Rust future is polled,
//! and whenever it is pending the driver runs Bun's loop (`tick` +
//! `auto_tick_active`) and every host future spawned with [`Ctx::spawn`].
//! Wakers from other threads (e.g. a `spawn_blocking` result) wake the
//! uSockets loop so a blocked `auto_tick_active` returns promptly.

use core::cell::RefCell;
use core::future::Future;
use core::pin::Pin;
use core::task::{Context as TaskContext, Poll, Waker};
use std::sync::Arc;

use crate::error::Result;
use crate::ffi;
use crate::loader::{Loader, Resolver};
use crate::runtime::{Context, ContextBuilder, Ctx, Runtime, RuntimeInner, RuntimeOptions};
use crate::value::{FromJs, IntoJs, Promise, Value};

// ─── Executor ────────────────────────────────────────────────────────────

type LocalFuture = Pin<Box<dyn Future<Output = ()> + 'static>>;

#[derive(Default)]
pub(crate) struct Executor {
    futures: RefCell<Vec<LocalFuture>>,
    incoming: RefCell<Vec<LocalFuture>>,
    /// Waker of the `drive()` future, if one is active.
    drive_waker: RefCell<Option<Waker>>,
}

impl Executor {
    pub(crate) fn spawn<'js, F>(&self, future: F)
    where
        F: Future<Output = ()> + 'js,
    {
        let boxed: Pin<Box<dyn Future<Output = ()> + 'js>> = Box::pin(future);
        // SAFETY: lifetime erasure — the runtime is process-lifetime on its
        // thread and the executor lives inside it, so a `'js` future can never
        // outlive what it borrows.
        let boxed: LocalFuture = unsafe { core::mem::transmute(boxed) };
        self.incoming.borrow_mut().push(boxed);
        if let Some(waker) = self.drive_waker.borrow().as_ref() {
            waker.wake_by_ref();
        }
    }

    pub(crate) fn has_pending(&self) -> bool {
        !self.futures.borrow().is_empty() || !self.incoming.borrow().is_empty()
    }

    /// Poll every spawned future once. Returns whether any of them completed.
    pub(crate) fn poll_all(&self, waker: &Waker) -> bool {
        let mut completed = false;
        let mut futures = core::mem::take(&mut *self.futures.borrow_mut());
        futures.append(&mut self.incoming.borrow_mut());
        let mut cx = TaskContext::from_waker(waker);
        futures.retain_mut(|future| match future.as_mut().poll(&mut cx) {
            Poll::Ready(()) => {
                completed = true;
                false
            }
            Poll::Pending => true,
        });
        // Futures spawned while polling land in `incoming`; keep them.
        let mut current = self.futures.borrow_mut();
        futures.append(&mut current);
        *current = futures;
        completed
    }

    pub(crate) fn poll_all_detached(&self, ctx: &Ctx<'_>) -> bool {
        let waker = wake_loop_waker(ctx.vm());
        self.poll_all(&waker)
    }
}

// ─── Wakers ──────────────────────────────────────────────────────────────

struct VmPtr(*mut core::ffi::c_void);
// SAFETY: only used to call `bun_embed_vm_wakeup`, which is thread-safe.
unsafe impl Send for VmPtr {}
// SAFETY: as above.
unsafe impl Sync for VmPtr {}

struct LoopWaker {
    outer: Option<Waker>,
    vm: VmPtr,
}

impl std::task::Wake for LoopWaker {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        if let Some(outer) = &self.outer {
            outer.wake_by_ref();
        }
        // SAFETY: thread-safe wakeup of the runtime's I/O loop.
        unsafe { ffi::bun_embed_vm_wakeup(self.vm.0) };
    }
}

fn wake_loop_waker(vm: *mut core::ffi::c_void) -> Waker {
    Waker::from(Arc::new(LoopWaker { outer: None, vm: VmPtr(vm) }))
}

fn combined_waker(outer: &Waker, vm: *mut core::ffi::c_void) -> Waker {
    Waker::from(Arc::new(LoopWaker { outer: Some(outer.clone()), vm: VmPtr(vm) }))
}

// ─── Driver ──────────────────────────────────────────────────────────────

/// Poll `future`, driving Bun's event loop and the host executor while it is
/// pending. Returns `Pending` only when nothing on this thread can make
/// progress (waiting on an external waker).
pub(crate) fn drive<R>(inner: &RuntimeInner, future: &mut Pin<Box<dyn Future<Output = R> + '_>>, cx: &mut TaskContext<'_>) -> Poll<R> {
    let ctx = Ctx::from_inner(inner);
    let waker = combined_waker(cx.waker(), inner.vm);
    let mut task_cx = TaskContext::from_waker(&waker);
    loop {
        ctx.run_deferred();
        if let Poll::Ready(result) = future.as_mut().poll(&mut task_cx) {
            return Poll::Ready(result);
        }
        let completed = inner.executor.poll_all(&waker);
        ctx.tick();
        if completed {
            // A host future settled a promise: propagate through the
            // microtask queue and re-poll.
            continue;
        }
        if ctx.is_event_loop_alive() {
            // Block until bun has something to do (or a waker fires).
            ctx.auto_tick_active();
            continue;
        }
        return Poll::Pending;
    }
}

// ─── PromiseFuture ───────────────────────────────────────────────────────

/// Future returned by [`Promise::into_future`].
pub struct PromiseFuture<'js, V> {
    promise: Promise<'js>,
    _marker: core::marker::PhantomData<V>,
}

impl<'js, V> PromiseFuture<'js, V> {
    pub(crate) fn new(promise: Promise<'js>) -> Self {
        PromiseFuture { promise, _marker: core::marker::PhantomData }
    }
}

impl<'js, V: FromJs<'js>> Future for PromiseFuture<'js, V> {
    type Output = Result<V>;

    fn poll(self: Pin<&mut Self>, _cx: &mut TaskContext<'_>) -> Poll<Self::Output> {
        let ctx = *self.promise.ctx();
        loop {
            if let Some(result) = self.promise.result() {
                return Poll::Ready(result);
            }
            if ctx.inner.executor.has_pending() || !ctx.inner.deferred.borrow().is_empty() {
                // Let the driver poll host futures; it re-polls us afterwards.
                return Poll::Pending;
            }
            ctx.tick();
            if let Some(result) = self.promise.result() {
                return Poll::Ready(result);
            }
            if ctx.is_event_loop_alive() {
                ctx.auto_tick_active();
                continue;
            }
            return Poll::Pending;
        }
    }
}

// ─── Promised ────────────────────────────────────────────────────────────

/// A Rust future passed to JavaScript as a promise (mirrors
/// `rquickjs::promise::Promised`).
pub struct Promised<T>(pub T);

impl<T> From<T> for Promised<T> {
    fn from(future: T) -> Self {
        Promised(future)
    }
}

impl<'js, T, R> IntoJs<'js> for Promised<T>
where
    T: Future<Output = R> + 'js,
    R: IntoJs<'js> + 'js,
{
    fn into_js(self, ctx: &Ctx<'js>) -> Result<Value<'js>> {
        let ctx = *ctx;
        let (promise, resolve, reject) = Promise::new(&ctx)?;
        let future = self.0;
        ctx.spawn(async move {
            match future.await.into_js(&ctx) {
                Ok(value) => {
                    let _ = resolve.call::<_, ()>((value,));
                }
                Err(error) => {
                    let value = error.throw(&ctx);
                    let _ = reject.call::<_, ()>((value,));
                }
            }
        });
        Ok(promise.into_value())
    }
}

// ─── AsyncRuntime / AsyncContext ─────────────────────────────────────────

/// Async flavour of [`Runtime`]; all methods are thin async wrappers so code
/// written against `rquickjs::AsyncRuntime` reads the same.
#[derive(Clone)]
pub struct AsyncRuntime {
    inner: Runtime,
}

impl AsyncRuntime {
    pub fn new() -> Result<Self> {
        Ok(AsyncRuntime { inner: Runtime::new()? })
    }

    pub fn new_with(options: RuntimeOptions) -> Result<Self> {
        Ok(AsyncRuntime { inner: Runtime::new_with(options)? })
    }

    pub fn inner(&self) -> &Runtime {
        &self.inner
    }

    pub async fn set_max_stack_size(&self, size: usize) {
        self.inner.set_max_stack_size(size)
    }

    pub async fn set_memory_limit(&self, limit: usize) {
        self.inner.set_memory_limit(limit)
    }

    pub async fn set_gc_threshold(&self, threshold: usize) {
        self.inner.set_gc_threshold(threshold)
    }

    pub async fn set_info<S: Into<String>>(&self, info: S) -> Result<()> {
        self.inner.set_info(info)
    }

    pub async fn set_loader<R, L>(&self, resolver: R, loader: L)
    where
        R: Resolver + 'static,
        L: Loader + 'static,
    {
        self.inner.set_loader(resolver, loader)
    }

    pub async fn set_host_promise_rejection_tracker(&self, tracker: Option<crate::runtime::RejectionTracker>) {
        self.inner.set_host_promise_rejection_tracker(tracker)
    }

    pub async fn run_gc(&self) {
        self.inner.run_gc()
    }

    pub async fn is_job_pending(&self) -> bool {
        self.inner.is_job_pending()
    }

    pub async fn execute_pending_job(&self) -> Result<bool> {
        self.inner.execute_pending_job()
    }

    /// Run the event loop until idle: deferred calls, host futures (which
    /// may await tokio timers/I/O) and Bun's own timers, sockets, …
    pub fn idle(&self) -> Idle {
        Idle { runtime: self.inner.clone() }
    }

    /// A future that keeps host futures (`ctx.spawn`) and deferred calls
    /// moving for as long as it is polled. Bun's own timers/I/O are ticked
    /// without blocking; block-and-wait behaviour belongs to `async_with`.
    pub fn drive(&self) -> Drive {
        Drive { runtime: self.inner.clone() }
    }
}

/// Future returned by [`AsyncRuntime::idle`].
pub struct Idle {
    runtime: Runtime,
}

impl Future for Idle {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut TaskContext<'_>) -> Poll<()> {
        let inner = &self.runtime.inner;
        let ctx = Ctx::from_inner(inner);
        let waker = combined_waker(cx.waker(), inner.vm);
        loop {
            ctx.run_deferred();
            let completed = inner.executor.poll_all(&waker);
            ctx.tick();
            if completed {
                continue;
            }
            if ctx.is_event_loop_alive() {
                if inner.executor.has_pending() {
                    // Host futures may need the outer executor (tokio timers);
                    // let it run and come back when either side wakes us.
                    return Poll::Pending;
                }
                ctx.auto_tick_active();
                continue;
            }
            if inner.executor.has_pending() || !inner.deferred.borrow().is_empty() {
                return Poll::Pending;
            }
            return Poll::Ready(());
        }
    }
}

/// Future returned by [`AsyncRuntime::drive`].
pub struct Drive {
    runtime: Runtime,
}

impl Future for Drive {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut TaskContext<'_>) -> Poll<()> {
        let inner = &self.runtime.inner;
        *inner.executor.drive_waker.borrow_mut() = Some(cx.waker().clone());
        let ctx = Ctx::from_inner(inner);
        let waker = combined_waker(cx.waker(), inner.vm);
        loop {
            ctx.run_deferred();
            let completed = inner.executor.poll_all(&waker);
            ctx.tick();
            if !completed {
                break;
            }
        }
        if inner.executor.has_pending() {
            // Give other tasks a turn, then come back.
            cx.waker().wake_by_ref();
        }
        Poll::Pending
    }
}

/// Async flavour of [`Context`].
#[derive(Clone)]
pub struct AsyncContext {
    inner: Context,
    runtime: AsyncRuntime,
}

impl AsyncContext {
    pub async fn full(runtime: &AsyncRuntime) -> Result<Self> {
        Ok(AsyncContext { inner: Context::full(&runtime.inner)?, runtime: runtime.clone() })
    }

    pub async fn base(runtime: &AsyncRuntime) -> Result<Self> {
        Self::full(runtime).await
    }

    pub async fn custom<I>(runtime: &AsyncRuntime) -> Result<Self> {
        Self::full(runtime).await
    }

    pub fn builder() -> ContextBuilder {
        ContextBuilder::new()
    }

    pub fn runtime(&self) -> &AsyncRuntime {
        &self.runtime
    }

    /// Run a synchronous closure with a [`Ctx`] (async for rquickjs parity).
    pub async fn with<F, R>(&self, f: F) -> R
    where
        F: FnOnce(Ctx<'_>) -> R,
    {
        self.inner.with(f)
    }

    /// Run an async closure with a [`Ctx`]. While the closure's future is
    /// pending, Bun's event loop and every host future are driven.
    pub fn async_with<F, R>(&self, f: F) -> WithFuture<'_, R>
    where
        F: for<'js> FnOnce(Ctx<'js>) -> Pin<Box<dyn Future<Output = R> + 'js>>,
    {
        let inner: &RuntimeInner = &self.inner.runtime().inner;
        let future = f(Ctx::from_inner(inner));
        WithFuture { inner, future }
    }
}

/// Future returned by [`AsyncContext::async_with`].
pub struct WithFuture<'a, R> {
    inner: &'a RuntimeInner,
    future: Pin<Box<dyn Future<Output = R> + 'a>>,
}

impl<R> Future for WithFuture<'_, R> {
    type Output = R;

    fn poll(self: Pin<&mut Self>, cx: &mut TaskContext<'_>) -> Poll<R> {
        // SAFETY: `WithFuture` is not moved out of; `future` is already pinned.
        let this = unsafe { self.get_unchecked_mut() };
        drive(this.inner, &mut this.future, cx)
    }
}

/// Erase the lifetime of a boxed future (used by [`async_with!`], exactly like
/// rquickjs' `markers::uplift`).
///
/// # Safety
/// The future must be driven to completion (or dropped) before anything it
/// borrows goes away. [`WithFuture`] is awaited in place, inside the scope
/// of every borrow the async block captured, so that holds for the macro.
#[doc(hidden)]
pub unsafe fn uplift<'a, 'b, R>(future: Pin<Box<dyn Future<Output = R> + 'a>>) -> Pin<Box<dyn Future<Output = R> + 'b>> {
    unsafe { core::mem::transmute(future) }
}

/// `async_with!(context => |ctx| { ... })` — run an async block with a
/// [`Ctx`] on an [`AsyncContext`]. Also accepts `&context`.
#[macro_export]
macro_rules! async_with {
    ($context:expr => |$ctx:ident| { $($t:tt)* }) => {
        $crate::AsyncContext::async_with(&$context, |$ctx| {
            let fut = ::std::boxed::Box::pin(async move { $($t)* });
            // SAFETY: see `uplift`; the returned future is awaited in place.
            unsafe { $crate::async_rt::uplift(fut) }
        })
    };
}

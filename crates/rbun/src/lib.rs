//! `rbun` — embed Bun's JavaScript runtime in Rust, with an API modelled on
//! [`rquickjs`](https://docs.rs/rquickjs).
//!
//! Bun is linked as `libbun_embed.dylib` (built from the vendored Bun
//! distribution; see `com/github/oven-sh/bun/_vendor.ts` and `build.rs`). Values are
//! handled through JavaScriptCore's public C API; the event loop, module
//! loader and every Bun / Node API come from Bun itself.
//!
//! ```no_run
//! use rbun::{AsyncContext, AsyncRuntime, Module, Object, async_with};
//! # async fn run() -> rbun::Result<()> {
//! let rt = AsyncRuntime::new()?;
//! let context = AsyncContext::full(&rt).await?;
//! async_with!(context => |ctx| {
//!     let module = Module::import(&ctx, "./main.ts")?.into_future::<Object>().await?;
//!     Ok::<_, rbun::Error>(())
//! }).await?;
//! # Ok(()) }
//! ```
//!
//! Differences from rquickjs worth knowing:
//! - one VM per thread with a single realm: every [`Runtime`] created on a
//!   thread is a handle to the same VM and every [`Context`] refers to the
//!   same global object;
//! - `Module::declare` registers source with Bun's loader; declared modules
//!   are transpiled by Bun and may import Bun/Node builtins;
//! - unresolved specifiers fall back to Bun's own resolution instead of
//!   failing;
//! - values are GC-protected for their whole lifetime (they can be kept
//!   anywhere on the Rust side), which also means reference cycles through
//!   Rust-held handles are never collected — `Trace` is a no-op;
//! - exception messages are JavaScriptCore's, not QuickJS'.

pub mod ffi;

pub mod array_buffer;
pub mod async_rt;
pub mod atom;
pub mod class;
pub mod convert;
pub mod error;
pub mod function;
pub mod iterable;
pub mod loader;
pub mod module;
pub mod object;
pub mod persistent;
pub mod prelude;
pub mod runtime;
pub mod serde;
mod string;
pub mod utils;
pub mod value;

/// Enter any private Bun subprocess mode requested through the environment.
///
/// Call this as the first statement in an embedding executable's `main`.
/// Most invocations return immediately. On macOS, a [`Bun.WebView`](https://bun.com/docs/runtime/webview)
/// host child re-executes the current executable with a private environment
/// marker; this function transfers that child into Bun's native WebKit event
/// loop and does not return.
pub fn run_internal_process_mode() {
    // SAFETY: the ABI takes no arguments. Its only special path validates the
    // inherited file descriptor before entering Bun's non-returning host loop.
    unsafe { ffi::bun_embed_run_internal_process_mode() }
}

/// `rquickjs::context` compatibility path.
pub mod context {
    pub use crate::runtime::{Context, ContextBuilder, Ctx, EvalOptions, intrinsic};
}
/// `rquickjs::promise` compatibility path.
pub mod promise {
    pub use crate::async_rt::{PromiseFuture, Promised};
    pub use crate::value::{Promise, PromiseState};
}
/// `rquickjs::typed_array` compatibility path.
pub mod typed_array {
    pub use crate::array_buffer::{TypedArray, TypedArrayItem};
}

pub use array_buffer::{ArrayBuffer, TypedArray, TypedArrayItem};
pub use async_rt::{AsyncContext, AsyncRuntime, Drive, Idle, PromiseFuture, Promised};
pub use atom::{Atom, FromAtom, IntoAtom, PredefinedAtom};
pub use class::{Class, JsClass, JsClassName, PrototypeBuilder, Readable, Trace, Tracer, Writable, with_instance};
pub use convert::{Coerced, FromIteratorJs, IteratorJs, Null, Undefined};
pub use error::{AsSliceError, BorrowError, CatchResultExt, CaughtError, CaughtResult, Error, Exception, Result, ThrowResultExt};
pub use function::{Args, Async, Exhaustive, Func, IntoArg, IntoJsFunc, MutFn, OnceFn, Opt, Rest, This};
pub use iterable::{Iterable, JsIterator};
pub use loader::{Loader, Resolver};
pub use module::{Declarations, Evaluated, Exports, Module, ModuleDef};
pub use object::{Accessor, AsProperty, Filter, Property};
pub use persistent::{Outlive, Persistent};
pub use runtime::{
    Context, ContextBuilder, Ctx, EvalOptions, JsLifetime, Runtime, RuntimeOptions, TestResult,
    UserDataGuard,
};
pub use utils::{ResultExt, format_error};
pub use value::{Array, BigInt, Constructor, FromJs, Function, IntoArgs, IntoJs, Object, Promise, PromiseState, String, Symbol, Type, Value};

#[cfg(feature = "macro")]
pub use rbun_macros::{class, methods, JsLifetime as JsLifetimeDerive, Trace as TraceDerive};

/// Derive macros under the rquickjs names (`#[derive(rbun::class::Trace)]`,
/// `#[derive(rbun::JsLifetime)]`).
#[cfg(feature = "macro")]
pub mod derive {
    pub use rbun_macros::{JsLifetime, Trace};
}

/// Test helper: run `f` with a [`Ctx`] on a fresh handle to this thread's
/// runtime (mirrors rquickjs' `test_with`).
pub fn test_with<F, R>(f: F) -> R
where
    F: FnOnce(Ctx<'_>) -> R,
{
    let rt = Runtime::new().expect("runtime");
    let ctx = Context::full(&rt).expect("context");
    ctx.with(f)
}

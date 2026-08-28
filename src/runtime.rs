//! [`Runtime`], [`Context`] and [`Ctx`] — the synchronous half of the API,
//! shaped after `rquickjs::{Runtime, Context, Ctx}`.
//!
//! Bun has exactly one VM per thread with a single global object, so:
//! - the first `Runtime::new()` on a thread boots Bun; later calls on the
//!   same thread return handles to the same VM (state is shared);
//! - a [`Context`] is a cheap handle onto its runtime rather than a separate
//!   realm;
//! - a runtime is bound to the thread that created it (`!Send`), which holds
//!   the JavaScriptCore API lock for its whole lifetime — the same model as
//!   Bun's CLI main thread.

use core::any::{Any, TypeId};
use core::cell::{Cell, RefCell};
use core::ffi::{c_char, c_int};
use core::marker::PhantomData;
use core::ops::Deref;
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use crate::error::{Error, Result};
use crate::ffi;
use crate::function::Function;
use crate::loader::{Loader, Resolver};
use crate::module::ModuleRegistry;
use crate::string::JsString;
use crate::value::{Array, FromJs, IntoJs, Object, Promise, String as JsStringValue, Value};

/// Options for [`Runtime::new_with`].
#[derive(Debug, Clone)]
pub struct RuntimeOptions {
    /// Working directory: anchors relative module resolution and `process.cwd()`.
    pub cwd: PathBuf,
    /// What `process.argv` / `Bun.argv` report. Defaults to the host's argv.
    pub argv: Option<Vec<std::string::String>>,
    /// Install Bun's crash handler (signal handlers + a Rust panic hook that
    /// prints a Bun-style crash report). Off by default for embedders.
    pub install_crash_handler: bool,
}

impl Default for RuntimeOptions {
    fn default() -> Self {
        RuntimeOptions {
            cwd: std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/")),
            argv: None,
            install_crash_handler: false,
        }
    }
}

thread_local! {
    /// The runtime living on this thread, for host-function trampolines that
    /// only receive a `JSContextRef`.
    pub(crate) static CURRENT: Cell<*const RuntimeInner> = const { Cell::new(core::ptr::null()) };
    /// Keeps the per-thread runtime alive forever (Bun never tears down a VM).
    static THREAD_RUNTIME: RefCell<Option<Rc<RuntimeInner>>> = const { RefCell::new(None) };
}

pub(crate) const HOST_MODULE_NAME: &str = "[rbun-host].js";
pub(crate) const EVAL_MODULE_NAME: &str = "[rbun-eval].js";

pub(crate) type DeferredJob = Box<dyn FnOnce() -> Result<()>>;
pub(crate) type RejectionTracker = Box<dyn for<'js> Fn(Ctx<'js>, Value<'js>, Value<'js>, bool)>;

pub(crate) struct RuntimeInner {
    pub(crate) vm: *mut core::ffi::c_void,
    pub(crate) ctx: ffi::JSGlobalContextRef,
    cwd: PathBuf,
    pending_exception: Cell<ffi::JSValueRef>,
    userdata: RefCell<HashMap<TypeId, Rc<dyn Any>>>,
    pub(crate) modules: RefCell<ModuleRegistry>,
    pub(crate) resolver: RefCell<Option<Box<dyn Resolver>>>,
    pub(crate) loader: RefCell<Option<Box<dyn Loader>>>,
    pub(crate) executor: crate::async_rt::Executor,
    /// Calls queued by [`Function::defer`]; run by `execute_pending_job`.
    pub(crate) deferred: RefCell<VecDeque<DeferredJob>>,
    pub(crate) rejection_tracker: RefCell<Option<RejectionTracker>>,
    info: RefCell<Option<std::string::String>>,
    /// `__rbun.<name>` helper functions, rooted for the runtime's lifetime.
    helpers: RefCell<HashMap<&'static str, ffi::JSValueRef>>,
    /// Interned JS strings for short property keys (see [`Ctx::intern`]).
    atoms: RefCell<HashMap<Box<str>, ffi::JSValueRef>>,
}

/// Upper bounds for the interned-key cache: keys longer than this are not
/// cached, and the cache stops growing once full.
const INTERN_MAX_KEY_LEN: usize = 64;
const INTERN_MAX_ENTRIES: usize = 4096;

/// The embedded Bun VM.
#[derive(Clone)]
pub struct Runtime {
    pub(crate) inner: Rc<RuntimeInner>,
}

impl Runtime {
    /// Boot Bun on the current thread with the process' cwd and argv, or
    /// return a handle to the runtime this thread already has.
    pub fn new() -> Result<Runtime> {
        Runtime::new_with(RuntimeOptions::default())
    }

    /// Boot Bun on the current thread (options are ignored when the thread
    /// already has a runtime).
    pub fn new_with(options: RuntimeOptions) -> Result<Runtime> {
        if let Some(existing) = THREAD_RUNTIME.with(|r| r.borrow().clone()) {
            return Ok(Runtime { inner: existing });
        }

        let argv: Vec<std::string::String> = options.argv.unwrap_or_else(|| std::env::args().collect());
        // argv must outlive the process (bun keeps the pointers).
        let argv_c: Vec<std::ffi::CString> = argv
            .iter()
            .map(|a| std::ffi::CString::new(a.replace('\0', "")).expect("no NUL"))
            .collect();
        let argv_ptrs: Vec<*const c_char> = argv_c.iter().map(|a| a.as_ptr()).collect();
        let argc = argv_ptrs.len() as c_int;
        let argv_ptrs = Box::leak(argv_ptrs.into_boxed_slice());
        core::mem::forget(argv_c);

        // SAFETY: argv is process-lifetime (leaked above).
        unsafe { ffi::bun_embed_init(argc, argv_ptrs.as_ptr(), options.install_crash_handler) };

        let cwd = options.cwd.canonicalize().unwrap_or(options.cwd);
        let cwd_bytes = cwd.as_os_str().as_encoded_bytes();
        // SAFETY: valid (ptr, len) pair.
        let vm = unsafe { ffi::bun_embed_vm_create(cwd_bytes.as_ptr(), cwd_bytes.len()) };
        if vm.is_null() {
            return Err(Error::Init(last_error()));
        }
        // SAFETY: `vm` was just created on this thread.
        let ctx = unsafe { ffi::bun_embed_vm_global_object(vm) };

        let inner = Rc::new(RuntimeInner {
            vm,
            ctx,
            cwd,
            pending_exception: Cell::new(core::ptr::null()),
            userdata: RefCell::new(HashMap::new()),
            modules: RefCell::new(ModuleRegistry::default()),
            resolver: RefCell::new(None),
            loader: RefCell::new(None),
            executor: crate::async_rt::Executor::default(),
            deferred: RefCell::new(VecDeque::new()),
            rejection_tracker: RefCell::new(None),
            info: RefCell::new(None),
            helpers: RefCell::new(HashMap::new()),
            atoms: RefCell::new(HashMap::new()),
        });
        CURRENT.with(|c| c.set(Rc::as_ptr(&inner)));
        THREAD_RUNTIME.with(|r| *r.borrow_mut() = Some(inner.clone()));

        let rt = Runtime { inner };
        rt.with(|ctx| {
            let url = ctx.host_source_url();
            ctx.eval_with_source_url::<(), _>(BOOTSTRAP, &url)
        })?;
        crate::module::install_hooks(&rt)?;
        Ok(rt)
    }

    /// Run a closure with a [`Ctx`] (Bun has a single realm, so this is the
    /// same as going through a [`Context`]).
    pub fn with<F, R>(&self, f: F) -> R
    where
        F: FnOnce(Ctx<'_>) -> R,
    {
        f(Ctx { inner: &self.inner })
    }

    pub fn cwd(&self) -> &Path {
        &self.inner.cwd
    }

    /// Kept for API compatibility with rquickjs: JavaScriptCore sizes its
    /// stack from the thread it runs on; spawn the JS thread with the stack
    /// you need instead.
    pub fn set_max_stack_size(&self, _size: usize) {}

    /// No-op (JavaScriptCore manages its own heap).
    pub fn set_memory_limit(&self, _limit: usize) {}

    /// No-op (JavaScriptCore manages its own GC pacing).
    pub fn set_gc_threshold(&self, _threshold: usize) {}

    /// A label for this runtime (informational).
    pub fn set_info<S: Into<std::string::String>>(&self, info: S) -> Result<()> {
        *self.inner.info.borrow_mut() = Some(info.into());
        Ok(())
    }

    pub fn info(&self) -> Option<std::string::String> {
        self.inner.info.borrow().clone()
    }

    /// Install a module [`Resolver`] and [`Loader`], consulted by Bun's module
    /// loader before its own resolution (see [`crate::loader`]).
    pub fn set_loader<R, L>(&self, resolver: R, loader: L)
    where
        R: Resolver + 'static,
        L: Loader + 'static,
    {
        *self.inner.resolver.borrow_mut() = Some(Box::new(resolver));
        *self.inner.loader.borrow_mut() = Some(Box::new(loader));
    }

    /// Register a callback for unhandled promise rejections. Bun reports
    /// unhandled rejections itself (see `process.on("unhandledRejection")`);
    /// the callback is kept for API compatibility and invoked for rejected
    /// promises observed through [`Promise::finish`] / `into_future`.
    pub fn set_host_promise_rejection_tracker(&self, tracker: Option<RejectionTracker>) {
        *self.inner.rejection_tracker.borrow_mut() = tracker;
    }

    /// Whether a deferred call is queued or anything (timers, sockets, …)
    /// keeps the event loop alive.
    pub fn is_job_pending(&self) -> bool {
        !self.inner.deferred.borrow().is_empty()
            // SAFETY: the runtime's VM, on its thread.
            || unsafe { ffi::bun_embed_vm_is_event_loop_alive(self.inner.vm) }
    }

    /// Run one deferred call if any, otherwise one non-blocking event-loop
    /// tick. Returns whether more work is pending.
    pub fn execute_pending_job(&self) -> Result<bool> {
        self.with(|ctx| ctx.execute_pending_job_inner())
    }

    pub fn run_gc(&self) {
        // SAFETY: the runtime's VM, on its thread.
        unsafe { ffi::bun_embed_vm_garbage_collect(self.inner.vm) };
    }

    /// Run the event loop until idle.
    pub fn idle(&self) {
        self.with(|ctx| ctx.run_until_idle())
    }
}

pub(crate) fn last_error() -> std::string::String {
    let mut len = 0usize;
    // SAFETY: valid out pointer; the buffer is thread-local to bun.
    let ptr = unsafe { ffi::bun_embed_last_error(&mut len) };
    if ptr.is_null() || len == 0 {
        return "unknown error".into();
    }
    // SAFETY: bun returns a valid (ptr, len) UTF-8 buffer.
    std::string::String::from_utf8_lossy(unsafe { core::slice::from_raw_parts(ptr, len) }).into_owned()
}

// ─── Context ─────────────────────────────────────────────────────────────

/// Intrinsic markers accepted by [`ContextBuilder::with`] for source
/// compatibility with rquickjs. Bun's global always has every intrinsic.
pub mod intrinsic {
    macro_rules! intrinsics {
        ($($name:ident),*) => {$(
            #[derive(Debug, Clone, Copy, Default)]
            pub struct $name;
        )*};
    }
    intrinsics!(
        Base, BaseObjects, Date, Eval, RegExpCompiler, RegExp, Json, Proxy, MapSet, TypedArrays, Promise, BigInt,
        BigFloat, BigDecimal, OperatorOverloading, Performance, WeakRef, Iterators, DisposableStack, All, None
    );
}

/// Builder mirroring `rquickjs::ContextBuilder`; intrinsics are no-ops.
pub struct ContextBuilder<I = ()>(PhantomData<I>);

impl ContextBuilder {
    pub fn new() -> Self {
        ContextBuilder(PhantomData)
    }
}

impl Default for ContextBuilder {
    fn default() -> Self {
        ContextBuilder::new()
    }
}

impl<I> ContextBuilder<I> {
    pub fn with<J>(self) -> ContextBuilder<(I, J)> {
        ContextBuilder(PhantomData)
    }

    pub fn build(self, runtime: &Runtime) -> Result<Context> {
        Context::full(runtime)
    }

    pub async fn build_async(self, runtime: &crate::AsyncRuntime) -> Result<crate::AsyncContext> {
        crate::AsyncContext::full(runtime).await
    }
}

/// Options for [`Ctx::eval_with_options`].
#[derive(Debug, Clone)]
pub struct EvalOptions {
    /// Evaluate at global scope (a classic script).
    pub global: bool,
    /// Evaluate in strict mode (rquickjs default).
    pub strict: bool,
    /// Accepted for compatibility; no effect.
    pub backtrace_barrier: bool,
    /// Evaluate as an async module (top-level `await` allowed); the result
    /// is a promise of the module's completion.
    pub promise: bool,
}

impl Default for EvalOptions {
    fn default() -> Self {
        EvalOptions { global: true, strict: true, backtrace_barrier: false, promise: false }
    }
}

/// A JavaScript context. Bun has one realm per VM, so every `Context` of a
/// runtime refers to the same global object.
#[derive(Clone)]
pub struct Context {
    runtime: Runtime,
}

impl Context {
    /// A context with all of Bun's globals (`Bun`, `process`, timers, …).
    pub fn full(runtime: &Runtime) -> Result<Context> {
        Ok(Context { runtime: runtime.clone() })
    }

    /// Same as [`full`](Self::full): Bun does not offer a reduced global.
    pub fn base(runtime: &Runtime) -> Result<Context> {
        Context::full(runtime)
    }

    /// Same as [`full`](Self::full); the intrinsic list is ignored.
    pub fn custom<I>(runtime: &Runtime) -> Result<Context> {
        Context::full(runtime)
    }

    pub fn builder() -> ContextBuilder {
        ContextBuilder::new()
    }

    pub fn runtime(&self) -> &Runtime {
        &self.runtime
    }

    pub fn with<F, R>(&self, f: F) -> R
    where
        F: FnOnce(Ctx<'_>) -> R,
    {
        f(Ctx { inner: &self.runtime.inner })
    }
}

/// Guard returned by [`Ctx::userdata`]; derefs to the stored value.
pub struct UserDataGuard<T>(Rc<T>);

impl<T> Deref for UserDataGuard<T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.0
    }
}

/// Marker trait mirroring `rquickjs::JsLifetime`: types stored in user data
/// or classes name their `'js` lifetime through it so a `'static` `TypeId`
/// can be derived.
///
/// # Safety
/// `Changed<'to>` must be the same type with the `'js` lifetime replaced.
pub unsafe trait JsLifetime<'js> {
    type Changed<'to>: 'to;
}

macro_rules! js_lifetime_static {
    ($($t:ty),*) => {$(
        // SAFETY: no lifetime to change.
        unsafe impl<'js> JsLifetime<'js> for $t { type Changed<'to> = $t; }
    )*};
}
js_lifetime_static!((), bool, i8, i16, i32, i64, u8, u16, u32, u64, usize, isize, f32, f64, char, std::string::String);
// SAFETY: substitutes the lifetime.
unsafe impl<'js> JsLifetime<'js> for Value<'js> {
    type Changed<'to> = Value<'to>;
}
// SAFETY: substitutes the lifetime.
unsafe impl<'js> JsLifetime<'js> for Object<'js> {
    type Changed<'to> = Object<'to>;
}
// SAFETY: substitutes the lifetime.
unsafe impl<'js> JsLifetime<'js> for Function<'js> {
    type Changed<'to> = Function<'to>;
}
// SAFETY: substitutes the lifetime.
unsafe impl<'js> JsLifetime<'js> for Array<'js> {
    type Changed<'to> = Array<'to>;
}
// SAFETY: substitutes the lifetime.
unsafe impl<'js> JsLifetime<'js> for Promise<'js> {
    type Changed<'to> = Promise<'to>;
}
// SAFETY: substitutes the lifetime.
unsafe impl<'js, T: JsLifetime<'js>> JsLifetime<'js> for Vec<T> {
    type Changed<'to> = Vec<T::Changed<'to>>;
}
// SAFETY: substitutes the lifetime.
unsafe impl<'js, T: JsLifetime<'js>> JsLifetime<'js> for Option<T> {
    type Changed<'to> = Option<T::Changed<'to>>;
}
// SAFETY: substitutes the lifetime.
unsafe impl<'js, T: JsLifetime<'js>> JsLifetime<'js> for Box<T> {
    type Changed<'to> = Box<T::Changed<'to>>;
}
// SAFETY: substitutes the lifetime.
unsafe impl<'js, C: JsLifetime<'js>> JsLifetime<'js> for crate::Class<'js, C> {
    type Changed<'to> = crate::Class<'to, C::Changed<'to>>;
}

/// Handle to a live runtime; cheap to copy (and `Clone`, like rquickjs).
#[derive(Clone, Copy)]
pub struct Ctx<'js> {
    pub(crate) inner: &'js RuntimeInner,
}

impl<'js> Ctx<'js> {
    pub(crate) fn raw(&self) -> ffi::JSContextRef {
        self.inner.ctx
    }

    pub(crate) fn vm(&self) -> *mut core::ffi::c_void {
        self.inner.vm
    }

    pub(crate) fn from_inner(inner: &'js RuntimeInner) -> Self {
        Ctx { inner }
    }

    pub fn cwd(&self) -> &'js Path {
        &self.inner.cwd
    }

    pub(crate) fn host_source_url(&self) -> std::string::String {
        format!("file://{}/{}", self.inner.cwd.display(), HOST_MODULE_NAME)
    }

    pub(crate) fn eval_source_url(&self) -> std::string::String {
        format!("file://{}/{}", self.inner.cwd.display(), EVAL_MODULE_NAME)
    }

    // ─── Evaluation ───

    /// Evaluate a strict-mode script and convert its completion value.
    pub fn eval<V: FromJs<'js>, S: Into<Vec<u8>>>(&self, source: S) -> Result<V> {
        self.eval_with_options(source, EvalOptions::default())
    }

    pub fn eval_with_options<V: FromJs<'js>, S: Into<Vec<u8>>>(&self, source: S, options: EvalOptions) -> Result<V> {
        let source = std::string::String::from_utf8(source.into())?;
        if options.promise {
            let name = format!("[rbun-eval-{}].js", self.inner.modules.borrow().next_eval_id());
            self.inner.modules.borrow_mut().bump_eval_id();
            let promise = crate::module::Module::evaluate(*self, name, source)?;
            return V::from_js(self, promise.into_value());
        }
        let source = if options.strict && !source.trim_start().starts_with("\"use strict\"") && !source.trim_start().starts_with("'use strict'") {
            // Same line, so error positions stay intact.
            format!("\"use strict\"; {source}")
        } else {
            source
        };
        let url = self.eval_source_url();
        self.eval_with_source_url(source, &url)
    }

    /// Evaluate a classic script with an explicit source URL (a `file://` URL
    /// anchors dynamic `import()` inside it).
    pub fn eval_with_source_url<V: FromJs<'js>, S: Into<Vec<u8>>>(&self, source: S, source_url: &str) -> Result<V> {
        let source = std::string::String::from_utf8(source.into())?;
        let script = JsString::new(&source);
        let url = JsString::new(source_url);
        let mut exception: ffi::JSValueRef = core::ptr::null();
        // SAFETY: valid context and strings.
        let raw = unsafe {
            ffi::JSEvaluateScript(self.raw(), script.as_raw(), core::ptr::null_mut(), url.as_raw(), 1, &mut exception)
        };
        crate::function::resume_pending_panic();
        if !exception.is_null() {
            return Err(self.throw_raw(exception));
        }
        // SAFETY: value from the C API.
        let value = unsafe { Value::from_raw(*self, raw) };
        self.drain_microtasks();
        V::from_js(self, value)
    }

    /// Evaluate a file (as a script, or as a module when `promise` semantics
    /// are wanted use [`Module::import`](crate::Module::import)).
    pub fn eval_file<V: FromJs<'js>, P: AsRef<Path>>(&self, path: P) -> Result<V> {
        let source = std::fs::read(path.as_ref())?;
        let url = format!("file://{}", path.as_ref().display());
        self.eval_with_source_url(source, &url)
    }

    /// Evaluate a script as an async module and return its promise.
    pub fn eval_promise<S: Into<Vec<u8>>>(&self, source: S) -> Result<Promise<'js>> {
        self.eval_with_options(source, EvalOptions { promise: true, ..Default::default() })
    }

    /// `import(specifier)` through Bun's module loader. Await the returned
    /// promise to get the module namespace object.
    pub fn import(&self, specifier: &str) -> Result<Promise<'js>> {
        crate::module::Module::import(self, specifier)
    }

    /// Alias of [`Module::import`](crate::Module::import) for rquickjs parity.
    pub fn script_or_module_name(&self) -> Option<std::string::String> {
        None
    }

    // ─── Event loop ───

    /// One non-blocking event-loop tick (tasks, immediates, microtasks).
    pub fn tick(&self) {
        // SAFETY: the runtime's VM, on its thread.
        unsafe { ffi::bun_embed_vm_tick(self.inner.vm) };
        crate::function::resume_pending_panic();
    }

    /// Run every deferred call queued with [`Function::defer`].
    pub(crate) fn run_deferred(&self) {
        loop {
            let job = self.inner.deferred.borrow_mut().pop_front();
            match job {
                Some(job) => {
                    if let Err(error) = job() {
                        let _ = self.catch();
                        let _ = error;
                    }
                }
                None => break,
            }
        }
    }

    pub(crate) fn execute_pending_job_inner(&self) -> Result<bool> {
        let job = self.inner.deferred.borrow_mut().pop_front();
        if let Some(job) = job {
            job()?;
        } else {
            self.tick();
        }
        Ok(!self.inner.deferred.borrow().is_empty() || self.is_event_loop_alive())
    }

    /// Run one pending job; returns whether more are pending.
    pub fn execute_pending_job(&self) -> bool {
        self.execute_pending_job_inner().unwrap_or(false)
    }

    pub fn drain_microtasks(&self) {
        // SAFETY: the runtime's VM, on its thread.
        unsafe { ffi::bun_embed_vm_drain_microtasks(self.inner.vm) };
        crate::function::resume_pending_panic();
    }

    pub fn is_event_loop_alive(&self) -> bool {
        // SAFETY: the runtime's VM, on its thread.
        unsafe { ffi::bun_embed_vm_is_event_loop_alive(self.inner.vm) }
    }

    /// Block until the next I/O / timer event while something keeps the loop
    /// alive, then process it.
    pub fn auto_tick_active(&self) {
        // SAFETY: the runtime's VM, on its thread.
        unsafe { ffi::bun_embed_vm_auto_tick_active(self.inner.vm) };
        crate::function::resume_pending_panic();
    }

    /// Run the loop until nothing (timers, sockets, subprocesses, host
    /// futures, deferred calls…) keeps it alive.
    pub fn run_until_idle(&self) {
        loop {
            self.run_deferred();
            self.inner.executor.poll_all_detached(self);
            self.tick();
            if self.is_event_loop_alive() {
                self.auto_tick_active();
                continue;
            }
            if self.inner.executor.has_pending() || !self.inner.deferred.borrow().is_empty() {
                // Only host futures are pending: nothing in bun would wake
                // us, so poll them again without blocking.
                std::thread::yield_now();
                continue;
            }
            break;
        }
    }

    pub fn run_gc(&self) {
        // SAFETY: the runtime's VM, on its thread.
        unsafe { ffi::bun_embed_vm_garbage_collect(self.inner.vm) };
    }

    /// Spawn a future on the runtime's executor; it is polled whenever the
    /// event loop is driven from Rust (`async_with`, `Promise::into_future`,
    /// `run_until_idle`, `AsyncRuntime::drive`).
    pub fn spawn<F>(&self, future: F)
    where
        F: core::future::Future<Output = ()> + 'js,
    {
        self.inner.executor.spawn(future);
    }

    // ─── Values ───

    pub fn globals(&self) -> Object<'js> {
        // SAFETY: valid context.
        let raw = unsafe { ffi::JSContextGetGlobalObject(self.raw()) };
        // SAFETY: the global object is always live.
        Object(unsafe { Value::from_raw(*self, raw) })
    }

    pub fn string(&self, value: &str) -> Value<'js> {
        let s = JsString::new(value);
        // SAFETY: valid context and string.
        unsafe { Value::from_raw(*self, ffi::JSValueMakeString(self.raw(), s.as_raw())) }
    }

    pub fn new_object(&self) -> Result<Object<'js>> {
        Object::new(*self)
    }

    pub fn new_array(&self) -> Result<Array<'js>> {
        Array::new(*self)
    }

    /// `new Error(message)`.
    pub fn new_error(&self, message: &str) -> Value<'js> {
        let message = self.string(message);
        let args = [message.raw];
        let mut exception: ffi::JSValueRef = core::ptr::null();
        // SAFETY: valid context; args live.
        let raw = unsafe { ffi::JSObjectMakeError(self.raw(), 1, args.as_ptr(), &mut exception) };
        if !exception.is_null() {
            // SAFETY: value from the C API.
            return unsafe { Value::from_raw(*self, exception) };
        }
        // SAFETY: fresh object.
        unsafe { Value::from_raw(*self, raw) }
    }

    /// `new TypeError(message)`.
    pub fn new_type_error(&self, message: &str) -> Value<'js> {
        self.new_error_of("TypeError", message)
    }

    /// `new <constructor>(message)` for one of the global error constructors.
    pub fn new_error_of(&self, constructor: &str, message: &str) -> Value<'js> {
        let ctor = self.globals().get::<_, Value<'js>>(constructor).ok().and_then(|v| v.into_object());
        match ctor {
            Some(ctor) => ctor
                .construct::<_, Value<'js>>((message,))
                .unwrap_or_else(|_| {
                    let _ = self.catch();
                    self.new_error(message)
                }),
            None => self.new_error(message),
        }
    }

    /// `JSON.parse(json)`.
    pub fn json_parse<S: Into<Vec<u8>>>(&self, json: S) -> Result<Value<'js>> {
        let json = std::string::String::from_utf8(json.into())?;
        let s = JsString::new(&json);
        // SAFETY: valid context and string.
        let raw = unsafe { ffi::JSValueMakeFromJSONString(self.raw(), s.as_raw()) };
        if raw.is_null() {
            return Err(crate::error::Exception::throw_syntax(self, "JSON Parse error: invalid JSON"));
        }
        // SAFETY: value from the C API.
        Ok(unsafe { Value::from_raw(*self, raw) })
    }

    /// `JSON.parse(json)` with a `reviver`-less extended form kept for parity.
    pub fn json_parse_ext<S: Into<Vec<u8>>>(&self, json: S, _allow_extensions: bool) -> Result<Value<'js>> {
        self.json_parse(json)
    }

    /// `JSON.stringify(value)`; `Ok(None)` for values JSON cannot represent.
    pub fn json_stringify<V: IntoJs<'js>>(&self, value: V) -> Result<Option<JsStringValue<'js>>> {
        let value = value.into_js(self)?;
        Ok(self.json_stringify_to_rust(&value)?.map(|s| JsStringValue(self.string(&s))))
    }

    pub fn json_stringify_replacer<V: IntoJs<'js>, R: IntoJs<'js>>(&self, value: V, _replacer: R) -> Result<Option<JsStringValue<'js>>> {
        self.json_stringify(value)
    }

    pub fn json_stringify_replacer_space<V: IntoJs<'js>, R: IntoJs<'js>, S: IntoJs<'js>>(&self, value: V, _replacer: R, _space: S) -> Result<Option<JsStringValue<'js>>> {
        self.json_stringify(value)
    }

    pub(crate) fn json_stringify_to_rust(&self, value: &Value<'js>) -> Result<Option<std::string::String>> {
        let mut exception: ffi::JSValueRef = core::ptr::null();
        // SAFETY: live value.
        let raw = unsafe { ffi::JSValueCreateJSONString(self.raw(), value.raw, 0, &mut exception) };
        if !exception.is_null() {
            return Err(self.throw_raw(exception));
        }
        // SAFETY: owned string or null.
        Ok(unsafe { JsString::from_raw(raw) }.map(|s| s.to_rust_string()))
    }

    /// Convert any `IntoJs` into a value.
    pub fn value<V: IntoJs<'js>>(&self, value: V) -> Result<Value<'js>> {
        value.into_js(self)
    }

    // ─── Exceptions ───

    /// Take the pending exception (or `undefined` when none is pending).
    pub fn catch(&self) -> Value<'js> {
        let raw = self.inner.pending_exception.replace(core::ptr::null());
        if raw.is_null() {
            return Value::new_undefined(*self);
        }
        // SAFETY: protected when it was stored; `from_raw` takes its own
        // protection and the stored one is released below.
        let value = unsafe { Value::from_raw(*self, raw) };
        // SAFETY: balances the protection taken in `throw_raw`.
        unsafe { ffi::JSValueUnprotect(self.raw(), raw) };
        value
    }

    /// Set `value` as the pending exception and return `Error::Exception`.
    pub fn throw(&self, value: Value<'js>) -> Error {
        self.throw_raw(value.raw)
    }

    pub(crate) fn throw_raw(&self, raw: ffi::JSValueRef) -> Error {
        // SAFETY: live value; protected until `catch` takes it.
        unsafe { ffi::JSValueProtect(self.raw(), raw) };
        let previous = self.inner.pending_exception.replace(raw);
        if !previous.is_null() {
            // SAFETY: balances the protection of the overwritten exception.
            unsafe { ffi::JSValueUnprotect(self.raw(), previous) };
        }
        Error::Exception
    }

    // ─── User data ───

    /// Store user data on the context, replacing (and returning) any previous
    /// value of the same type.
    pub fn store_userdata<U: JsLifetime<'js> + 'js>(&self, data: U) -> Result<Option<Rc<U::Changed<'static>>>> {
        let id = TypeId::of::<U::Changed<'static>>();
        // SAFETY: `U` and `U::Changed<'static>` are the same type modulo the
        // `'js` lifetime, which the runtime outlives on this thread.
        let data: U::Changed<'static> = unsafe { transmute_unchecked(data) };
        let previous = self.inner.userdata.borrow_mut().insert(id, Rc::new(data));
        Ok(previous.and_then(|p| p.downcast::<U::Changed<'static>>().ok()))
    }

    pub fn userdata<U: JsLifetime<'js> + 'js>(&self) -> Option<UserDataGuard<U::Changed<'static>>> {
        self.inner
            .userdata
            .borrow()
            .get(&TypeId::of::<U::Changed<'static>>())
            .cloned()
            .and_then(|p| p.downcast::<U::Changed<'static>>().ok())
            .map(UserDataGuard)
    }

    /// Remove user data; fails with `Error::UserDataBorrow` while a
    /// [`UserDataGuard`] is alive.
    pub fn remove_userdata<U: JsLifetime<'js> + 'js>(&self) -> Result<Option<Rc<U::Changed<'static>>>> {
        let id = TypeId::of::<U::Changed<'static>>();
        let mut userdata = self.inner.userdata.borrow_mut();
        if let Some(existing) = userdata.get(&id) {
            if Rc::strong_count(existing) > 1 {
                return Err(Error::UserDataBorrow);
            }
        }
        Ok(userdata.remove(&id).and_then(|p| p.downcast::<U::Changed<'static>>().ok()))
    }

    // ─── Internals ───

    /// A helper function from the bootstrap script (`__rbun.<name>`).
    pub(crate) fn function(&self, name: &'static str) -> Result<Function<'js>> {
        if let Some(&raw) = self.inner.helpers.borrow().get(name) {
            // SAFETY: rooted below for the runtime's lifetime.
            return Ok(Function(Object(unsafe { Value::from_raw(*self, raw) })));
        }
        let rbun: Object<'js> = self.globals().get("__rbun")?;
        let function: Function<'js> = rbun.get(name)?;
        let raw = function.as_raw();
        // Leak one protection so the cached raw pointer stays valid.
        core::mem::forget(function.clone());
        self.inner.helpers.borrow_mut().insert(name, raw);
        Ok(function)
    }

    /// A JS string for `name`, served from a per-runtime cache for short
    /// keys so repeated property access does not re-encode and re-allocate
    /// the key each time (rquickjs interns atoms the same way).
    pub fn intern(&self, name: &str) -> Value<'js> {
        if name.len() <= INTERN_MAX_KEY_LEN {
            if let Some(&raw) = self.inner.atoms.borrow().get(name) {
                // SAFETY: rooted below for the runtime's lifetime.
                return unsafe { Value::from_raw(*self, raw) };
            }
        }
        let value = self.string(name);
        if name.len() <= INTERN_MAX_KEY_LEN {
            let mut atoms = self.inner.atoms.borrow_mut();
            if atoms.len() < INTERN_MAX_ENTRIES {
                // Leak one protection so the cached raw pointer stays valid.
                core::mem::forget(value.clone());
                atoms.insert(name.into(), value.as_raw());
            }
        }
        value
    }
}

/// `transmute` between two types of identical size (used for lifetime
/// substitution through `JsLifetime`).
///
/// # Safety
/// `A` and `B` must be the same type up to lifetimes.
unsafe fn transmute_unchecked<A, B>(a: A) -> B {
    debug_assert_eq!(core::mem::size_of::<A>(), core::mem::size_of::<B>());
    let b = unsafe { core::mem::transmute_copy::<A, B>(&a) };
    core::mem::forget(a);
    b
}

/// Host-side helpers installed on `globalThis.__rbun` (non-enumerable).
const BOOTSTRAP: &str = r#""use strict";
(() => {
  const HOST = "[rbun-host].js";
  const rbun = {
    import: (specifier) => import(specifier),
    setName: (fn, name) => Object.defineProperty(fn, "name", { value: name, configurable: true }),
    setLength: (fn, length) => Object.defineProperty(fn, "length", { value: length, configurable: true }),
    // Strict-mode assignment (throws on read-only / setter-less accessors).
    setStrict: (obj, key, value) => { obj[key] = value; },
    defineProperty: (obj, key, value, writable, enumerable, configurable) =>
      Object.defineProperty(obj, key, { value, writable, enumerable, configurable }),
    defineAccessor: (obj, key, get, set, enumerable, configurable) =>
      Object.defineProperty(obj, key, { get, set, enumerable, configurable }),
    ownKeys: (obj, strings, symbols, enumerableOnly) => {
      const keys = [];
      if (strings) for (const key of Object.getOwnPropertyNames(obj)) {
        if (!enumerableOnly || Object.prototype.propertyIsEnumerable.call(obj, key)) keys.push(key);
      }
      if (symbols) for (const key of Object.getOwnPropertySymbols(obj)) {
        if (!enumerableOnly || Object.prototype.propertyIsEnumerable.call(obj, key)) keys.push(key);
      }
      return keys;
    },
    symbolDescription: (symbol) => symbol.description,
    getIterator: (value) => {
      if (value != null && typeof value[Symbol.iterator] === "function") return value[Symbol.iterator]();
      if (value != null && typeof value.next === "function") return value;
      throw new TypeError("value is not iterable");
    },
    makeIterable: (next) => {
      const iterator = { next, [Symbol.iterator]() { return this; } };
      return iterator;
    },
    // A real JS class function around a native instance factory, so
    // `typeof C === "function"`, `new C()`, `instanceof` and `new.target`
    // subclassing all behave like a JS class.
    makeClass: (name, factory, prototype) => {
      const C = {
        [name]: function (...args) {
          if (new.target === undefined) throw new TypeError(`Class constructor ${name} cannot be invoked without 'new'`);
          const instance = factory(...args);
          if (instance !== null && typeof instance === "object" && new.target !== C) {
            Object.setPrototypeOf(instance, new.target.prototype);
          }
          return instance;
        },
      }[name];
      Object.defineProperty(C, "prototype", { value: prototype, writable: false, enumerable: false, configurable: false });
      Object.defineProperty(prototype, "constructor", { value: C, writable: true, enumerable: false, configurable: true });
      return C;
    },
    resolve: null,
    load: null,
  };
  Object.defineProperty(globalThis, "__rbun", { value: rbun, configurable: true, writable: false, enumerable: false });
  const importerName = (importer) => {
    if (!importer || importer.endsWith(HOST) || /\[rbun-eval(-\d+)?\]\.js$/.test(importer)) return "";
    return importer.startsWith("rbun:") ? importer.slice(5) : importer;
  };
  const onResolve = ({ path, importer }) => {
    const resolved = rbun.resolve(importerName(importer), path);
    return resolved == null ? undefined : { path: resolved, namespace: "rbun" };
  };
  const onLoad = ({ path }) => {
    const entry = rbun.load(path);
    if (entry == null) throw new Error(`rbun: module not found: ${path}`);
    if (entry.exports !== undefined) return { exports: entry.exports, loader: "object" };
    return { contents: entry.contents, loader: entry.loader ?? "js" };
  };
  Bun.plugin({
    name: "rbun",
    setup(build) {
      build.onResolve({ filter: /.*/ }, onResolve);
      build.onResolve({ filter: /.*/, namespace: "rbun" }, onResolve);
      build.onLoad({ filter: /.*/, namespace: "rbun" }, onLoad);
    },
  });
})();
"#;

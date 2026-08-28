//! Functions: calling JavaScript functions and exposing Rust closures to
//! JavaScript, mirroring `rquickjs::function`.
//!
//! JSC's `JSObjectMakeFunctionWithCallback` produces a real `Function` (with
//! `Function.prototype`, `.call`, `.bind`, …) but carries no user data, so
//! the closure is looked up by the function object's identity in a
//! thread-local registry. Function objects created this way are protected for
//! the runtime's lifetime, which keeps their identity stable.

use core::cell::{Cell, RefCell};
use core::future::Future;
use core::marker::PhantomData;
use std::collections::HashMap;
use std::rc::Rc;

use crate::error::{BorrowError, Error, Result};
use crate::ffi;
use crate::runtime::{CURRENT, Ctx, RuntimeInner};
use crate::string::JsString;
pub use crate::value::{Constructor, Function};
use crate::value::{FromJs, IntoArgs, IntoJs, Object, Promise, Value};

// ─── Parameter wrappers ──────────────────────────────────────────────────

/// The `this` value of a call.
pub struct This<T>(pub T);
/// All remaining arguments (or, when calling, arguments to spread).
pub struct Rest<T>(pub Vec<T>);
/// An optional argument (`None` when missing / `undefined`).
pub struct Opt<T>(pub Option<T>);
/// A parameter that is not converted: the raw value even when `undefined`.
pub struct Exhaustive;
/// Wrap an `async fn` so its future is awaited on the runtime's executor and
/// exposed to JavaScript as a promise.
pub struct Async<F>(pub F);
/// Wrap a Rust function so it can be converted into a JavaScript function
/// with `IntoJs` (`exports.export("name", Func::from(f))`).
pub struct Func<F, P>(pub F, PhantomData<P>);
/// A `FnMut` closure; calling it re-entrantly is an error.
pub struct MutFn<F>(RefCell<F>);
/// A `FnOnce` closure; calling it twice is an error.
pub struct OnceFn<F>(Cell<Option<F>>);

impl<T> core::ops::Deref for This<T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.0
    }
}
impl<T> core::ops::DerefMut for This<T> {
    fn deref_mut(&mut self) -> &mut T {
        &mut self.0
    }
}
impl<T> core::ops::Deref for Rest<T> {
    type Target = Vec<T>;
    fn deref(&self) -> &Vec<T> {
        &self.0
    }
}
impl<T> core::ops::DerefMut for Rest<T> {
    fn deref_mut(&mut self) -> &mut Vec<T> {
        &mut self.0
    }
}
impl<T> core::ops::Deref for Opt<T> {
    type Target = Option<T>;
    fn deref(&self) -> &Option<T> {
        &self.0
    }
}
impl<T> From<Vec<T>> for Rest<T> {
    fn from(v: Vec<T>) -> Self {
        Rest(v)
    }
}
impl<T> From<Option<T>> for Opt<T> {
    fn from(v: Option<T>) -> Self {
        Opt(v)
    }
}

impl<F, P> Func<F, P> {
    pub fn new(f: F) -> Self {
        Func(f, PhantomData)
    }
}
impl<F, P> From<F> for Func<F, P> {
    fn from(f: F) -> Self {
        Func(f, PhantomData)
    }
}
impl<F> MutFn<F> {
    pub fn new(f: F) -> Self {
        MutFn(RefCell::new(f))
    }
}
impl<F> From<F> for MutFn<F> {
    fn from(f: F) -> Self {
        MutFn::new(f)
    }
}
impl<F> OnceFn<F> {
    pub fn new(f: F) -> Self {
        OnceFn(Cell::new(Some(f)))
    }
}
impl<F> From<F> for OnceFn<F> {
    fn from(f: F) -> Self {
        OnceFn::new(f)
    }
}

impl<'js, F, P> IntoJs<'js> for Func<F, P>
where
    F: IntoJsFunc<'js, P> + 'js,
{
    fn into_js(self, ctx: &Ctx<'js>) -> Result<Value<'js>> {
        Ok(Function::new(*ctx, self.0)?.into_value())
    }
}

// ─── Args (outgoing) ─────────────────────────────────────────────────────

/// Arguments for [`Function::call_arg`].
pub struct Args<'js> {
    pub(crate) ctx: Ctx<'js>,
    pub(crate) this: Option<Value<'js>>,
    pub(crate) args: Vec<Value<'js>>,
}

impl<'js> Args<'js> {
    pub fn new(ctx: Ctx<'js>, capacity: usize) -> Self {
        Args { ctx, this: None, args: Vec::with_capacity(capacity) }
    }

    pub fn new_unsized(ctx: Ctx<'js>) -> Self {
        Args::new(ctx, 0)
    }

    pub fn push_arg<T: IntoJs<'js>>(&mut self, arg: T) -> Result<()> {
        self.args.push(arg.into_js(&self.ctx)?);
        Ok(())
    }

    pub fn push_args<T: IntoJs<'js>, I: IntoIterator<Item = T>>(&mut self, args: I) -> Result<()> {
        for arg in args {
            self.push_arg(arg)?;
        }
        Ok(())
    }

    pub fn this<T: IntoJs<'js>>(&mut self, this: T) -> Result<()> {
        self.this = Some(this.into_js(&self.ctx)?);
        Ok(())
    }

    pub fn len(&self) -> usize {
        self.args.len()
    }

    pub fn is_empty(&self) -> bool {
        self.args.is_empty()
    }

    /// Call `function` with these arguments.
    pub fn apply<R: FromJs<'js>>(self, function: &Function<'js>) -> Result<R> {
        function.call_arg(self)
    }
}

impl<'js> IntoArgs<'js> for Args<'js> {
    fn into_args(self, _ctx: &Ctx<'js>) -> Result<Args<'js>> {
        Ok(self)
    }
}

/// One element of an argument tuple: a plain value, [`This`] or [`Rest`].
pub trait IntoArg<'js> {
    fn into_arg(self, args: &mut Args<'js>) -> Result<()>;
}

impl<'js, T: IntoJs<'js>> IntoArg<'js> for T {
    fn into_arg(self, args: &mut Args<'js>) -> Result<()> {
        args.push_arg(self)
    }
}
impl<'js, T: IntoJs<'js>> IntoArg<'js> for This<T> {
    fn into_arg(self, args: &mut Args<'js>) -> Result<()> {
        args.this(self.0)
    }
}
impl<'js, T: IntoJs<'js>> IntoArg<'js> for Rest<T> {
    fn into_arg(self, args: &mut Args<'js>) -> Result<()> {
        args.push_args(self.0)
    }
}

impl<'js> IntoArgs<'js> for () {
    fn into_args(self, ctx: &Ctx<'js>) -> Result<Args<'js>> {
        Ok(Args::new(*ctx, 0))
    }
}

macro_rules! impl_into_args {
    ($($t:ident),*) => {
        impl<'js, $($t: IntoArg<'js>),*> IntoArgs<'js> for ($($t,)*) {
            #[allow(non_snake_case)]
            fn into_args(self, ctx: &Ctx<'js>) -> Result<Args<'js>> {
                let ($($t,)*) = self;
                let mut args = Args::new(*ctx, 0);
                $($t.into_arg(&mut args)?;)*
                Ok(args)
            }
        }
    };
}
impl_into_args!(A);
impl_into_args!(A, B);
impl_into_args!(A, B, C);
impl_into_args!(A, B, C, D);
impl_into_args!(A, B, C, D, E);
impl_into_args!(A, B, C, D, E, G);
impl_into_args!(A, B, C, D, E, G, H);
impl_into_args!(A, B, C, D, E, G, H, I);

// ─── Params (incoming) ───────────────────────────────────────────────────

/// The arguments of a call into a host function.
pub struct Params<'js> {
    ctx: Ctx<'js>,
    this: LazyValue<'js>,
    function: LazyValue<'js>,
    new_target: Value<'js>,
    args: Vec<Value<'js>>,
    index: usize,
}

/// A value JSC keeps alive for the duration of a host call, rooted by us
/// only when a callback actually asks for it (`this` / `function`).
struct LazyValue<'js> {
    raw: ffi::JSValueRef,
    value: std::cell::OnceCell<Value<'js>>,
}

impl<'js> LazyValue<'js> {
    fn owned(value: Value<'js>) -> Self {
        let raw = value.as_raw();
        LazyValue { raw, value: std::cell::OnceCell::from(value) }
    }

    /// # Safety
    /// `raw` must stay alive for as long as the `LazyValue` does.
    unsafe fn borrowed(raw: ffi::JSValueRef) -> Self {
        LazyValue { raw, value: std::cell::OnceCell::new() }
    }

    fn get(&self, ctx: Ctx<'js>) -> &Value<'js> {
        // SAFETY: `raw` is live per the `borrowed` contract.
        self.value.get_or_init(|| unsafe { Value::from_raw(ctx, self.raw) })
    }
}

impl<'js> Params<'js> {
    pub fn new(ctx: Ctx<'js>, this: Value<'js>, function: Value<'js>, args: Vec<Value<'js>>) -> Self {
        let new_target = Value::new_undefined(ctx);
        Params { ctx, this: LazyValue::owned(this), function: LazyValue::owned(function), new_target, args, index: 0 }
    }

    /// # Safety
    /// `this` and `function` must stay alive for the lifetime of the `Params`
    /// (they do inside a host-function callback: JSC holds them on the
    /// calling frame).
    pub(crate) unsafe fn from_borrowed(ctx: Ctx<'js>, this: ffi::JSValueRef, function: ffi::JSValueRef, args: Vec<Value<'js>>) -> Self {
        let new_target = Value::new_undefined(ctx);
        // SAFETY: forwarded from the caller.
        let (this, function) = unsafe { (LazyValue::borrowed(this), LazyValue::borrowed(function)) };
        Params { ctx, this, function, new_target, args, index: 0 }
    }

    pub fn with_new_target(mut self, new_target: Value<'js>) -> Self {
        self.new_target = new_target;
        self
    }

    pub fn ctx(&self) -> &Ctx<'js> {
        &self.ctx
    }

    pub fn this(&self) -> &Value<'js> {
        self.this.get(self.ctx)
    }

    pub fn function(&self) -> &Value<'js> {
        self.function.get(self.ctx)
    }

    pub fn new_target(&self) -> &Value<'js> {
        &self.new_target
    }

    pub fn is_constructor(&self) -> bool {
        !self.new_target.is_undefined()
    }

    pub fn len(&self) -> usize {
        self.args.len()
    }

    pub fn is_empty(&self) -> bool {
        self.args.is_empty()
    }

    /// Number of arguments not yet consumed.
    pub fn remaining(&self) -> usize {
        self.args.len().saturating_sub(self.index)
    }

    /// Take the next argument, if any.
    pub fn take(&mut self) -> Option<Value<'js>> {
        let slot = self.args.get_mut(self.index)?;
        // Move the argument out (each is handed to exactly one parameter)
        // rather than cloning it, which would re-root it in the GC.
        let value = core::mem::replace(slot, Value::new_undefined(self.ctx));
        self.index += 1;
        Some(value)
    }

    /// Take all remaining arguments.
    pub fn take_rest(&mut self) -> Vec<Value<'js>> {
        let start = self.index.min(self.args.len());
        self.index = self.args.len();
        self.args.drain(start..).collect()
    }

    pub fn check_params(&self, requirement: ParamRequirement) -> Result<()> {
        let given = self.args.len();
        if given < requirement.min {
            return Err(Error::NumArgs {
                expected: requirement.min..requirement.max.map(|m| m + 1).unwrap_or(usize::MAX),
                given,
            });
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ParamRequirement {
    pub min: usize,
    /// `None` = unbounded.
    pub max: Option<usize>,
}

impl ParamRequirement {
    pub const fn none() -> Self {
        ParamRequirement { min: 0, max: Some(0) }
    }
    pub const fn single() -> Self {
        ParamRequirement { min: 1, max: Some(1) }
    }
    pub const fn optional() -> Self {
        ParamRequirement { min: 0, max: Some(1) }
    }
    pub const fn any() -> Self {
        ParamRequirement { min: 0, max: None }
    }
    pub const fn combine(self, other: ParamRequirement) -> Self {
        ParamRequirement {
            min: self.min + other.min,
            max: match (self.max, other.max) {
                (Some(a), Some(b)) => Some(a + b),
                _ => None,
            },
        }
    }
}

/// A type that can be extracted from the parameters of a host function.
pub trait FromParam<'js>: Sized {
    fn param_requirement() -> ParamRequirement;
    fn from_param(params: &mut Params<'js>) -> Result<Self>;
}

impl<'js> FromParam<'js> for Ctx<'js> {
    fn param_requirement() -> ParamRequirement {
        ParamRequirement::none()
    }
    fn from_param(params: &mut Params<'js>) -> Result<Self> {
        Ok(*params.ctx())
    }
}

impl<'js, T: FromJs<'js>> FromParam<'js> for This<T> {
    fn param_requirement() -> ParamRequirement {
        ParamRequirement::none()
    }
    fn from_param(params: &mut Params<'js>) -> Result<Self> {
        let ctx = *params.ctx();
        Ok(This(T::from_js(&ctx, params.this().clone())?))
    }
}

impl<'js, T: FromJs<'js>> FromParam<'js> for Rest<T> {
    fn param_requirement() -> ParamRequirement {
        ParamRequirement::any()
    }
    fn from_param(params: &mut Params<'js>) -> Result<Self> {
        let ctx = *params.ctx();
        params.take_rest().into_iter().map(|v| T::from_js(&ctx, v)).collect::<Result<Vec<T>>>().map(Rest)
    }
}

impl<'js, T: FromJs<'js>> FromParam<'js> for Opt<T> {
    fn param_requirement() -> ParamRequirement {
        ParamRequirement::optional()
    }
    fn from_param(params: &mut Params<'js>) -> Result<Self> {
        let ctx = *params.ctx();
        match params.take() {
            Some(v) if !v.is_undefined() => Ok(Opt(Some(T::from_js(&ctx, v)?))),
            _ => Ok(Opt(None)),
        }
    }
}

impl<'js> FromParam<'js> for Exhaustive {
    fn param_requirement() -> ParamRequirement {
        ParamRequirement::none()
    }
    fn from_param(params: &mut Params<'js>) -> Result<Self> {
        if params.remaining() > 0 {
            return Err(Error::TooManyArgs);
        }
        Ok(Exhaustive)
    }
}

impl<'js, T: FromJs<'js>> FromParam<'js> for T {
    fn param_requirement() -> ParamRequirement {
        ParamRequirement::single()
    }
    fn from_param(params: &mut Params<'js>) -> Result<Self> {
        let ctx = *params.ctx();
        let value = params.take().unwrap_or_else(|| Value::new_undefined(ctx));
        T::from_js(&ctx, value)
    }
}

/// A Rust callable that can be exposed to JavaScript. Implemented for every
/// `Fn(A, B, …) -> R` whose parameters implement [`FromParam`] and whose
/// return implements [`IntoJs`] (`Result<T>` included); for [`MutFn`] / [`OnceFn`] wrappers
/// of `FnMut` / `FnOnce`; and for [`Async`] wrappers of functions returning
/// futures.
pub trait IntoJsFunc<'js, P> {
    fn param_requirements() -> ParamRequirement;
    fn call(&self, params: Params<'js>) -> Result<Value<'js>>;
}

macro_rules! impl_into_js_func {
    ($($p:ident),*) => {
        impl<'js, Fun, R, $($p),*> IntoJsFunc<'js, ($($p,)*)> for Fun
        where
            Fun: Fn($($p),*) -> R,
            R: IntoJs<'js>,
            $($p: FromParam<'js>,)*
        {
            fn param_requirements() -> ParamRequirement {
                ParamRequirement::none()$(.combine(<$p as FromParam<'js>>::param_requirement()))*
            }

            #[allow(non_snake_case, unused_mut, unused_variables)]
            fn call(&self, mut params: Params<'js>) -> Result<Value<'js>> {
                let ctx = *params.ctx();
                $(let $p = <$p as FromParam<'js>>::from_param(&mut params)?;)*
                (self)($($p),*).into_js(&ctx)
            }
        }

        impl<'js, Fun, R, $($p),*> IntoJsFunc<'js, ($($p,)*)> for MutFn<Fun>
        where
            Fun: FnMut($($p),*) -> R,
            R: IntoJs<'js>,
            $($p: FromParam<'js>,)*
        {
            fn param_requirements() -> ParamRequirement {
                ParamRequirement::none()$(.combine(<$p as FromParam<'js>>::param_requirement()))*
            }

            #[allow(non_snake_case, unused_mut, unused_variables)]
            fn call(&self, mut params: Params<'js>) -> Result<Value<'js>> {
                let ctx = *params.ctx();
                let mut f = self.0.try_borrow_mut().map_err(|_| Error::FunctionBorrow(BorrowError::AlreadyBorrowed))?;
                $(let $p = <$p as FromParam<'js>>::from_param(&mut params)?;)*
                (&mut *f)($($p),*).into_js(&ctx)
            }
        }

        impl<'js, Fun, R, $($p),*> IntoJsFunc<'js, ($($p,)*)> for OnceFn<Fun>
        where
            Fun: FnOnce($($p),*) -> R,
            R: IntoJs<'js>,
            $($p: FromParam<'js>,)*
        {
            fn param_requirements() -> ParamRequirement {
                ParamRequirement::none()$(.combine(<$p as FromParam<'js>>::param_requirement()))*
            }

            #[allow(non_snake_case, unused_mut, unused_variables)]
            fn call(&self, mut params: Params<'js>) -> Result<Value<'js>> {
                let ctx = *params.ctx();
                let f = self.0.take().ok_or(Error::FunctionBorrow(BorrowError::AlreadyUsed))?;
                $(let $p = <$p as FromParam<'js>>::from_param(&mut params)?;)*
                f($($p),*).into_js(&ctx)
            }
        }

        impl<'js, Fun, Fut, R, $($p),*> IntoJsFunc<'js, ($($p,)*)> for Async<Fun>
        where
            Fun: Fn($($p),*) -> Fut,
            Fut: Future<Output = R> + 'js,
            R: IntoJs<'js> + 'js,
            $($p: FromParam<'js>,)*
        {
            fn param_requirements() -> ParamRequirement {
                ParamRequirement::none()$(.combine(<$p as FromParam<'js>>::param_requirement()))*
            }

            #[allow(non_snake_case, unused_mut, unused_variables)]
            fn call(&self, mut params: Params<'js>) -> Result<Value<'js>> {
                let ctx = *params.ctx();
                $(let $p = <$p as FromParam<'js>>::from_param(&mut params)?;)*
                let future = (self.0)($($p),*);
                let (promise, resolve, reject) = Promise::new(&ctx)?;
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
    };
}
impl_into_js_func!();
impl_into_js_func!(A);
impl_into_js_func!(A, B);
impl_into_js_func!(A, B, C);
impl_into_js_func!(A, B, C, D);
impl_into_js_func!(A, B, C, D, E);
impl_into_js_func!(A, B, C, D, E, G);
impl_into_js_func!(A, B, C, D, E, G, H);
impl_into_js_func!(A, B, C, D, E, G, H, I);

// ─── Registry + trampoline ───────────────────────────────────────────────

pub(crate) type ErasedFn = dyn Fn(Params<'static>) -> Result<Value<'static>>;

thread_local! {
    static FUNCTIONS: RefCell<HashMap<usize, Rc<ErasedFn>>> = RefCell::new(HashMap::new());
    /// A panic raised inside a host callback. Unwinding cannot cross the C
    /// frames of JavaScriptCore, so the trampoline catches it, throws a JS
    /// exception to unwind the JS side, and the Rust entry point that called
    /// into JS resumes the panic once control is back (like rquickjs).
    static PENDING_PANIC: RefCell<Option<Box<dyn core::any::Any + Send>>> = const { RefCell::new(None) };
}

/// Resume a panic captured in a host callback, if any.
pub(crate) fn resume_pending_panic() {
    if let Some(payload) = PENDING_PANIC.with(|p| p.borrow_mut().take()) {
        std::panic::resume_unwind(payload);
    }
}

pub(crate) fn current_inner() -> Option<&'static RuntimeInner> {
    let ptr = CURRENT.with(|c| c.get());
    // SAFETY: set when the thread's runtime is created and never cleared
    // (the VM is process-lifetime); callbacks only run on that thread.
    unsafe { ptr.as_ref() }
}

pub(crate) unsafe extern "C" fn host_function_trampoline(
    _ctx: ffi::JSContextRef,
    function: ffi::JSObjectRef,
    this_object: ffi::JSObjectRef,
    argument_count: usize,
    arguments: *const ffi::JSValueRef,
    exception: *mut ffi::JSValueRef,
) -> ffi::JSValueRef {
    let Some(inner) = current_inner() else {
        return core::ptr::null();
    };
    let ctx: Ctx<'static> = Ctx::from_inner(inner);
    let Some(callback) = FUNCTIONS.with(|f| f.borrow().get(&(function as usize)).cloned()) else {
        let error = ctx.new_error("rbun: host function is no longer registered");
        // SAFETY: valid out pointer.
        unsafe { *exception = error.as_raw() };
        core::mem::forget(error);
        return core::ptr::null();
    };

    // SAFETY: JSC passes `argument_count` live values.
    let args: Vec<Value<'static>> = (0..argument_count)
        .map(|i| unsafe { Value::from_raw(ctx, *arguments.add(i)) })
        .collect();
    // SAFETY: `this_object` / `function` stay alive on JSC's calling frame
    // until we return, so `Params` roots them only on demand.
    let params = unsafe { Params::from_borrowed(ctx, this_object as ffi::JSValueRef, function as ffi::JSValueRef, args) };

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| callback(params)));
    let result = match result {
        Ok(result) => result,
        Err(payload) => {
            PENDING_PANIC.with(|p| *p.borrow_mut() = Some(payload));
            Err(crate::Exception::throw_message(&ctx, "rbun: host function panicked"))
        }
    };
    match result {
        Ok(value) => {
            // Dropping releases our protection; the raw value goes straight
            // back to JSC (and sits on the machine stack, which the GC scans
            // conservatively), so it cannot be collected in between.
            let raw = value.as_raw();
            drop(value);
            raw
        }
        Err(error) => {
            let thrown = error.throw(&ctx);
            let raw = thrown.as_raw();
            drop(thrown);
            // SAFETY: valid out pointer.
            unsafe { *exception = raw };
            core::ptr::null()
        }
    }
}

/// The [`Ctx`] of the runtime on this thread (for generated code that needs
/// a context inside a constructor closure).
pub fn current_ctx<'js>() -> Result<Ctx<'js>> {
    match current_inner() {
        // SAFETY: the runtime is process-lifetime on its thread; shortening
        // `'static` to the caller's `'js` is sound.
        Some(inner) => Ok(Ctx::from_inner(unsafe { &*(inner as *const RuntimeInner) })),
        None => Err(Error::Unknown),
    }
}

/// Register `callback` for a fresh host function object and return it.
pub(crate) fn make_host_function<'js>(ctx: Ctx<'js>, callback: Rc<dyn Fn(Params<'js>) -> Result<Value<'js>> + 'js>) -> Function<'js> {
    let name = JsString::new("");
    // SAFETY: valid context.
    let raw = unsafe { ffi::JSObjectMakeFunctionWithCallback(ctx.raw(), name.as_raw(), Some(host_function_trampoline)) };
    // Permanent protection so the registry key stays valid.
    // SAFETY: fresh object.
    unsafe { ffi::JSValueProtect(ctx.raw(), raw as ffi::JSValueRef) };
    // SAFETY: lifetime erasure. The runtime (and therefore everything a
    // `'js` closure may borrow) is process-lifetime on its thread, and the
    // registry is thread-local, so no callback can outlive its runtime.
    let callback: Rc<ErasedFn> = unsafe { core::mem::transmute(callback) };
    FUNCTIONS.with(|f| f.borrow_mut().insert(raw as usize, callback));
    // SAFETY: fresh object.
    Function(Object(unsafe { Value::from_raw(ctx, raw as ffi::JSValueRef) }))
}

impl<'js> Function<'js> {
    /// Create a JavaScript function from a Rust callable. The function (and
    /// the closure) live as long as the runtime.
    pub fn new<P, F>(ctx: Ctx<'js>, f: F) -> Result<Function<'js>>
    where
        F: IntoJsFunc<'js, P> + 'js,
    {
        Ok(make_host_function(ctx, Rc::new(move |params| f.call(params))))
    }

    /// Set the function's `name`.
    pub fn set_name<N: AsRef<str>>(&self, name: N) -> Result<()> {
        let set_name = self.ctx.function("setName")?;
        set_name.call::<_, ()>((self, name.as_ref()))
    }

    /// Set the function's `name`, builder style.
    pub fn with_name<N: AsRef<str>>(self, name: N) -> Result<Self> {
        self.set_name(name)?;
        Ok(self)
    }

    /// Set the function's `length`.
    pub fn set_length(&self, length: usize) -> Result<()> {
        let set_length = self.ctx.function("setLength")?;
        set_length.call::<_, ()>((self, length))
    }

    pub fn with_length(self, length: usize) -> Result<Self> {
        self.set_length(length)?;
        Ok(self)
    }

    pub fn name(&self) -> Result<std::string::String> {
        self.0.get("name")
    }

    /// Call with the given arguments (`(a, b)` or `(This(x), a, b)`).
    pub fn call<A: IntoArgs<'js>, R: FromJs<'js>>(&self, args: A) -> Result<R> {
        let args = args.into_args(&self.ctx)?;
        self.call_arg(args)
    }

    pub fn call_arg<R: FromJs<'js>>(&self, args: Args<'js>) -> Result<R> {
        let raw_args: Vec<ffi::JSValueRef> = args.args.iter().map(|a| a.raw).collect();
        let this = args.this.as_ref().map(|t| t.raw_object()).unwrap_or(core::ptr::null_mut());
        let mut exception: ffi::JSValueRef = core::ptr::null();
        // SAFETY: live function; args are live values.
        let raw = unsafe {
            ffi::JSObjectCallAsFunction(self.ctx.raw(), self.raw_object(), this, raw_args.len(), raw_args.as_ptr(), &mut exception)
        };
        resume_pending_panic();
        if !exception.is_null() {
            return Err(self.ctx.throw_raw(exception));
        }
        // SAFETY: value from the C API.
        let result = unsafe { Value::from_raw(self.ctx, raw) };
        // Calls through the C API bypass bun's `EventLoop::run_callback`, so
        // drain the microtasks a JS caller would have observed.
        self.ctx.drain_microtasks();
        R::from_js(&self.ctx, result)
    }

    /// Call with raw argument values (internal helper).
    pub(crate) fn call_raw(&self, raw_args: &[ffi::JSValueRef]) -> Result<Value<'js>> {
        let mut exception: ffi::JSValueRef = core::ptr::null();
        // SAFETY: live function; args are live values.
        let raw = unsafe {
            ffi::JSObjectCallAsFunction(self.ctx.raw(), self.raw_object(), core::ptr::null_mut(), raw_args.len(), raw_args.as_ptr(), &mut exception)
        };
        resume_pending_panic();
        if !exception.is_null() {
            return Err(self.ctx.throw_raw(exception));
        }
        // SAFETY: value from the C API.
        Ok(unsafe { Value::from_raw(self.ctx, raw) })
    }

    /// Queue a call to run as a pending job (see
    /// [`Runtime::execute_pending_job`](crate::Runtime::execute_pending_job)).
    pub fn defer<A: IntoArgs<'js>>(&self, args: A) -> Result<()> {
        let args = args.into_args(&self.ctx)?;
        let function = self.clone();
        let job: Box<dyn FnOnce() -> Result<()> + 'js> = Box::new(move || function.call_arg::<()>(args));
        // SAFETY: lifetime erasure — the runtime (and the queue) is
        // process-lifetime on this thread, so the job cannot outlive `'js`.
        let job: crate::runtime::DeferredJob = unsafe { core::mem::transmute(job) };
        self.ctx.inner.deferred.borrow_mut().push_back(job);
        Ok(())
    }

    /// `new this(...args)`.
    pub fn construct<A: IntoArgs<'js>, R: FromJs<'js>>(&self, args: A) -> Result<R> {
        self.0.construct(args)
    }

    pub fn is_constructor(&self) -> bool {
        self.0.is_constructor()
    }
}

impl<'js> Constructor<'js> {
    /// A constructor for the class `C`: `f` receives the constructor
    /// arguments and returns the instance (typically by converting a `C`
    /// through [`Class::instance`](crate::Class::instance) via `IntoJs`).
    /// Supports `class X extends C` through `new.target`.
    pub fn new_class<C, F, P>(ctx: Ctx<'js>, f: F) -> Result<Self>
    where
        C: crate::class::JsClass<'js> + 'js,
        F: IntoJsFunc<'js, P> + 'js,
    {
        let prototype = crate::class::Class::<C>::prototype(&ctx)?;
        let prototype = match prototype {
            Some(p) => p,
            None => Object::new(ctx)?,
        };
        Constructor::new_prototype(ctx, prototype, f)
    }

    /// A constructor whose instances get `prototype`; `f` builds the
    /// instance object.
    pub fn new_prototype<F, P>(ctx: Ctx<'js>, prototype: Object<'js>, f: F) -> Result<Self>
    where
        F: IntoJsFunc<'js, P> + 'js,
    {
        let factory = Function::new(ctx, f)?;
        let make_class = ctx.function("makeClass")?;
        let constructor: Function<'js> = make_class.call(("", factory, &prototype))?;
        Ok(Constructor(constructor))
    }

    /// `new this(...args)`.
    pub fn construct<A: IntoArgs<'js>, R: FromJs<'js>>(&self, args: A) -> Result<R> {
        self.0.construct(args)
    }

    pub fn call<A: IntoArgs<'js>, R: FromJs<'js>>(&self, args: A) -> Result<R> {
        self.0.call(args)
    }
}

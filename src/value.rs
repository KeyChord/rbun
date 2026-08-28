//! JavaScript values: [`Value`] and its typed views [`Object`], [`Function`],
//! [`Constructor`], [`Array`], [`Promise`], [`String`], [`Symbol`], [`BigInt`].
//!
//! Every value holds a GC protection for as long as it lives, so values can be
//! stored anywhere on the Rust side (boxed futures, thread-locals, …) without
//! JSC's conservative stack scan having to see them.

use core::ffi::c_uint;
use core::fmt;
use core::ops::Deref;

use crate::atom::{Atom, FromAtom, IntoAtom};
use crate::error::{Error, Exception, Result};
use crate::ffi;
use crate::runtime::Ctx;
use crate::string::JsString;

pub use crate::object::{Accessor, AsProperty, Filter, ObjectIter, Property};

/// The type of a value, following `rquickjs::Type`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Type {
    Uninitialized,
    Undefined,
    Null,
    Bool,
    Int,
    Float,
    String,
    Symbol,
    Array,
    Constructor,
    Function,
    Promise,
    Exception,
    Object,
    Module,
    BigInt,
    Unknown,
}

impl Type {
    pub fn as_str(self) -> &'static str {
        match self {
            Type::Uninitialized => "uninitialized",
            Type::Undefined => "undefined",
            Type::Null => "null",
            Type::Bool => "bool",
            Type::Int => "int",
            Type::Float => "float",
            Type::String => "string",
            Type::Symbol => "symbol",
            Type::Array => "array",
            Type::Constructor => "constructor",
            Type::Function => "function",
            Type::Promise => "promise",
            Type::Exception => "exception",
            Type::Object => "object",
            Type::Module => "module",
            Type::BigInt => "big_int",
            Type::Unknown => "unknown",
        }
    }

    /// Whether a value of type `self` can be treated as `other`
    /// (`Array` is an `Object`, `Int` is a `Float`, …).
    pub fn interpretable_as(self, other: Type) -> bool {
        if self == other {
            return true;
        }
        match other {
            Type::Object => matches!(self, Type::Array | Type::Constructor | Type::Function | Type::Promise | Type::Exception | Type::Object),
            Type::Function => matches!(self, Type::Constructor),
            Type::Float => matches!(self, Type::Int),
            _ => false,
        }
    }

    pub fn is_object(self) -> bool {
        self.interpretable_as(Type::Object)
    }
}

impl fmt::Display for Type {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

pub struct Value<'js> {
    pub(crate) ctx: Ctx<'js>,
    pub(crate) raw: ffi::JSValueRef,
}

impl<'js> Value<'js> {
    /// Wrap a raw value, taking a GC protection on it.
    pub unsafe fn from_raw(ctx: Ctx<'js>, raw: ffi::JSValueRef) -> Self {
        let raw = if raw.is_null() {
            // SAFETY: valid context.
            unsafe { ffi::JSValueMakeUndefined(ctx.raw()) }
        } else {
            raw
        };
        // SAFETY: `raw` is a live value on the current (locked) context.
        unsafe { ffi::JSValueProtect(ctx.raw(), raw) };
        Value { ctx, raw }
    }

    pub fn ctx(&self) -> &Ctx<'js> {
        &self.ctx
    }

    /// The raw `JSValueRef`, valid while this (or another) protection lives.
    pub fn as_raw(&self) -> ffi::JSValueRef {
        self.raw
    }

    /// The raw bits (`bun_jsc::JSValue` encoding).
    pub fn encoded(&self) -> usize {
        self.raw as usize
    }

    pub fn new_undefined(ctx: Ctx<'js>) -> Self {
        // SAFETY: valid context.
        unsafe { Value::from_raw(ctx, ffi::JSValueMakeUndefined(ctx.raw())) }
    }

    pub fn new_null(ctx: Ctx<'js>) -> Self {
        // SAFETY: valid context.
        unsafe { Value::from_raw(ctx, ffi::JSValueMakeNull(ctx.raw())) }
    }

    /// Alias of [`new_undefined`](Self::new_undefined) (QuickJS' uninitialized
    /// value has no JSC counterpart).
    pub fn new_uninitialized(ctx: Ctx<'js>) -> Self {
        Value::new_undefined(ctx)
    }

    pub fn new_bool(ctx: Ctx<'js>, value: bool) -> Self {
        // SAFETY: valid context.
        unsafe { Value::from_raw(ctx, ffi::JSValueMakeBoolean(ctx.raw(), value)) }
    }

    pub fn new_int(ctx: Ctx<'js>, value: i32) -> Self {
        Value::new_number(ctx, value as f64)
    }

    pub fn new_float(ctx: Ctx<'js>, value: f64) -> Self {
        Value::new_number(ctx, value)
    }

    pub fn new_number(ctx: Ctx<'js>, value: f64) -> Self {
        // SAFETY: valid context.
        unsafe { Value::from_raw(ctx, ffi::JSValueMakeNumber(ctx.raw(), value)) }
    }

    pub fn new_big_int(ctx: Ctx<'js>, value: i64) -> Self {
        BigInt::from_i64(ctx, value).map(BigInt::into_value).unwrap_or_else(|_| Value::new_undefined(ctx))
    }

    pub fn from_string(string: String<'js>) -> Self {
        string.into_value()
    }

    pub fn from_object(object: Object<'js>) -> Self {
        object.into_value()
    }

    pub fn from_function(function: Function<'js>) -> Self {
        function.into_value()
    }

    pub fn from_array(array: Array<'js>) -> Self {
        array.into_value()
    }

    pub fn from_symbol(symbol: Symbol<'js>) -> Self {
        symbol.into_value()
    }

    pub fn from_big_int(big_int: BigInt<'js>) -> Self {
        big_int.into_value()
    }

    pub fn from_exception(exception: Exception<'js>) -> Self {
        exception.into_value()
    }

    pub fn from_promise(promise: Promise<'js>) -> Self {
        promise.into_value()
    }

    fn raw_type(&self) -> ffi::JSType {
        // SAFETY: live value.
        unsafe { ffi::JSValueGetType(self.ctx.raw(), self.raw) }
    }

    pub fn type_of(&self) -> Type {
        match self.raw_type() {
            ffi::kJSTypeUndefined => Type::Undefined,
            ffi::kJSTypeNull => Type::Null,
            ffi::kJSTypeBoolean => Type::Bool,
            ffi::kJSTypeNumber => {
                let n = self.as_number().unwrap_or(f64::NAN);
                if n.fract() == 0.0 && n.abs() <= (i32::MAX as f64) && !(n == 0.0 && n.is_sign_negative()) {
                    Type::Int
                } else {
                    Type::Float
                }
            }
            ffi::kJSTypeString => Type::String,
            ffi::kJSTypeSymbol => Type::Symbol,
            ffi::kJSTypeBigInt => Type::BigInt,
            ffi::kJSTypeObject => {
                if self.is_array() {
                    Type::Array
                } else if self.is_function() {
                    if self.is_constructor() { Type::Constructor } else { Type::Function }
                } else if self.is_promise() {
                    Type::Promise
                } else if self.is_error() {
                    Type::Exception
                } else {
                    Type::Object
                }
            }
            _ => Type::Unknown,
        }
    }

    pub fn type_name(&self) -> &'static str {
        self.type_of().as_str()
    }

    pub fn is_undefined(&self) -> bool {
        self.raw_type() == ffi::kJSTypeUndefined
    }
    pub fn is_uninitialized(&self) -> bool {
        false
    }
    pub fn is_null(&self) -> bool {
        self.raw_type() == ffi::kJSTypeNull
    }
    pub fn is_undefined_or_null(&self) -> bool {
        matches!(self.raw_type(), ffi::kJSTypeUndefined | ffi::kJSTypeNull)
    }
    pub fn is_bool(&self) -> bool {
        self.raw_type() == ffi::kJSTypeBoolean
    }
    pub fn is_number(&self) -> bool {
        self.raw_type() == ffi::kJSTypeNumber
    }
    pub fn is_int(&self) -> bool {
        self.type_of() == Type::Int
    }
    pub fn is_float(&self) -> bool {
        self.type_of() == Type::Float
    }
    pub fn is_string(&self) -> bool {
        self.raw_type() == ffi::kJSTypeString
    }
    pub fn is_symbol(&self) -> bool {
        self.raw_type() == ffi::kJSTypeSymbol
    }
    pub fn is_big_int(&self) -> bool {
        self.raw_type() == ffi::kJSTypeBigInt
    }
    pub fn is_object(&self) -> bool {
        self.raw_type() == ffi::kJSTypeObject
    }
    pub fn is_module(&self) -> bool {
        false
    }
    pub fn is_function(&self) -> bool {
        // SAFETY: live value; object check first.
        self.is_object() && unsafe { ffi::JSObjectIsFunction(self.ctx.raw(), self.raw as ffi::JSObjectRef) }
    }
    pub fn is_constructor(&self) -> bool {
        // SAFETY: live value; object check first.
        self.is_object() && unsafe { ffi::JSObjectIsConstructor(self.ctx.raw(), self.raw as ffi::JSObjectRef) }
    }
    pub fn is_array(&self) -> bool {
        // SAFETY: live value.
        unsafe { ffi::JSValueIsArray(self.ctx.raw(), self.raw) }
    }
    pub fn is_promise(&self) -> bool {
        // SAFETY: bun's probe only inspects the cell tag of an object.
        self.is_object() && unsafe { ffi::bun_embed_promise_status(self.encoded()) } >= 0
    }
    pub fn is_date(&self) -> bool {
        // SAFETY: live value.
        unsafe { ffi::JSValueIsDate(self.ctx.raw(), self.raw) }
    }
    pub fn is_error(&self) -> bool {
        if !self.is_object() {
            return false;
        }
        let Ok(error_ctor) = self.ctx.globals().get::<_, Value<'js>>("Error") else {
            return false;
        };
        let mut exception: ffi::JSValueRef = core::ptr::null();
        // SAFETY: live values.
        let result = unsafe {
            ffi::JSValueIsInstanceOfConstructor(self.ctx.raw(), self.raw, error_ctor.raw as ffi::JSObjectRef, &mut exception)
        };
        result && exception.is_null()
    }
    pub fn is_exception(&self) -> bool {
        self.is_error()
    }

    pub fn as_bool(&self) -> Option<bool> {
        // SAFETY: live value.
        self.is_bool().then(|| unsafe { ffi::JSValueToBoolean(self.ctx.raw(), self.raw) })
    }

    pub fn as_number(&self) -> Option<f64> {
        if !self.is_number() {
            return None;
        }
        let mut exception: ffi::JSValueRef = core::ptr::null();
        // SAFETY: live value.
        let n = unsafe { ffi::JSValueToNumber(self.ctx.raw(), self.raw, &mut exception) };
        exception.is_null().then_some(n)
    }

    pub fn as_int(&self) -> Option<i32> {
        self.as_number().filter(|n| n.fract() == 0.0 && n.abs() <= i32::MAX as f64).map(|n| n as i32)
    }

    pub fn as_float(&self) -> Option<f64> {
        self.as_number()
    }

    pub fn as_string(&self) -> Option<&String<'js>> {
        // SAFETY: `String` is `repr(transparent)` over `Value`.
        self.is_string().then(|| unsafe { &*(self as *const Value<'js> as *const String<'js>) })
    }

    pub fn as_symbol(&self) -> Option<&Symbol<'js>> {
        // SAFETY: `Symbol` is `repr(transparent)` over `Value`.
        self.is_symbol().then(|| unsafe { &*(self as *const Value<'js> as *const Symbol<'js>) })
    }

    pub fn as_big_int(&self) -> Option<&BigInt<'js>> {
        // SAFETY: `BigInt` is `repr(transparent)` over `Value`.
        self.is_big_int().then(|| unsafe { &*(self as *const Value<'js> as *const BigInt<'js>) })
    }

    pub fn as_object(&self) -> Option<&Object<'js>> {
        // SAFETY: `Object` is `repr(transparent)` over `Value`.
        self.is_object().then(|| unsafe { &*(self as *const Value<'js> as *const Object<'js>) })
    }

    pub fn as_function(&self) -> Option<&Function<'js>> {
        // SAFETY: `Function` is `repr(transparent)` over `Value`.
        self.is_function().then(|| unsafe { &*(self as *const Value<'js> as *const Function<'js>) })
    }

    pub fn as_constructor(&self) -> Option<&Constructor<'js>> {
        // SAFETY: `Constructor` is `repr(transparent)` over `Value`.
        self.is_constructor().then(|| unsafe { &*(self as *const Value<'js> as *const Constructor<'js>) })
    }

    pub fn as_array(&self) -> Option<&Array<'js>> {
        // SAFETY: `Array` is `repr(transparent)` over `Value`.
        self.is_array().then(|| unsafe { &*(self as *const Value<'js> as *const Array<'js>) })
    }

    pub fn as_promise(&self) -> Option<&Promise<'js>> {
        // SAFETY: `Promise` is `repr(transparent)` over `Value`.
        self.is_promise().then(|| unsafe { &*(self as *const Value<'js> as *const Promise<'js>) })
    }

    pub fn as_exception(&self) -> Option<&Exception<'js>> {
        // SAFETY: `Exception` is `repr(transparent)` over `Object` over `Value`.
        self.is_error().then(|| unsafe { &*(self as *const Value<'js> as *const Exception<'js>) })
    }

    pub fn into_string(self) -> Option<String<'js>> {
        self.try_into_string().ok()
    }
    pub fn into_symbol(self) -> Option<Symbol<'js>> {
        self.try_into_symbol().ok()
    }
    pub fn into_big_int(self) -> Option<BigInt<'js>> {
        self.try_into_big_int().ok()
    }
    pub fn into_object(self) -> Option<Object<'js>> {
        self.try_into_object().ok()
    }
    pub fn into_function(self) -> Option<Function<'js>> {
        self.try_into_function().ok()
    }
    pub fn into_constructor(self) -> Option<Constructor<'js>> {
        self.try_into_constructor().ok()
    }
    pub fn into_array(self) -> Option<Array<'js>> {
        self.try_into_array().ok()
    }
    pub fn into_promise(self) -> Option<Promise<'js>> {
        self.try_into_promise().ok()
    }
    pub fn into_exception(self) -> Option<Exception<'js>> {
        self.try_into_exception().ok()
    }

    pub fn try_into_string(self) -> core::result::Result<String<'js>, Self> {
        if self.is_string() { Ok(String(self)) } else { Err(self) }
    }
    pub fn try_into_symbol(self) -> core::result::Result<Symbol<'js>, Self> {
        if self.is_symbol() { Ok(Symbol(self)) } else { Err(self) }
    }
    pub fn try_into_big_int(self) -> core::result::Result<BigInt<'js>, Self> {
        if self.is_big_int() { Ok(BigInt(self)) } else { Err(self) }
    }
    pub fn try_into_object(self) -> core::result::Result<Object<'js>, Self> {
        if self.is_object() { Ok(Object(self)) } else { Err(self) }
    }
    pub fn try_into_function(self) -> core::result::Result<Function<'js>, Self> {
        if self.is_function() { Ok(Function(Object(self))) } else { Err(self) }
    }
    pub fn try_into_constructor(self) -> core::result::Result<Constructor<'js>, Self> {
        if self.is_constructor() { Ok(Constructor(Function(Object(self)))) } else { Err(self) }
    }
    pub fn try_into_array(self) -> core::result::Result<Array<'js>, Self> {
        if self.is_array() { Ok(Array(Object(self))) } else { Err(self) }
    }
    pub fn try_into_promise(self) -> core::result::Result<Promise<'js>, Self> {
        if self.is_promise() { Ok(Promise(Object(self))) } else { Err(self) }
    }
    pub fn try_into_exception(self) -> core::result::Result<Exception<'js>, Self> {
        if self.is_error() { Ok(Exception(Object(self))) } else { Err(self) }
    }

    /// `String(value)`; never throws (failures collapse to `[exception]`).
    pub fn to_string_lossy(&self) -> std::string::String {
        let mut exception: ffi::JSValueRef = core::ptr::null();
        // SAFETY: live value.
        let raw = unsafe { ffi::JSValueToStringCopy(self.ctx.raw(), self.raw, &mut exception) };
        // SAFETY: owned string or null.
        match unsafe { JsString::from_raw(raw) } {
            Some(s) if exception.is_null() => s.to_rust_string(),
            _ => "[exception]".to_string(),
        }
    }

    /// `JSON.stringify(value)`; `Ok(None)` when not representable.
    pub fn to_json(&self) -> Result<Option<std::string::String>> {
        self.ctx.json_stringify_to_rust(self)
    }

    /// Convert into a Rust type.
    pub fn get<T: FromJs<'js>>(&self) -> Result<T> {
        T::from_js(&self.ctx, self.clone())
    }

    pub(crate) fn raw_object(&self) -> ffi::JSObjectRef {
        self.raw as ffi::JSObjectRef
    }
}

impl Clone for Value<'_> {
    fn clone(&self) -> Self {
        // SAFETY: live value; protections are counted.
        unsafe { ffi::JSValueProtect(self.ctx.raw(), self.raw) };
        Value { ctx: self.ctx, raw: self.raw }
    }
}

impl Drop for Value<'_> {
    fn drop(&mut self) {
        // SAFETY: balances the protect taken in `from_raw` / `clone`.
        unsafe { ffi::JSValueUnprotect(self.ctx.raw(), self.raw) };
    }
}

impl fmt::Debug for Value<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.type_of() {
            Type::String => write!(f, "{:?}", self.to_string_lossy()),
            Type::Object | Type::Array => match self.to_json() {
                Ok(Some(json)) if json.len() <= 512 => write!(f, "{json}"),
                _ => write!(f, "[{}]", self.to_string_lossy()),
            },
            Type::Exception => write!(f, "[{}]", self.to_string_lossy()),
            Type::Symbol => write!(f, "{}", self.to_string_lossy()),
            _ => write!(f, "{}", self.to_string_lossy()),
        }
    }
}

impl<'js> AsRef<Value<'js>> for Value<'js> {
    fn as_ref(&self) -> &Value<'js> {
        self
    }
}

impl PartialEq for Value<'_> {
    fn eq(&self, other: &Self) -> bool {
        // SAFETY: live values.
        unsafe { ffi::JSValueIsStrictEqual(self.ctx.raw(), self.raw, other.raw) }
    }
}

macro_rules! newtype_common {
    ($t:ident($inner:ident), $inner_ty:ty) => {
        impl<'js> $t<'js> {
            pub fn ctx(&self) -> &Ctx<'js> {
                self.as_value().ctx()
            }
            pub fn as_raw(&self) -> ffi::JSValueRef {
                self.as_value().as_raw()
            }
        }
        impl<'js> Deref for $t<'js> {
            type Target = $inner_ty;
            fn deref(&self) -> &$inner_ty {
                &self.0
            }
        }
        impl<'js> AsRef<Value<'js>> for $t<'js> {
            fn as_ref(&self) -> &Value<'js> {
                self.as_value()
            }
        }
        impl PartialEq for $t<'_> {
            fn eq(&self, other: &Self) -> bool {
                self.as_value() == other.as_value()
            }
        }
    };
}

// ─── String ──────────────────────────────────────────────────────────────

/// A JavaScript string value.
#[repr(transparent)]
#[derive(Clone)]
pub struct String<'js>(pub(crate) Value<'js>);

impl<'js> String<'js> {
    pub fn from_str(ctx: Ctx<'js>, value: &str) -> Result<Self> {
        Ok(String(ctx.string(value)))
    }

    /// Convert to a Rust string.
    #[allow(clippy::inherent_to_string)]
    pub fn to_string(&self) -> Result<std::string::String> {
        Ok(self.0.to_string_lossy())
    }

    pub fn into_value(self) -> Value<'js> {
        self.0
    }

    pub fn as_value(&self) -> &Value<'js> {
        &self.0
    }
}
newtype_common!(String(Value), Value<'js>);

impl fmt::Debug for String<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self.0.to_string_lossy())
    }
}

// ─── Symbol ──────────────────────────────────────────────────────────────

#[repr(transparent)]
#[derive(Clone)]
pub struct Symbol<'js>(pub(crate) Value<'js>);

impl<'js> Symbol<'js> {
    /// `Symbol(description)`.
    pub fn new(ctx: Ctx<'js>, description: &str) -> Result<Self> {
        let description = JsString::new(description);
        // SAFETY: valid context and string.
        let raw = unsafe { ffi::JSValueMakeSymbol(ctx.raw(), description.as_raw()) };
        // SAFETY: fresh value.
        Ok(Symbol(unsafe { Value::from_raw(ctx, raw) }))
    }

    fn well_known(ctx: Ctx<'js>, name: &str) -> Symbol<'js> {
        let symbol: Object<'js> = ctx.globals().get("Symbol").expect("Symbol global");
        symbol.get::<_, Symbol<'js>>(name).expect("well-known symbol")
    }

    pub fn iterator(ctx: Ctx<'js>) -> Symbol<'js> {
        Symbol::well_known(ctx, "iterator")
    }
    pub fn async_iterator(ctx: Ctx<'js>) -> Symbol<'js> {
        Symbol::well_known(ctx, "asyncIterator")
    }
    pub fn has_instance(ctx: Ctx<'js>) -> Symbol<'js> {
        Symbol::well_known(ctx, "hasInstance")
    }
    pub fn to_primitive(ctx: Ctx<'js>) -> Symbol<'js> {
        Symbol::well_known(ctx, "toPrimitive")
    }
    pub fn to_string_tag(ctx: Ctx<'js>) -> Symbol<'js> {
        Symbol::well_known(ctx, "toStringTag")
    }
    pub fn species(ctx: Ctx<'js>) -> Symbol<'js> {
        Symbol::well_known(ctx, "species")
    }
    pub fn unscopables(ctx: Ctx<'js>) -> Symbol<'js> {
        Symbol::well_known(ctx, "unscopables")
    }

    /// `symbol.description` (`undefined` when there is none).
    pub fn description(&self) -> Result<Value<'js>> {
        let ctx = *self.ctx();
        ctx.function("symbolDescription")?.call((self.as_value(),))
    }

    pub fn into_value(self) -> Value<'js> {
        self.0
    }

    pub fn as_value(&self) -> &Value<'js> {
        &self.0
    }
}
newtype_common!(Symbol(Value), Value<'js>);

impl fmt::Debug for Symbol<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0.to_string_lossy())
    }
}

// ─── BigInt ──────────────────────────────────────────────────────────────

#[repr(transparent)]
#[derive(Clone)]
pub struct BigInt<'js>(pub(crate) Value<'js>);

impl<'js> BigInt<'js> {
    pub fn from_i64(ctx: Ctx<'js>, value: i64) -> Result<Self> {
        let mut exception: ffi::JSValueRef = core::ptr::null();
        // SAFETY: valid context.
        let raw = unsafe { ffi::JSBigIntCreateWithInt64(ctx.raw(), value, &mut exception) };
        if !exception.is_null() {
            return Err(ctx.throw_raw(exception));
        }
        // SAFETY: fresh value.
        Ok(BigInt(unsafe { Value::from_raw(ctx, raw) }))
    }

    pub fn from_u64(ctx: Ctx<'js>, value: u64) -> Result<Self> {
        let mut exception: ffi::JSValueRef = core::ptr::null();
        // SAFETY: valid context.
        let raw = unsafe { ffi::JSBigIntCreateWithUInt64(ctx.raw(), value, &mut exception) };
        if !exception.is_null() {
            return Err(ctx.throw_raw(exception));
        }
        // SAFETY: fresh value.
        Ok(BigInt(unsafe { Value::from_raw(ctx, raw) }))
    }

    pub fn to_i64(&self) -> Result<i64> {
        let ctx = *self.ctx();
        let mut exception: ffi::JSValueRef = core::ptr::null();
        // SAFETY: live value.
        let n = unsafe { ffi::JSValueToInt64(ctx.raw(), self.0.raw, &mut exception) };
        if !exception.is_null() {
            return Err(ctx.throw_raw(exception));
        }
        Ok(n)
    }

    pub fn to_u64(&self) -> Result<u64> {
        let ctx = *self.ctx();
        let mut exception: ffi::JSValueRef = core::ptr::null();
        // SAFETY: live value.
        let n = unsafe { ffi::JSValueToUInt64(ctx.raw(), self.0.raw, &mut exception) };
        if !exception.is_null() {
            return Err(ctx.throw_raw(exception));
        }
        Ok(n)
    }

    pub fn into_value(self) -> Value<'js> {
        self.0
    }

    pub fn as_value(&self) -> &Value<'js> {
        &self.0
    }
}
newtype_common!(BigInt(Value), Value<'js>);

impl fmt::Debug for BigInt<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}n", self.0.to_string_lossy())
    }
}

// ─── Object ──────────────────────────────────────────────────────────────

#[repr(transparent)]
#[derive(Clone)]
pub struct Object<'js>(pub(crate) Value<'js>);

impl<'js> Object<'js> {
    pub fn new(ctx: Ctx<'js>) -> Result<Self> {
        // SAFETY: valid context.
        let raw = unsafe { ffi::JSObjectMake(ctx.raw(), core::ptr::null_mut(), core::ptr::null_mut()) };
        // SAFETY: fresh object.
        Ok(Object(unsafe { Value::from_raw(ctx, raw) }))
    }

    pub fn into_value(self) -> Value<'js> {
        self.0
    }

    pub fn as_value(&self) -> &Value<'js> {
        &self.0
    }

    /// Alias of [`into_value`](Self::into_value).
    pub fn into_inner(self) -> Value<'js> {
        self.0
    }

    pub fn get<K: IntoAtom<'js>, V: FromJs<'js>>(&self, key: K) -> Result<V> {
        let key = key.into_atom(&self.ctx)?;
        let mut exception: ffi::JSValueRef = core::ptr::null();
        // SAFETY: live object and key.
        let raw = unsafe { ffi::JSObjectGetPropertyForKey(self.ctx.raw(), self.raw_object(), key.0.raw, &mut exception) };
        crate::function::resume_pending_panic();
        if !exception.is_null() {
            return Err(self.ctx.throw_raw(exception));
        }
        // SAFETY: value from the C API.
        V::from_js(&self.ctx, unsafe { Value::from_raw(self.ctx, raw) })
    }

    pub fn set<K: IntoAtom<'js>, V: IntoJs<'js>>(&self, key: K, value: V) -> Result<()> {
        let key = key.into_atom(&self.ctx)?;
        let value = value.into_js(&self.ctx)?;
        // Strict-mode semantics: assigning to a read-only property throws,
        // matching rquickjs (`'key' is read-only`).
        let set = self.ctx.function("setStrict")?;
        set.call_raw(&[self.raw, key.0.raw, value.raw])?;
        Ok(())
    }

    pub fn contains_key<K: IntoAtom<'js>>(&self, key: K) -> Result<bool> {
        let key = key.into_atom(&self.ctx)?;
        let mut exception: ffi::JSValueRef = core::ptr::null();
        // SAFETY: live object and key.
        let has = unsafe { ffi::JSObjectHasPropertyForKey(self.ctx.raw(), self.raw_object(), key.0.raw, &mut exception) };
        if !exception.is_null() {
            return Err(self.ctx.throw_raw(exception));
        }
        Ok(has)
    }

    pub fn remove<K: IntoAtom<'js>>(&self, key: K) -> Result<()> {
        let key = key.into_atom(&self.ctx)?;
        let mut exception: ffi::JSValueRef = core::ptr::null();
        // SAFETY: live object and key.
        unsafe { ffi::JSObjectDeletePropertyForKey(self.ctx.raw(), self.raw_object(), key.0.raw, &mut exception) };
        if !exception.is_null() {
            return Err(self.ctx.throw_raw(exception));
        }
        Ok(())
    }

    /// Own enumerable string keys.
    pub fn keys<K: FromAtom<'js>>(&self) -> ObjectKeysIter<'js, K> {
        self.own_keys(Filter::new().string().enum_only())
    }

    /// Own enumerable string properties.
    pub fn props<K: FromAtom<'js>, V: FromJs<'js>>(&self) -> ObjectIter<'js, K, V> {
        self.own_props(Filter::new().string().enum_only())
    }

    /// Own keys matching `filter`.
    pub fn own_keys<K: FromAtom<'js>>(&self, filter: Filter) -> ObjectKeysIter<'js, K> {
        ObjectKeysIter { keys: crate::object::own_key_values(self, filter), _marker: core::marker::PhantomData }
    }

    /// Own properties matching `filter`.
    pub fn own_props<K: FromAtom<'js>, V: FromJs<'js>>(&self, filter: Filter) -> ObjectIter<'js, K, V> {
        ObjectIter { object: self.clone(), keys: crate::object::own_key_values(self, filter), _marker: core::marker::PhantomData }
    }

    /// Define a property with a descriptor (see [`Property`] / [`Accessor`]).
    pub fn prop<K: IntoAtom<'js>, P: AsProperty<'js>>(&self, key: K, prop: P) -> Result<()> {
        let key = key.into_atom(&self.ctx)?;
        prop.define(&self.ctx, self, &key)
    }

    pub fn prototype(&self) -> Option<Object<'js>> {
        // SAFETY: live object.
        let raw = unsafe { ffi::JSObjectGetPrototype(self.ctx.raw(), self.raw_object()) };
        // SAFETY: value from the C API.
        unsafe { Value::from_raw(self.ctx, raw) }.into_object()
    }

    pub fn set_prototype(&self, prototype: Option<&Object<'js>>) -> Result<()> {
        let value = match prototype {
            Some(p) => p.0.clone(),
            None => Value::new_null(self.ctx),
        };
        // SAFETY: live objects.
        unsafe { ffi::JSObjectSetPrototype(self.ctx.raw(), self.raw_object(), value.raw) };
        Ok(())
    }

    pub fn is_instance_of(&self, class: impl AsRef<Value<'js>>) -> bool {
        let class = class.as_ref();
        if !class.is_object() {
            return false;
        }
        let mut exception: ffi::JSValueRef = core::ptr::null();
        // SAFETY: live values.
        let result = unsafe { ffi::JSValueIsInstanceOfConstructor(self.ctx.raw(), self.raw, class.raw_object(), &mut exception) };
        result && exception.is_null()
    }

    /// `new this(...args)`.
    pub fn construct<A: IntoArgs<'js>, R: FromJs<'js>>(&self, args: A) -> Result<R> {
        let args = args.into_args(&self.ctx)?;
        let raw_args: Vec<ffi::JSValueRef> = args.args.iter().map(|a| a.raw).collect();
        let mut exception: ffi::JSValueRef = core::ptr::null();
        // SAFETY: live object; args are live values.
        let raw = unsafe {
            ffi::JSObjectCallAsConstructor(self.ctx.raw(), self.raw_object(), raw_args.len(), raw_args.as_ptr(), &mut exception)
        };
        crate::function::resume_pending_panic();
        if !exception.is_null() {
            return Err(self.ctx.throw_raw(exception));
        }
        // SAFETY: object from the C API.
        R::from_js(&self.ctx, unsafe { Value::from_raw(self.ctx, raw) })
    }
}
newtype_common!(Object(Value), Value<'js>);

impl<'js> IntoIterator for Object<'js> {
    type Item = Result<(Atom<'js>, Value<'js>)>;
    type IntoIter = ObjectIter<'js, Atom<'js>, Value<'js>>;
    fn into_iter(self) -> Self::IntoIter {
        let keys = crate::object::own_key_values(&self, Filter::new().string().enum_only());
        ObjectIter { object: self, keys, _marker: core::marker::PhantomData }
    }
}

/// Iterator over object keys (see [`Object::keys`]).
pub struct ObjectKeysIter<'js, K> {
    keys: std::vec::IntoIter<Atom<'js>>,
    _marker: core::marker::PhantomData<K>,
}

impl<'js, K: FromAtom<'js>> Iterator for ObjectKeysIter<'js, K> {
    type Item = Result<K>;
    fn next(&mut self) -> Option<Self::Item> {
        self.keys.next().map(K::from_atom)
    }
}

impl fmt::Debug for Object<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.0, f)
    }
}

// ─── Function / Constructor (behaviour in function.rs) ───────────────────

#[repr(transparent)]
#[derive(Clone)]
pub struct Function<'js>(pub(crate) Object<'js>);

impl<'js> Function<'js> {
    pub fn into_value(self) -> Value<'js> {
        self.0.0
    }
    pub fn as_value(&self) -> &Value<'js> {
        &self.0.0
    }
    pub fn as_object(&self) -> &Object<'js> {
        &self.0
    }
    pub fn into_object(self) -> Object<'js> {
        self.0
    }
    /// The underlying object (rquickjs naming).
    pub fn into_inner(self) -> Object<'js> {
        self.0
    }
    pub fn as_inner(&self) -> &Object<'js> {
        &self.0
    }
}
newtype_common!(Function(Object), Object<'js>);

impl fmt::Debug for Function<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[function {}]", self.0.get::<_, Value<'_>>("name").map(|v| v.to_string_lossy()).unwrap_or_default())
    }
}

/// A function usable with `new`.
#[repr(transparent)]
#[derive(Clone)]
pub struct Constructor<'js>(pub(crate) Function<'js>);

impl<'js> Constructor<'js> {
    pub fn into_value(self) -> Value<'js> {
        self.0.0.0
    }
    pub fn as_value(&self) -> &Value<'js> {
        &self.0.0.0
    }
    pub fn as_function(&self) -> &Function<'js> {
        &self.0
    }
    pub fn into_function(self) -> Function<'js> {
        self.0
    }
    pub fn as_object(&self) -> &Object<'js> {
        &self.0.0
    }
    pub fn into_object(self) -> Object<'js> {
        self.0.0
    }
    pub fn into_inner(self) -> Function<'js> {
        self.0
    }
    pub fn as_inner(&self) -> &Function<'js> {
        &self.0
    }
    pub fn from_function(function: Function<'js>) -> Option<Self> {
        function.is_constructor().then(|| Constructor(function))
    }
}
newtype_common!(Constructor(Function), Function<'js>);

impl fmt::Debug for Constructor<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[constructor {}]", self.0.0.get::<_, Value<'_>>("name").map(|v| v.to_string_lossy()).unwrap_or_default())
    }
}

// ─── Array ───────────────────────────────────────────────────────────────

#[repr(transparent)]
#[derive(Clone)]
pub struct Array<'js>(pub(crate) Object<'js>);

impl<'js> Array<'js> {
    pub fn new(ctx: Ctx<'js>) -> Result<Self> {
        let mut exception: ffi::JSValueRef = core::ptr::null();
        // SAFETY: valid context.
        let raw = unsafe { ffi::JSObjectMakeArray(ctx.raw(), 0, core::ptr::null(), &mut exception) };
        if !exception.is_null() {
            return Err(ctx.throw_raw(exception));
        }
        // SAFETY: fresh array.
        Ok(Array(Object(unsafe { Value::from_raw(ctx, raw) })))
    }

    pub fn into_value(self) -> Value<'js> {
        self.0.0
    }
    pub fn as_value(&self) -> &Value<'js> {
        &self.0.0
    }
    pub fn as_object(&self) -> &Object<'js> {
        &self.0
    }
    pub fn into_object(self) -> Object<'js> {
        self.0
    }
    pub fn into_inner(self) -> Object<'js> {
        self.0
    }
    pub fn as_inner(&self) -> &Object<'js> {
        &self.0
    }

    pub fn len(&self) -> usize {
        self.0.get::<_, f64>("length").map(|n| n as usize).unwrap_or(0)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn get<V: FromJs<'js>>(&self, index: usize) -> Result<V> {
        let mut exception: ffi::JSValueRef = core::ptr::null();
        // SAFETY: live array.
        let raw = unsafe { ffi::JSObjectGetPropertyAtIndex(self.ctx.raw(), self.raw_object(), index as c_uint, &mut exception) };
        if !exception.is_null() {
            return Err(self.ctx.throw_raw(exception));
        }
        // SAFETY: value from the C API.
        V::from_js(&self.ctx, unsafe { Value::from_raw(self.ctx, raw) })
    }

    pub fn set<V: IntoJs<'js>>(&self, index: usize, value: V) -> Result<()> {
        let value = value.into_js(&self.ctx)?;
        let mut exception: ffi::JSValueRef = core::ptr::null();
        // SAFETY: live array and value.
        unsafe { ffi::JSObjectSetPropertyAtIndex(self.ctx.raw(), self.raw_object(), index as c_uint, value.raw, &mut exception) };
        if !exception.is_null() {
            return Err(self.ctx.throw_raw(exception));
        }
        Ok(())
    }

    pub fn iter<V: FromJs<'js>>(&self) -> ArrayIter<'js, V> {
        ArrayIter { array: self.clone(), index: 0, len: self.len(), _marker: core::marker::PhantomData }
    }
}
newtype_common!(Array(Object), Object<'js>);

pub struct ArrayIter<'js, V> {
    array: Array<'js>,
    index: usize,
    len: usize,
    _marker: core::marker::PhantomData<V>,
}

impl<'js, V: FromJs<'js>> Iterator for ArrayIter<'js, V> {
    type Item = Result<V>;
    fn next(&mut self) -> Option<Self::Item> {
        if self.index >= self.len {
            return None;
        }
        let item = self.array.get(self.index);
        self.index += 1;
        Some(item)
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.len.saturating_sub(self.index);
        (remaining, Some(remaining))
    }
}

impl<'js, V: FromJs<'js>> DoubleEndedIterator for ArrayIter<'js, V> {
    fn next_back(&mut self) -> Option<Self::Item> {
        if self.index >= self.len {
            return None;
        }
        self.len -= 1;
        Some(self.array.get(self.len))
    }
}

impl<'js> IntoIterator for Array<'js> {
    type Item = Result<Value<'js>>;
    type IntoIter = ArrayIter<'js, Value<'js>>;
    fn into_iter(self) -> Self::IntoIter {
        let len = self.len();
        ArrayIter { array: self, index: 0, len, _marker: core::marker::PhantomData }
    }
}

impl fmt::Debug for Array<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.0, f)
    }
}

// ─── Promise ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromiseState {
    Pending,
    Resolved,
    Rejected,
}

#[repr(transparent)]
#[derive(Clone)]
pub struct Promise<'js>(pub(crate) Object<'js>);

impl<'js> Promise<'js> {
    /// A new pending promise with its `resolve` / `reject` functions.
    pub fn new(ctx: &Ctx<'js>) -> Result<(Promise<'js>, Function<'js>, Function<'js>)> {
        let mut resolve: ffi::JSObjectRef = core::ptr::null_mut();
        let mut reject: ffi::JSObjectRef = core::ptr::null_mut();
        let mut exception: ffi::JSValueRef = core::ptr::null();
        // SAFETY: valid context; out pointers are valid.
        let raw = unsafe { ffi::JSObjectMakeDeferredPromise(ctx.raw(), &mut resolve, &mut reject, &mut exception) };
        if !exception.is_null() {
            return Err(ctx.throw_raw(exception));
        }
        // SAFETY: fresh objects from the C API.
        unsafe {
            Ok((
                Promise(Object(Value::from_raw(*ctx, raw))),
                Function(Object(Value::from_raw(*ctx, resolve))),
                Function(Object(Value::from_raw(*ctx, reject))),
            ))
        }
    }

    pub fn into_value(self) -> Value<'js> {
        self.0.0
    }
    pub fn as_value(&self) -> &Value<'js> {
        &self.0.0
    }
    pub fn as_object(&self) -> &Object<'js> {
        &self.0
    }
    pub fn into_object(self) -> Object<'js> {
        self.0
    }
    pub fn into_inner(self) -> Object<'js> {
        self.0
    }
    pub fn as_inner(&self) -> &Object<'js> {
        &self.0
    }

    pub fn state(&self) -> PromiseState {
        // SAFETY: live promise.
        match unsafe { ffi::bun_embed_promise_status(self.encoded()) } {
            1 => PromiseState::Resolved,
            2 => PromiseState::Rejected,
            _ => PromiseState::Pending,
        }
    }

    /// The settled value: `None` while pending, `Some(Ok(v))` when resolved,
    /// `Some(Err(Error::Exception))` (with the reason pending on the context)
    /// when rejected.
    pub fn result<V: FromJs<'js>>(&self) -> Option<Result<V>> {
        match self.state() {
            PromiseState::Pending => None,
            PromiseState::Resolved => Some(V::from_js(&self.ctx, self.raw_result())),
            PromiseState::Rejected => {
                self.set_handled();
                Some(Err(self.ctx.throw(self.raw_result())))
            }
        }
    }

    pub(crate) fn raw_result(&self) -> Value<'js> {
        // SAFETY: live promise; the VM pointer is the runtime's.
        let raw = unsafe { ffi::bun_embed_promise_result(self.ctx.vm(), self.encoded()) };
        // SAFETY: value from the VM.
        unsafe { Value::from_raw(self.ctx, raw as ffi::JSValueRef) }
    }

    pub(crate) fn set_handled(&self) {
        // SAFETY: live promise; the VM pointer is the runtime's.
        unsafe { ffi::bun_embed_promise_set_handled(self.ctx.vm(), self.encoded()) }
    }

    /// `promise.then(on_fulfilled, on_rejected)`.
    pub fn then(&self) -> Result<Function<'js>> {
        self.0.get("then")
    }

    /// Drive the event loop until this promise settles (synchronously).
    pub fn finish<V: FromJs<'js>>(&self) -> Result<V> {
        // The host observes the outcome; keep bun from reporting a rejection
        // as unhandled in the meantime.
        self.set_handled();
        self.ctx.run_deferred();
        // SAFETY: the runtime's VM, on its thread; live promise.
        match unsafe { ffi::bun_embed_vm_wait_for_promise(self.ctx.vm(), self.encoded()) } {
            0 => {}
            1 => return Err(Error::Stopped),
            _ => return Err(Error::new_from_js("value", "promise")),
        }
        self.result().unwrap_or(Err(Error::Stopped))
    }

    /// Await this promise from Rust; the returned future drives Bun's event
    /// loop while polled from [`AsyncContext::async_with`](crate::AsyncContext::async_with).
    pub fn into_future<V: FromJs<'js>>(self) -> crate::async_rt::PromiseFuture<'js, V> {
        self.set_handled();
        crate::async_rt::PromiseFuture::new(self)
    }
}
newtype_common!(Promise(Object), Object<'js>);

impl fmt::Debug for Promise<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[promise {:?}]", self.state())
    }
}

// ─── Conversions ─────────────────────────────────────────────────────────

pub trait IntoJs<'js> {
    fn into_js(self, ctx: &Ctx<'js>) -> Result<Value<'js>>;
}

pub trait FromJs<'js>: Sized {
    fn from_js(ctx: &Ctx<'js>, value: Value<'js>) -> Result<Self>;
}

/// Arguments for [`Function::call`]: tuples of [`IntoArg`](crate::function::IntoArg)
/// values, optionally starting with [`This`](crate::function::This).
pub trait IntoArgs<'js> {
    fn into_args(self, ctx: &Ctx<'js>) -> Result<crate::function::Args<'js>>;

    /// Call `function` with these arguments.
    fn apply<R: FromJs<'js>>(self, function: &Function<'js>) -> Result<R>
    where
        Self: Sized,
    {
        let args = self.into_args(function.ctx())?;
        function.call_arg(args)
    }
}

macro_rules! into_js_self {
    ($($t:ident),*) => {$(
        impl<'js> IntoJs<'js> for $t<'js> {
            fn into_js(self, _ctx: &Ctx<'js>) -> Result<Value<'js>> {
                Ok(self.into_value())
            }
        }
        impl<'js> IntoJs<'js> for &$t<'js> {
            fn into_js(self, _ctx: &Ctx<'js>) -> Result<Value<'js>> {
                Ok(self.clone().into_value())
            }
        }
    )*};
}
into_js_self!(Object, Function, Constructor, Array, Promise, String, Symbol, BigInt, Exception);

impl<'js> IntoJs<'js> for Value<'js> {
    fn into_js(self, _ctx: &Ctx<'js>) -> Result<Value<'js>> {
        Ok(self)
    }
}
impl<'js> IntoJs<'js> for &Value<'js> {
    fn into_js(self, _ctx: &Ctx<'js>) -> Result<Value<'js>> {
        Ok(self.clone())
    }
}
impl<'js> IntoJs<'js> for () {
    fn into_js(self, ctx: &Ctx<'js>) -> Result<Value<'js>> {
        Ok(Value::new_undefined(*ctx))
    }
}
impl<'js> IntoJs<'js> for bool {
    fn into_js(self, ctx: &Ctx<'js>) -> Result<Value<'js>> {
        Ok(Value::new_bool(*ctx, self))
    }
}
macro_rules! into_js_number {
    ($($t:ty),*) => {$(
        impl<'js> IntoJs<'js> for $t {
            fn into_js(self, ctx: &Ctx<'js>) -> Result<Value<'js>> {
                Ok(Value::new_number(*ctx, self as f64))
            }
        }
    )*};
}
into_js_number!(f64, f32, i8, i16, i32, u8, u16, u32);
macro_rules! into_js_big_number {
    ($($t:ty),*) => {$(
        impl<'js> IntoJs<'js> for $t {
            fn into_js(self, ctx: &Ctx<'js>) -> Result<Value<'js>> {
                // Numbers up to 2^53 stay plain numbers (like rquickjs);
                // larger magnitudes become BigInt to avoid losing precision.
                const MAX_SAFE: i128 = 1 << 53;
                let wide = self as i128;
                if wide.abs() <= MAX_SAFE {
                    Ok(Value::new_number(*ctx, self as f64))
                } else if wide >= 0 {
                    BigInt::from_u64(*ctx, self as u64).map(BigInt::into_value)
                } else {
                    BigInt::from_i64(*ctx, self as i64).map(BigInt::into_value)
                }
            }
        }
    )*};
}
into_js_big_number!(i64, u64, usize, isize);
impl<'js> IntoJs<'js> for &str {
    fn into_js(self, ctx: &Ctx<'js>) -> Result<Value<'js>> {
        Ok(ctx.string(self))
    }
}
impl<'js> IntoJs<'js> for std::string::String {
    fn into_js(self, ctx: &Ctx<'js>) -> Result<Value<'js>> {
        Ok(ctx.string(&self))
    }
}
impl<'js> IntoJs<'js> for &std::string::String {
    fn into_js(self, ctx: &Ctx<'js>) -> Result<Value<'js>> {
        Ok(ctx.string(self))
    }
}
impl<'js> IntoJs<'js> for std::borrow::Cow<'_, str> {
    fn into_js(self, ctx: &Ctx<'js>) -> Result<Value<'js>> {
        Ok(ctx.string(&self))
    }
}
impl<'js> IntoJs<'js> for &std::ffi::CStr {
    fn into_js(self, ctx: &Ctx<'js>) -> Result<Value<'js>> {
        Ok(ctx.string(&self.to_string_lossy()))
    }
}
impl<'js> IntoJs<'js> for std::ffi::CString {
    fn into_js(self, ctx: &Ctx<'js>) -> Result<Value<'js>> {
        Ok(ctx.string(&self.to_string_lossy()))
    }
}
impl<'js> IntoJs<'js> for char {
    fn into_js(self, ctx: &Ctx<'js>) -> Result<Value<'js>> {
        Ok(ctx.string(&self.to_string()))
    }
}
impl<'js, T: IntoJs<'js>> IntoJs<'js> for Result<T> {
    fn into_js(self, ctx: &Ctx<'js>) -> Result<Value<'js>> {
        self.and_then(|value| value.into_js(ctx))
    }
}
impl<'js, T: IntoJs<'js>> IntoJs<'js> for Option<T> {
    fn into_js(self, ctx: &Ctx<'js>) -> Result<Value<'js>> {
        match self {
            Some(v) => v.into_js(ctx),
            None => Ok(Value::new_undefined(*ctx)),
        }
    }
}

impl<'js> FromJs<'js> for Value<'js> {
    fn from_js(_ctx: &Ctx<'js>, value: Value<'js>) -> Result<Self> {
        Ok(value)
    }
}
macro_rules! from_js_newtype {
    ($($t:ident => $try:ident, $name:literal);* $(;)?) => {$(
        impl<'js> FromJs<'js> for $t<'js> {
            fn from_js(_ctx: &Ctx<'js>, value: Value<'js>) -> Result<Self> {
                let type_name = value.type_name();
                value.$try().map_err(|_| Error::new_from_js(type_name, $name))
            }
        }
    )*};
}
from_js_newtype! {
    Object => try_into_object, "object";
    Function => try_into_function, "function";
    Constructor => try_into_constructor, "constructor";
    Array => try_into_array, "array";
    Promise => try_into_promise, "promise";
    String => try_into_string, "string";
    Symbol => try_into_symbol, "symbol";
    BigInt => try_into_big_int, "big_int";
    Exception => try_into_exception, "exception";
}
impl<'js> FromJs<'js> for () {
    fn from_js(_ctx: &Ctx<'js>, _value: Value<'js>) -> Result<Self> {
        Ok(())
    }
}
impl<'js> FromJs<'js> for bool {
    fn from_js(_ctx: &Ctx<'js>, value: Value<'js>) -> Result<Self> {
        let type_name = value.type_name();
        value.as_bool().ok_or_else(|| Error::new_from_js(type_name, "bool"))
    }
}
impl<'js> FromJs<'js> for f64 {
    fn from_js(ctx: &Ctx<'js>, value: Value<'js>) -> Result<Self> {
        let type_name = value.type_name();
        if value.is_number() {
            return value.as_number().ok_or_else(|| Error::new_from_js(type_name, "f64"));
        }
        if value.is_big_int() {
            let mut exception: ffi::JSValueRef = core::ptr::null();
            // SAFETY: live value.
            let n = unsafe { ffi::JSValueToNumber(ctx.raw(), value.raw, &mut exception) };
            if exception.is_null() {
                return Ok(n);
            }
            let _ = ctx.throw_raw(exception);
            let _ = ctx.catch();
        }
        Err(Error::new_from_js(type_name, "f64"))
    }
}
impl<'js> FromJs<'js> for f32 {
    fn from_js(ctx: &Ctx<'js>, value: Value<'js>) -> Result<Self> {
        Ok(f64::from_js(ctx, value)? as f32)
    }
}
macro_rules! from_js_int {
    ($($t:ty),*) => {$(
        impl<'js> FromJs<'js> for $t {
            fn from_js(ctx: &Ctx<'js>, value: Value<'js>) -> Result<Self> {
                let type_name = value.type_name();
                if value.is_big_int() {
                    let big = BigInt(value);
                    return <$t>::try_from(big.to_i64()?).map_err(|_| Error::new_from_js("big_int", stringify!($t)));
                }
                if !value.is_number() {
                    return Err(Error::new_from_js(type_name, stringify!($t)));
                }
                let n = f64::from_js(ctx, value)?;
                if n.is_nan() || n.fract() != 0.0 || n < <$t>::MIN as f64 || n > <$t>::MAX as f64 {
                    return Err(Error::new_from_js(type_name, stringify!($t)));
                }
                Ok(n as $t)
            }
        }
    )*};
}
from_js_int!(i8, i16, i32, i64, u8, u16, u32, u64, usize, isize);
impl<'js> FromJs<'js> for std::string::String {
    fn from_js(_ctx: &Ctx<'js>, value: Value<'js>) -> Result<Self> {
        let type_name = value.type_name();
        if !value.is_string() {
            return Err(Error::new_from_js(type_name, "string"));
        }
        Ok(value.to_string_lossy())
    }
}
impl<'js> FromJs<'js> for char {
    fn from_js(ctx: &Ctx<'js>, value: Value<'js>) -> Result<Self> {
        let string = std::string::String::from_js(ctx, value)?;
        let mut chars = string.chars();
        match (chars.next(), chars.next()) {
            (Some(c), None) => Ok(c),
            _ => Err(Error::new_from_js_message("string", "char", "string was not a single character")),
        }
    }
}
impl<'js, T: FromJs<'js>> FromJs<'js> for Result<T> {
    fn from_js(ctx: &Ctx<'js>, value: Value<'js>) -> Result<Self> {
        Ok(T::from_js(ctx, value))
    }
}
impl<'js, T: FromJs<'js>> FromJs<'js> for Option<T> {
    fn from_js(ctx: &Ctx<'js>, value: Value<'js>) -> Result<Self> {
        if value.is_undefined_or_null() { Ok(None) } else { T::from_js(ctx, value).map(Some) }
    }
}

//! Extra conversions, mirroring `rquickjs::convert`: [`Undefined`], [`Null`],
//! [`Coerced`], dates, collections, tuples and [`IteratorJs::collect_js`].

use core::ops::Deref;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, LinkedList, VecDeque};
use std::hash::{BuildHasher, Hash};
use std::rc::Rc;
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, SystemTime};

use crate::atom::{FromAtom, IntoAtom};
use crate::error::{Error, Result};
use crate::ffi;
use crate::runtime::Ctx;
use crate::value::{Array, FromJs, IntoJs, Object, Value};

/// The `undefined` value.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Undefined;

impl<'js> IntoJs<'js> for Undefined {
    fn into_js(self, ctx: &Ctx<'js>) -> Result<Value<'js>> {
        Ok(Value::new_undefined(*ctx))
    }
}
impl<'js> FromJs<'js> for Undefined {
    fn from_js(_ctx: &Ctx<'js>, value: Value<'js>) -> Result<Self> {
        if value.is_undefined() { Ok(Undefined) } else { Err(Error::new_from_js(value.type_name(), "undefined")) }
    }
}

/// The `null` value.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Null;

impl<'js> IntoJs<'js> for Null {
    fn into_js(self, ctx: &Ctx<'js>) -> Result<Value<'js>> {
        Ok(Value::new_null(*ctx))
    }
}
impl<'js> FromJs<'js> for Null {
    fn from_js(_ctx: &Ctx<'js>, value: Value<'js>) -> Result<Self> {
        if value.is_null() { Ok(Null) } else { Err(Error::new_from_js(value.type_name(), "null")) }
    }
}

/// Convert with JavaScript coercion (`String(v)`, `Number(v)`, `Boolean(v)`)
/// instead of strict type matching.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Coerced<T>(pub T);

impl<T> Deref for Coerced<T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.0
    }
}
impl<T> AsRef<T> for Coerced<T> {
    fn as_ref(&self) -> &T {
        &self.0
    }
}
impl<T> From<T> for Coerced<T> {
    fn from(value: T) -> Self {
        Coerced(value)
    }
}

impl<'js> FromJs<'js> for Coerced<std::string::String> {
    fn from_js(_ctx: &Ctx<'js>, value: Value<'js>) -> Result<Self> {
        Ok(Coerced(value.to_string_lossy()))
    }
}
impl<'js> FromJs<'js> for Coerced<crate::String<'js>> {
    fn from_js(ctx: &Ctx<'js>, value: Value<'js>) -> Result<Self> {
        Ok(Coerced(crate::String::from_str(*ctx, &value.to_string_lossy())?))
    }
}
impl<'js> FromJs<'js> for Coerced<bool> {
    fn from_js(_ctx: &Ctx<'js>, value: Value<'js>) -> Result<Self> {
        // SAFETY: live value.
        Ok(Coerced(unsafe { ffi::JSValueToBoolean(value.ctx().raw(), value.as_raw()) }))
    }
}
impl<'js> FromJs<'js> for Coerced<f64> {
    fn from_js(ctx: &Ctx<'js>, value: Value<'js>) -> Result<Self> {
        let mut exception: ffi::JSValueRef = core::ptr::null();
        // SAFETY: live value.
        let n = unsafe { ffi::JSValueToNumber(ctx.raw(), value.as_raw(), &mut exception) };
        if !exception.is_null() {
            return Err(ctx.throw_raw(exception));
        }
        Ok(Coerced(n))
    }
}
impl<'js> FromJs<'js> for Coerced<i32> {
    fn from_js(ctx: &Ctx<'js>, value: Value<'js>) -> Result<Self> {
        let mut exception: ffi::JSValueRef = core::ptr::null();
        // SAFETY: live value.
        let n = unsafe { ffi::JSValueToInt32(ctx.raw(), value.as_raw(), &mut exception) };
        if !exception.is_null() {
            return Err(ctx.throw_raw(exception));
        }
        Ok(Coerced(n))
    }
}
impl<'js> FromJs<'js> for Coerced<i64> {
    fn from_js(ctx: &Ctx<'js>, value: Value<'js>) -> Result<Self> {
        let mut exception: ffi::JSValueRef = core::ptr::null();
        // SAFETY: live value.
        let n = unsafe { ffi::JSValueToInt64(ctx.raw(), value.as_raw(), &mut exception) };
        if !exception.is_null() {
            return Err(ctx.throw_raw(exception));
        }
        Ok(Coerced(n))
    }
}
impl<'js> FromJs<'js> for Coerced<u64> {
    fn from_js(ctx: &Ctx<'js>, value: Value<'js>) -> Result<Self> {
        let mut exception: ffi::JSValueRef = core::ptr::null();
        // SAFETY: live value.
        let n = unsafe { ffi::JSValueToUInt64(ctx.raw(), value.as_raw(), &mut exception) };
        if !exception.is_null() {
            return Err(ctx.throw_raw(exception));
        }
        Ok(Coerced(n))
    }
}
impl<'js, T> IntoJs<'js> for Coerced<T>
where
    T: IntoJs<'js>,
{
    fn into_js(self, ctx: &Ctx<'js>) -> Result<Value<'js>> {
        self.0.into_js(ctx)
    }
}

// ─── Dates ───────────────────────────────────────────────────────────────

impl<'js> IntoJs<'js> for SystemTime {
    fn into_js(self, ctx: &Ctx<'js>) -> Result<Value<'js>> {
        let millis: f64 = match self.duration_since(SystemTime::UNIX_EPOCH) {
            Ok(d) => d.as_millis() as f64,
            Err(e) => -(e.duration().as_millis() as f64),
        };
        let arg = Value::new_number(*ctx, millis);
        let args = [arg.as_raw()];
        let mut exception: ffi::JSValueRef = core::ptr::null();
        // SAFETY: valid context; args live.
        let raw = unsafe { ffi::JSObjectMakeDate(ctx.raw(), 1, args.as_ptr(), &mut exception) };
        if !exception.is_null() {
            return Err(ctx.throw_raw(exception));
        }
        // SAFETY: fresh object.
        Ok(unsafe { Value::from_raw(*ctx, raw as ffi::JSValueRef) })
    }
}

impl<'js> FromJs<'js> for SystemTime {
    fn from_js(ctx: &Ctx<'js>, value: Value<'js>) -> Result<Self> {
        if !value.is_date() {
            return Err(Error::new_from_js(value.type_name(), "SystemTime"));
        }
        let millis: f64 = Coerced::<f64>::from_js(ctx, value)?.0;
        if millis.is_nan() {
            return Err(Error::new_from_js_message("Date", "SystemTime", "Invalid Date"));
        }
        if millis >= 0.0 {
            Ok(SystemTime::UNIX_EPOCH + Duration::from_millis(millis as u64))
        } else {
            SystemTime::UNIX_EPOCH
                .checked_sub(Duration::from_millis((-millis) as u64))
                .ok_or_else(|| Error::new_from_js_message("Date", "SystemTime", "Timestamp too small"))
        }
    }
}

// ─── Smart pointers ──────────────────────────────────────────────────────

impl<'js, T: IntoJs<'js>> IntoJs<'js> for Box<T> {
    fn into_js(self, ctx: &Ctx<'js>) -> Result<Value<'js>> {
        (*self).into_js(ctx)
    }
}
impl<'js, T: FromJs<'js>> FromJs<'js> for Box<T> {
    fn from_js(ctx: &Ctx<'js>, value: Value<'js>) -> Result<Self> {
        T::from_js(ctx, value).map(Box::new)
    }
}
impl<'js, T: IntoJs<'js> + Clone> IntoJs<'js> for Rc<T> {
    fn into_js(self, ctx: &Ctx<'js>) -> Result<Value<'js>> {
        (*self).clone().into_js(ctx)
    }
}
impl<'js, T: FromJs<'js>> FromJs<'js> for Rc<T> {
    fn from_js(ctx: &Ctx<'js>, value: Value<'js>) -> Result<Self> {
        T::from_js(ctx, value).map(Rc::new)
    }
}
impl<'js, T: IntoJs<'js> + Clone> IntoJs<'js> for Arc<T> {
    fn into_js(self, ctx: &Ctx<'js>) -> Result<Value<'js>> {
        (*self).clone().into_js(ctx)
    }
}
impl<'js, T: FromJs<'js>> FromJs<'js> for Arc<T> {
    fn from_js(ctx: &Ctx<'js>, value: Value<'js>) -> Result<Self> {
        T::from_js(ctx, value).map(Arc::new)
    }
}
impl<'js, T: FromJs<'js>> FromJs<'js> for Mutex<T> {
    fn from_js(ctx: &Ctx<'js>, value: Value<'js>) -> Result<Self> {
        T::from_js(ctx, value).map(Mutex::new)
    }
}
impl<'js, T: FromJs<'js>> FromJs<'js> for RwLock<T> {
    fn from_js(ctx: &Ctx<'js>, value: Value<'js>) -> Result<Self> {
        T::from_js(ctx, value).map(RwLock::new)
    }
}
impl<'js, T: IntoJs<'js> + Clone> IntoJs<'js> for &[T] {
    fn into_js(self, ctx: &Ctx<'js>) -> Result<Value<'js>> {
        self.iter().cloned().collect_js::<Array<'js>>(ctx).map(Array::into_value)
    }
}
impl<'js, T: IntoJs<'js>, const N: usize> IntoJs<'js> for [T; N] {
    fn into_js(self, ctx: &Ctx<'js>) -> Result<Value<'js>> {
        self.into_iter().collect_js::<Array<'js>>(ctx).map(Array::into_value)
    }
}

// ─── Collections ─────────────────────────────────────────────────────────

macro_rules! into_js_seq {
    ($($t:ident),*) => {$(
        impl<'js, T: IntoJs<'js>> IntoJs<'js> for $t<T> {
            fn into_js(self, ctx: &Ctx<'js>) -> Result<Value<'js>> {
                self.into_iter().collect_js::<Array<'js>>(ctx).map(Array::into_value)
            }
        }
        impl<'js, T: FromJs<'js>> FromJs<'js> for $t<T> {
            fn from_js(ctx: &Ctx<'js>, value: Value<'js>) -> Result<Self> {
                Array::from_js(ctx, value)?.iter().collect()
            }
        }
    )*};
}
into_js_seq!(Vec, VecDeque, LinkedList);

impl<'js, T: IntoJs<'js>, S> IntoJs<'js> for HashSet<T, S> {
    fn into_js(self, ctx: &Ctx<'js>) -> Result<Value<'js>> {
        self.into_iter().collect_js::<Array<'js>>(ctx).map(Array::into_value)
    }
}
impl<'js, T: FromJs<'js> + Eq + Hash, S: BuildHasher + Default> FromJs<'js> for HashSet<T, S> {
    fn from_js(ctx: &Ctx<'js>, value: Value<'js>) -> Result<Self> {
        Array::from_js(ctx, value)?.iter().collect()
    }
}
impl<'js, T: IntoJs<'js>> IntoJs<'js> for BTreeSet<T> {
    fn into_js(self, ctx: &Ctx<'js>) -> Result<Value<'js>> {
        self.into_iter().collect_js::<Array<'js>>(ctx).map(Array::into_value)
    }
}
impl<'js, T: FromJs<'js> + Ord> FromJs<'js> for BTreeSet<T> {
    fn from_js(ctx: &Ctx<'js>, value: Value<'js>) -> Result<Self> {
        Array::from_js(ctx, value)?.iter().collect()
    }
}
impl<'js, K: IntoAtom<'js>, V: IntoJs<'js>, S> IntoJs<'js> for HashMap<K, V, S> {
    fn into_js(self, ctx: &Ctx<'js>) -> Result<Value<'js>> {
        self.into_iter().collect_js::<Object<'js>>(ctx).map(Object::into_value)
    }
}
impl<'js, K: FromAtom<'js> + Eq + Hash, V: FromJs<'js>, S: BuildHasher + Default> FromJs<'js> for HashMap<K, V, S> {
    fn from_js(ctx: &Ctx<'js>, value: Value<'js>) -> Result<Self> {
        Object::from_js(ctx, value)?.props::<K, V>().collect()
    }
}
impl<'js, K: IntoAtom<'js>, V: IntoJs<'js>> IntoJs<'js> for BTreeMap<K, V> {
    fn into_js(self, ctx: &Ctx<'js>) -> Result<Value<'js>> {
        self.into_iter().collect_js::<Object<'js>>(ctx).map(Object::into_value)
    }
}
impl<'js, K: FromAtom<'js> + Ord, V: FromJs<'js>> FromJs<'js> for BTreeMap<K, V> {
    fn from_js(ctx: &Ctx<'js>, value: Value<'js>) -> Result<Self> {
        Object::from_js(ctx, value)?.props::<K, V>().collect()
    }
}

// ─── Tuples ──────────────────────────────────────────────────────────────

macro_rules! tuple_conversions {
    ($($t:ident $i:tt),+) => {
        impl<'js, $($t: IntoJs<'js>),+> IntoJs<'js> for ($($t,)+) {
            fn into_js(self, ctx: &Ctx<'js>) -> Result<Value<'js>> {
                let array = Array::new(*ctx)?;
                $(array.set($i, self.$i)?;)+
                Ok(array.into_value())
            }
        }
        impl<'js, $($t: FromJs<'js>),+> FromJs<'js> for ($($t,)+) {
            fn from_js(ctx: &Ctx<'js>, value: Value<'js>) -> Result<Self> {
                let array = Array::from_js(ctx, value)?;
                Ok(($(array.get::<$t>($i)?,)+))
            }
        }
    };
}
tuple_conversions!(A 0);
tuple_conversions!(A 0, B 1);
tuple_conversions!(A 0, B 1, C 2);
tuple_conversions!(A 0, B 1, C 2, D 3);
tuple_conversions!(A 0, B 1, C 2, D 3, E 4);
tuple_conversions!(A 0, B 1, C 2, D 3, E 4, F 5);
tuple_conversions!(A 0, B 1, C 2, D 3, E 4, F 5, G 6);
tuple_conversions!(A 0, B 1, C 2, D 3, E 4, F 5, G 6, H 7);

// ─── collect_js ──────────────────────────────────────────────────────────

/// Build a JavaScript value from an iterator (`Array` from items, `Object`
/// from `(key, value)` pairs).
pub trait FromIteratorJs<'js, A>: Sized {
    type Item;
    fn from_iter_js<T>(ctx: &Ctx<'js>, iter: T) -> Result<Self>
    where
        T: IntoIterator<Item = A>;
}

impl<'js, A: IntoJs<'js>> FromIteratorJs<'js, A> for Array<'js> {
    type Item = Value<'js>;
    fn from_iter_js<T>(ctx: &Ctx<'js>, iter: T) -> Result<Self>
    where
        T: IntoIterator<Item = A>,
    {
        let array = Array::new(*ctx)?;
        for (i, item) in iter.into_iter().enumerate() {
            array.set(i, item)?;
        }
        Ok(array)
    }
}

impl<'js, K: IntoAtom<'js>, V: IntoJs<'js>> FromIteratorJs<'js, (K, V)> for Object<'js> {
    type Item = (crate::atom::Atom<'js>, Value<'js>);
    fn from_iter_js<T>(ctx: &Ctx<'js>, iter: T) -> Result<Self>
    where
        T: IntoIterator<Item = (K, V)>,
    {
        let object = Object::new(*ctx)?;
        for (key, value) in iter {
            object.set(key, value)?;
        }
        Ok(object)
    }
}

impl<'js, A: IntoJs<'js>> FromIteratorJs<'js, A> for Value<'js> {
    type Item = Value<'js>;
    fn from_iter_js<T>(ctx: &Ctx<'js>, iter: T) -> Result<Self>
    where
        T: IntoIterator<Item = A>,
    {
        Array::from_iter_js(ctx, iter).map(Array::into_value)
    }
}

/// `iter.collect_js::<Array>(&ctx)` / `collect_js::<Object>(&ctx)`.
pub trait IteratorJs<'js, A>: Iterator<Item = A> + Sized {
    fn collect_js<B: FromIteratorJs<'js, A>>(self, ctx: &Ctx<'js>) -> Result<B> {
        B::from_iter_js(ctx, self)
    }
}

impl<'js, A, T: Iterator<Item = A>> IteratorJs<'js, A> for T {}

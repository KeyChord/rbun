//! Lifetime-erased handles, mirroring `rquickjs::Persistent`: store values
//! outside of a `Ctx` borrow (thread-locals, registries) and restore them
//! later on the runtime's thread.

use crate::error::{Error, Result};
use crate::ffi;
use crate::runtime::Ctx;
use crate::value::{Array, Constructor, FromJs, Function, Object, Promise, String as JsString, Symbol, Value};

pub struct Persistent<T> {
    raw: ffi::JSValueRef,
    ctx: ffi::JSContextRef,
    _marker: core::marker::PhantomData<T>,
}

impl<T> Clone for Persistent<T> {
    fn clone(&self) -> Self {
        // SAFETY: protections are counted; the value is live (immediates are
        // protected too — harmless and keeps this independent of the encoding).
        unsafe { ffi::JSValueProtect(self.ctx, self.raw) };
        Persistent { raw: self.raw, ctx: self.ctx, _marker: core::marker::PhantomData }
    }
}

impl<T> Drop for Persistent<T> {
    fn drop(&mut self) {
        // SAFETY: balances the protection taken at creation / clone.
        unsafe { ffi::JSValueUnprotect(self.ctx, self.raw) };
    }
}

impl<T> core::fmt::Debug for Persistent<T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "Persistent({:p})", self.raw)
    }
}

/// Types that can be saved into a [`Persistent`].
pub trait Outlive<'js> {
    type Target<'to>;
    fn as_raw_value(&self) -> &Value<'js>;
    fn from_value<'to>(value: Value<'to>) -> Result<Self::Target<'to>>;
}

macro_rules! outlive {
    ($t:ident) => {
        impl<'js> Outlive<'js> for $t<'js> {
            type Target<'to> = $t<'to>;
            fn as_raw_value(&self) -> &Value<'js> {
                self.as_ref()
            }
            fn from_value<'to>(value: Value<'to>) -> Result<Self::Target<'to>> {
                let ctx = *value.ctx();
                <$t<'to> as FromJs<'to>>::from_js(&ctx, value)
            }
        }
    };
}
outlive!(Value);
outlive!(Object);
outlive!(Function);
outlive!(Constructor);
outlive!(Array);
outlive!(Promise);
outlive!(JsString);
outlive!(Symbol);

impl<T> Persistent<T> {
    /// Save a value, releasing its `'js` lifetime.
    pub fn save<'js, V>(_ctx: &Ctx<'js>, value: V) -> Persistent<T>
    where
        V: Outlive<'js, Target<'static> = T>,
    {
        let raw_value = value.as_raw_value();
        let raw = raw_value.as_raw();
        let ctx = raw_value.ctx().raw();
        // SAFETY: live value.
        unsafe { ffi::JSValueProtect(ctx, raw) };
        Persistent { raw, ctx, _marker: core::marker::PhantomData }
    }

    /// Restore the value on the runtime it was saved from.
    pub fn restore<'js>(self, ctx: &Ctx<'js>) -> Result<<T as Outlive<'static>>::Target<'js>>
    where
        T: Outlive<'static>,
    {
        if self.ctx != ctx.raw() {
            return Err(Error::UnrelatedRuntime);
        }
        // SAFETY: the value is kept alive by this handle's protection.
        let value = unsafe { Value::from_raw(*ctx, self.raw) };
        T::from_value(value)
    }
}

impl<'js, T> crate::value::IntoJs<'js> for Persistent<T>
where
    T: Outlive<'static>,
    <T as Outlive<'static>>::Target<'js>: crate::value::IntoJs<'js>,
{
    fn into_js(self, ctx: &Ctx<'js>) -> Result<Value<'js>> {
        self.restore(ctx)?.into_js(ctx)
    }
}

impl<'js, T> FromJs<'js> for Persistent<T>
where
    T: Outlive<'static>,
    <T as Outlive<'static>>::Target<'js>: FromJs<'js> + Outlive<'js, Target<'static> = T>,
{
    fn from_js(ctx: &Ctx<'js>, value: Value<'js>) -> Result<Self> {
        let value = <T as Outlive<'static>>::Target::<'js>::from_js(ctx, value)?;
        Ok(Persistent::save(ctx, value))
    }
}

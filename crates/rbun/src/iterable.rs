//! Rust iterators as JavaScript iterables and vice versa, mirroring
//! `rquickjs::{Iterable, JsIterator}`.

use core::cell::RefCell;
use std::rc::Rc;

use crate::error::{Error, Result};
use crate::function::Function;
use crate::runtime::Ctx;
use crate::value::{FromJs, IntoJs, Object, Value};

/// Wrap a Rust iterator so JavaScript can consume it (`[...it]`, `for-of`).
/// Single-use: once exhausted it stays exhausted.
pub struct Iterable<I>(I);

impl<I: IntoIterator> From<I> for Iterable<I> {
    fn from(iter: I) -> Self {
        Iterable(iter)
    }
}

impl<'js, I> IntoJs<'js> for Iterable<I>
where
    I: IntoIterator + 'js,
    I::Item: IntoJs<'js>,
{
    fn into_js(self, ctx: &Ctx<'js>) -> Result<Value<'js>> {
        let iter = Rc::new(RefCell::new(Some(self.0.into_iter())));
        let next = Function::new(*ctx, move |cx: Ctx<'js>| -> Result<Value<'js>> {
            let result = Object::new(cx)?;
            let mut iter = iter.borrow_mut();
            match iter.as_mut().and_then(|i| i.next()) {
                Some(item) => {
                    result.set("value", item)?;
                    result.set("done", false)?;
                }
                None => {
                    *iter = None;
                    result.set("value", Value::new_undefined(cx))?;
                    result.set("done", true)?;
                }
            }
            Ok(result.into_value())
        })?
        .with_name("next")?;
        let make = ctx.function("makeIterable")?;
        make.call((next,))
    }
}

/// A JavaScript iterator (or iterable) consumed from Rust.
pub struct JsIterator<'js, T = Value<'js>> {
    iterator: Object<'js>,
    next: Function<'js>,
    done: bool,
    _marker: core::marker::PhantomData<T>,
}

impl<'js, T> JsIterator<'js, T> {
    /// Iterate with a different item conversion.
    pub fn typed<U: FromJs<'js>>(self) -> JsIterator<'js, U> {
        JsIterator { iterator: self.iterator, next: self.next, done: self.done, _marker: core::marker::PhantomData }
    }

    pub fn as_object(&self) -> &Object<'js> {
        &self.iterator
    }
}

impl<'js, T: FromJs<'js>> FromJs<'js> for JsIterator<'js, T> {
    fn from_js(ctx: &Ctx<'js>, value: Value<'js>) -> Result<Self> {
        let get_iterator = ctx.function("getIterator")?;
        let iterator: Value<'js> = get_iterator.call((value,))?;
        let iterator = iterator.into_object().ok_or_else(|| Error::new_from_js("value", "iterator"))?;
        let next: Function<'js> = iterator.get("next")?;
        Ok(JsIterator { iterator, next, done: false, _marker: core::marker::PhantomData })
    }
}

impl<'js, T: FromJs<'js>> Iterator for JsIterator<'js, T> {
    type Item = Result<T>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.done {
            return None;
        }
        let step = (|| -> Result<Option<T>> {
            let result: Object<'js> = self.next.call((crate::function::This(self.iterator.clone()),))?;
            let done: bool = result.get::<_, Value<'js>>("done")?.get::<crate::convert::Coerced<bool>>()?.0;
            if done {
                return Ok(None);
            }
            let value: Value<'js> = result.get("value")?;
            Ok(Some(T::from_js(self.iterator.ctx(), value)?))
        })();
        match step {
            Ok(Some(item)) => Some(Ok(item)),
            Ok(None) => {
                self.done = true;
                None
            }
            Err(error) => {
                self.done = true;
                Some(Err(error))
            }
        }
    }
}

//! Property descriptors and key filters, mirroring `rquickjs::object`.

use crate::atom::Atom;
use crate::error::Result;
use crate::function::{Function, IntoJsFunc};
use crate::runtime::Ctx;
use crate::value::{Array, FromJs, IntoJs, Object, Value};
use crate::atom::FromAtom;

/// Which own keys [`Object::own_keys`] returns.
#[derive(Debug, Clone, Copy)]
pub struct Filter {
    string: bool,
    symbol: bool,
    private: bool,
    enum_only: bool,
}

impl Default for Filter {
    fn default() -> Self {
        Filter { string: true, symbol: true, private: false, enum_only: false }
    }
}

impl Filter {
    /// No keys at all; combine with the setters.
    pub const fn new() -> Self {
        Filter { string: false, symbol: false, private: false, enum_only: false }
    }
    pub const fn string(mut self) -> Self {
        self.string = true;
        self
    }
    pub const fn symbol(mut self) -> Self {
        self.symbol = true;
        self
    }
    /// Accepted for compatibility; JavaScriptCore has no enumerable private
    /// keys.
    pub const fn private(mut self) -> Self {
        self.private = true;
        self
    }
    pub const fn enum_only(mut self) -> Self {
        self.enum_only = true;
        self
    }
}

pub(crate) fn own_key_values<'js>(object: &Object<'js>, filter: Filter) -> std::vec::IntoIter<Atom<'js>> {
    let ctx = *object.ctx();
    let keys = (|| -> Result<Vec<Atom<'js>>> {
        let own_keys = ctx.function("ownKeys")?;
        let array: Array<'js> = own_keys.call((object, filter.string, filter.symbol, filter.enum_only))?;
        array.iter::<Value<'js>>().map(|v| Atom::from_value(ctx, &v?)).collect()
    })()
    .unwrap_or_default();
    keys.into_iter()
}

/// Iterator over `(key, value)` pairs (see [`Object::props`]).
pub struct ObjectIter<'js, K, V> {
    pub(crate) object: Object<'js>,
    pub(crate) keys: std::vec::IntoIter<Atom<'js>>,
    pub(crate) _marker: core::marker::PhantomData<(K, V)>,
}

impl<'js, K: FromAtom<'js>, V: FromJs<'js>> Iterator for ObjectIter<'js, K, V> {
    type Item = Result<(K, V)>;
    fn next(&mut self) -> Option<Self::Item> {
        let atom = self.keys.next()?;
        Some((|| {
            let value: V = self.object.get(&atom)?;
            Ok((K::from_atom(atom)?, value))
        })())
    }
}

/// Something that can be defined as a property (see [`Object::prop`]).
pub trait AsProperty<'js> {
    fn define(self, ctx: &Ctx<'js>, object: &Object<'js>, key: &Atom<'js>) -> Result<()>;
}

/// A data property. Read-only, non-enumerable and non-configurable unless the
/// corresponding setter is called (rquickjs' defaults).
pub struct Property<T> {
    value: T,
    writable: bool,
    configurable: bool,
    enumerable: bool,
}

impl<T> From<T> for Property<T> {
    fn from(value: T) -> Self {
        Property { value, writable: false, configurable: false, enumerable: false }
    }
}

impl<T> Property<T> {
    pub fn writable(mut self) -> Self {
        self.writable = true;
        self
    }
    pub fn configurable(mut self) -> Self {
        self.configurable = true;
        self
    }
    pub fn enumerable(mut self) -> Self {
        self.enumerable = true;
        self
    }
}

impl<'js, T: IntoJs<'js>> AsProperty<'js> for Property<T> {
    fn define(self, ctx: &Ctx<'js>, object: &Object<'js>, key: &Atom<'js>) -> Result<()> {
        let value = self.value.into_js(ctx)?;
        let define = ctx.function("defineProperty")?;
        define.call::<_, ()>((object, key, value, self.writable, self.enumerable, self.configurable))
    }
}

/// Any plain value defines a read-only, non-enumerable, non-configurable
/// property (like rquickjs).
macro_rules! as_property_value {
    ($($t:ty),*) => {$(
        impl<'js> AsProperty<'js> for $t {
            fn define(self, ctx: &Ctx<'js>, object: &Object<'js>, key: &Atom<'js>) -> Result<()> {
                Property::from(self).define(ctx, object, key)
            }
        }
    )*};
}
as_property_value!((), bool, i8, i16, i32, i64, u8, u16, u32, u64, usize, isize, f32, f64, &str, std::string::String, char);
impl<'js> AsProperty<'js> for Value<'js> {
    fn define(self, ctx: &Ctx<'js>, object: &Object<'js>, key: &Atom<'js>) -> Result<()> {
        Property::from(self).define(ctx, object, key)
    }
}
impl<'js> AsProperty<'js> for Object<'js> {
    fn define(self, ctx: &Ctx<'js>, object: &Object<'js>, key: &Atom<'js>) -> Result<()> {
        Property::from(self).define(ctx, object, key)
    }
}
impl<'js> AsProperty<'js> for Function<'js> {
    fn define(self, ctx: &Ctx<'js>, object: &Object<'js>, key: &Atom<'js>) -> Result<()> {
        Property::from(self).define(ctx, object, key)
    }
}

/// Placeholder for an accessor without a setter.
pub struct NoSetter;

/// Either a function convertible with [`IntoJsFunc`] or [`NoSetter`].
pub trait MaybeFunc<'js, P> {
    fn into_function(self, ctx: &Ctx<'js>) -> Result<Option<Function<'js>>>;
}
impl<'js> MaybeFunc<'js, ()> for NoSetter {
    fn into_function(self, _ctx: &Ctx<'js>) -> Result<Option<Function<'js>>> {
        Ok(None)
    }
}
impl<'js, F: IntoJsFunc<'js, P> + 'js, P> MaybeFunc<'js, P> for F {
    fn into_function(self, ctx: &Ctx<'js>) -> Result<Option<Function<'js>>> {
        Ok(Some(Function::new(*ctx, self)?))
    }
}

/// An accessor property with a getter and an optional setter.
pub struct Accessor<G, S = NoSetter, PG = (), PS = ()> {
    getter: G,
    setter: S,
    configurable: bool,
    enumerable: bool,
    _marker: core::marker::PhantomData<(PG, PS)>,
}

impl<G, PG> From<G> for Accessor<G, NoSetter, PG, ()> {
    fn from(getter: G) -> Self {
        Accessor { getter, setter: NoSetter, configurable: false, enumerable: false, _marker: core::marker::PhantomData }
    }
}

impl<G, PG> Accessor<G, NoSetter, PG, ()> {
    pub fn new(getter: G) -> Self {
        Accessor::from(getter)
    }

    pub fn set<S, PS>(self, setter: S) -> Accessor<G, S, PG, PS> {
        Accessor { getter: self.getter, setter, configurable: self.configurable, enumerable: self.enumerable, _marker: core::marker::PhantomData }
    }
}

impl<S, PS> Accessor<NoSetter, S, (), PS> {
    pub fn new_set(setter: S) -> Self {
        Accessor { getter: NoSetter, setter, configurable: false, enumerable: false, _marker: core::marker::PhantomData }
    }
}

impl<G, S, PG, PS> Accessor<G, S, PG, PS> {
    pub fn configurable(mut self) -> Self {
        self.configurable = true;
        self
    }
    pub fn enumerable(mut self) -> Self {
        self.enumerable = true;
        self
    }
}

impl<'js, G, S, PG, PS> AsProperty<'js> for Accessor<G, S, PG, PS>
where
    G: MaybeFunc<'js, PG>,
    S: MaybeFunc<'js, PS>,
{
    fn define(self, ctx: &Ctx<'js>, object: &Object<'js>, key: &Atom<'js>) -> Result<()> {
        let getter = match self.getter.into_function(ctx)? {
            Some(getter) => getter.into_value(),
            None => Value::new_undefined(*ctx),
        };
        let setter = match self.setter.into_function(ctx)? {
            Some(setter) => setter.into_value(),
            None => Value::new_undefined(*ctx),
        };
        let define = ctx.function("defineAccessor")?;
        define.call::<_, ()>((object, key, getter, setter, self.enumerable, self.configurable))
    }
}

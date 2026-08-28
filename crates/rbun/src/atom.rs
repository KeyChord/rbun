//! Property keys, mirroring `rquickjs::Atom`. A JavaScript property key is
//! a string, a symbol, or (for arrays) an index; an [`Atom`] holds any of
//! these as a value and is what [`Object::get`](crate::Object::get) & co.
//! accept through [`IntoAtom`].

use core::fmt;

use crate::error::{Error, Result};
use crate::runtime::Ctx;
use crate::value::{FromJs, IntoJs, String as JsString, Symbol, Value};

#[derive(Clone)]
pub struct Atom<'js>(pub(crate) Value<'js>);

impl<'js> Atom<'js> {
    pub fn from_str(ctx: Ctx<'js>, name: &str) -> Result<Self> {
        Ok(Atom(ctx.intern(name)))
    }

    /// Any string, symbol or number value.
    pub fn from_value(ctx: Ctx<'js>, value: &Value<'js>) -> Result<Self> {
        if value.is_string() || value.is_symbol() || value.is_number() {
            return Ok(Atom(value.clone()));
        }
        // Objects and the rest coerce through `String(value)`, like
        // `obj[value]` would.
        let _ = ctx;
        Ok(Atom(value.ctx().string(&value.to_string_lossy())))
    }

    pub fn from_u32(ctx: Ctx<'js>, index: u32) -> Result<Self> {
        Ok(Atom(Value::new_number(ctx, index as f64)))
    }

    pub fn from_predefined(ctx: Ctx<'js>, predefined: PredefinedAtom) -> Self {
        Atom(ctx.string(predefined.as_str()))
    }

    pub fn ctx(&self) -> &Ctx<'js> {
        self.0.ctx()
    }

    pub fn is_symbol(&self) -> bool {
        self.0.is_symbol()
    }

    /// The key as a Rust string (`String(key)`).
    #[allow(clippy::inherent_to_string)]
    pub fn to_string(&self) -> Result<std::string::String> {
        if self.0.is_symbol() {
            return Ok(self.0.to_string_lossy());
        }
        Ok(self.0.to_string_lossy())
    }

    pub fn to_js_string(&self) -> Result<JsString<'js>> {
        JsString::from_str(*self.0.ctx(), &self.to_string()?)
    }

    pub fn to_value(&self) -> Result<Value<'js>> {
        Ok(self.0.clone())
    }

    pub fn as_value(&self) -> &Value<'js> {
        &self.0
    }

    pub fn into_value(self) -> Value<'js> {
        self.0
    }
}

impl fmt::Debug for Atom<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Atom({:?})", self.0)
    }
}

impl PartialEq for Atom<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl<'js> IntoJs<'js> for Atom<'js> {
    fn into_js(self, _ctx: &Ctx<'js>) -> Result<Value<'js>> {
        Ok(self.0)
    }
}

impl<'js> IntoJs<'js> for &Atom<'js> {
    fn into_js(self, _ctx: &Ctx<'js>) -> Result<Value<'js>> {
        Ok(self.0.clone())
    }
}

impl<'js> FromJs<'js> for Atom<'js> {
    fn from_js(ctx: &Ctx<'js>, value: Value<'js>) -> Result<Self> {
        Atom::from_value(*ctx, &value)
    }
}

/// Types usable as property keys.
pub trait IntoAtom<'js> {
    fn into_atom(self, ctx: &Ctx<'js>) -> Result<Atom<'js>>;
}

impl<'js> IntoAtom<'js> for Atom<'js> {
    fn into_atom(self, _ctx: &Ctx<'js>) -> Result<Atom<'js>> {
        Ok(self)
    }
}
impl<'js> IntoAtom<'js> for &Atom<'js> {
    fn into_atom(self, _ctx: &Ctx<'js>) -> Result<Atom<'js>> {
        Ok(self.clone())
    }
}
impl<'js> IntoAtom<'js> for Value<'js> {
    fn into_atom(self, ctx: &Ctx<'js>) -> Result<Atom<'js>> {
        Atom::from_value(*ctx, &self)
    }
}
impl<'js> IntoAtom<'js> for &Value<'js> {
    fn into_atom(self, ctx: &Ctx<'js>) -> Result<Atom<'js>> {
        Atom::from_value(*ctx, self)
    }
}
impl<'js> IntoAtom<'js> for Symbol<'js> {
    fn into_atom(self, _ctx: &Ctx<'js>) -> Result<Atom<'js>> {
        Ok(Atom(self.into_value()))
    }
}
impl<'js> IntoAtom<'js> for &Symbol<'js> {
    fn into_atom(self, _ctx: &Ctx<'js>) -> Result<Atom<'js>> {
        Ok(Atom(self.as_value().clone()))
    }
}
impl<'js> IntoAtom<'js> for JsString<'js> {
    fn into_atom(self, _ctx: &Ctx<'js>) -> Result<Atom<'js>> {
        Ok(Atom(self.into_value()))
    }
}
impl<'js> IntoAtom<'js> for &str {
    fn into_atom(self, ctx: &Ctx<'js>) -> Result<Atom<'js>> {
        Atom::from_str(*ctx, self)
    }
}
impl<'js> IntoAtom<'js> for std::string::String {
    fn into_atom(self, ctx: &Ctx<'js>) -> Result<Atom<'js>> {
        Atom::from_str(*ctx, &self)
    }
}
impl<'js> IntoAtom<'js> for &std::string::String {
    fn into_atom(self, ctx: &Ctx<'js>) -> Result<Atom<'js>> {
        Atom::from_str(*ctx, self)
    }
}
impl<'js> IntoAtom<'js> for std::borrow::Cow<'_, str> {
    fn into_atom(self, ctx: &Ctx<'js>) -> Result<Atom<'js>> {
        Atom::from_str(*ctx, &self)
    }
}
impl<'js> IntoAtom<'js> for &std::ffi::CStr {
    fn into_atom(self, ctx: &Ctx<'js>) -> Result<Atom<'js>> {
        Atom::from_str(*ctx, &self.to_string_lossy())
    }
}
impl<'js> IntoAtom<'js> for PredefinedAtom {
    fn into_atom(self, ctx: &Ctx<'js>) -> Result<Atom<'js>> {
        Ok(Atom::from_predefined(*ctx, self))
    }
}
macro_rules! into_atom_number {
    ($($t:ty),*) => {$(
        impl<'js> IntoAtom<'js> for $t {
            fn into_atom(self, ctx: &Ctx<'js>) -> Result<Atom<'js>> {
                Ok(Atom(Value::new_number(*ctx, self as f64)))
            }
        }
    )*};
}
into_atom_number!(i8, i16, i32, i64, u8, u16, u32, u64, usize, isize, f64);

/// Types a property key can be read back as.
pub trait FromAtom<'js>: Sized {
    fn from_atom(atom: Atom<'js>) -> Result<Self>;
}

impl<'js> FromAtom<'js> for Atom<'js> {
    fn from_atom(atom: Atom<'js>) -> Result<Self> {
        Ok(atom)
    }
}
impl<'js> FromAtom<'js> for Value<'js> {
    fn from_atom(atom: Atom<'js>) -> Result<Self> {
        Ok(atom.0)
    }
}
impl<'js> FromAtom<'js> for std::string::String {
    fn from_atom(atom: Atom<'js>) -> Result<Self> {
        atom.to_string()
    }
}
impl<'js> FromAtom<'js> for JsString<'js> {
    fn from_atom(atom: Atom<'js>) -> Result<Self> {
        atom.to_js_string()
    }
}
impl<'js> FromAtom<'js> for Symbol<'js> {
    fn from_atom(atom: Atom<'js>) -> Result<Self> {
        let type_name = atom.0.type_name();
        atom.0.into_symbol().ok_or_else(|| Error::new_from_js(type_name, "symbol"))
    }
}
macro_rules! from_atom_number {
    ($($t:ty),*) => {$(
        impl<'js> FromAtom<'js> for $t {
            fn from_atom(atom: Atom<'js>) -> Result<Self> {
                let ctx = *atom.ctx();
                <$t as FromJs<'js>>::from_js(&ctx, atom.0)
            }
        }
    )*};
}
from_atom_number!(i8, i16, i32, i64, u8, u16, u32, u64, usize, isize, f64);

/// Commonly used property names, mirroring `rquickjs::atom::PredefinedAtom`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[allow(missing_docs)]
pub enum PredefinedAtom {
    Null, False, True, If, Else, Return, Var, This, Delete, Void, Typeof, New, In, Instanceof,
    Do, While, For, Break, Continue, Switch, Case, Default, Throw, Try, Catch, Finally,
    FunctionKw, Debugger, With, Class, Const, Enum, Export, Extends, Import, Super, Implements,
    Interface, Let, Package, Private, Protected, Public, Static, Yield, Await, Empty, Length,
    Message, Errors, Stack, Name, ToString, ToLocaleString, ValueOf, Eval, Prototype,
    Constructor, Configurable, Writable, Enumerable, Value, Get, Set, Of, Symbol, Iterator,
    AsyncIterator, HasInstance, ToPrimitive, ToStringTag, Then, Resolve, Reject, Promise,
    Proxy, Revoke, Async, Exec, Groups, Status, Reason, GlobalThis, Done, Next, Values,
    Keys, Entries, Source, Flags, Global, Unicode, Raw, Callee, Caller, Arguments, Target,
    Apply, Call, Bind, Undefined, Object, Function, Array, Number, Boolean, String, Error,
    TypeError, RangeError, ReferenceError, SyntaxError, EvalError, URIError, InternalError,
    Map, Set_, WeakMap, WeakSet, JSON, Math, Reflect, Date, RegExp, BigInt, Species, Match,
    MatchAll, Replace, Search, Split, IsConcatSpreadable, Unscopables,
}

impl PredefinedAtom {
    pub const fn as_str(self) -> &'static str {
        use PredefinedAtom::*;
        match self {
            Null => "null", False => "false", True => "true", If => "if", Else => "else", Return => "return",
            Var => "var", This => "this", Delete => "delete", Void => "void", Typeof => "typeof", New => "new",
            In => "in", Instanceof => "instanceof", Do => "do", While => "while", For => "for", Break => "break",
            Continue => "continue", Switch => "switch", Case => "case", Default => "default", Throw => "throw",
            Try => "try", Catch => "catch", Finally => "finally", FunctionKw => "function", Debugger => "debugger",
            With => "with", Class => "class", Const => "const", Enum => "enum", Export => "export",
            Extends => "extends", Import => "import", Super => "super", Implements => "implements",
            Interface => "interface", Let => "let", Package => "package", Private => "private",
            Protected => "protected", Public => "public", Static => "static", Yield => "yield", Await => "await",
            Empty => "", Length => "length", Message => "message", Errors => "errors", Stack => "stack",
            Name => "name", ToString => "toString", ToLocaleString => "toLocaleString", ValueOf => "valueOf",
            Eval => "eval", Prototype => "prototype", Constructor => "constructor", Configurable => "configurable",
            Writable => "writable", Enumerable => "enumerable", Value => "value", Get => "get", Set => "set",
            Of => "of", Symbol => "Symbol", Iterator => "iterator", AsyncIterator => "asyncIterator",
            HasInstance => "hasInstance", ToPrimitive => "toPrimitive", ToStringTag => "toStringTag",
            Then => "then", Resolve => "resolve", Reject => "reject", Promise => "Promise", Proxy => "Proxy",
            Revoke => "revoke", Async => "async", Exec => "exec", Groups => "groups", Status => "status",
            Reason => "reason", GlobalThis => "globalThis", Done => "done", Next => "next", Values => "values",
            Keys => "keys", Entries => "entries", Source => "source", Flags => "flags", Global => "global",
            Unicode => "unicode", Raw => "raw", Callee => "callee", Caller => "caller", Arguments => "arguments",
            Target => "target", Apply => "apply", Call => "call", Bind => "bind", Undefined => "undefined",
            Object => "Object", Function => "Function", Array => "Array", Number => "Number", Boolean => "Boolean",
            String => "String", Error => "Error", TypeError => "TypeError", RangeError => "RangeError",
            ReferenceError => "ReferenceError", SyntaxError => "SyntaxError", EvalError => "EvalError",
            URIError => "URIError", InternalError => "InternalError", Map => "Map", Set_ => "Set",
            WeakMap => "WeakMap", WeakSet => "WeakSet", JSON => "JSON", Math => "Math", Reflect => "Reflect",
            Date => "Date", RegExp => "RegExp", BigInt => "BigInt", Species => "species", Match => "match",
            MatchAll => "matchAll", Replace => "replace", Search => "search", Split => "split",
            IsConcatSpreadable => "isConcatSpreadable", Unscopables => "unscopables",
        }
    }

    /// Whether this is a well-known symbol (`Symbol.<name>`) rather than a
    /// string key.
    pub const fn is_symbol(self) -> bool {
        use PredefinedAtom::*;
        matches!(self, Iterator | AsyncIterator | HasInstance | ToPrimitive | ToStringTag | Species | Match | MatchAll | Replace | Search | Split | IsConcatSpreadable | Unscopables)
    }
}

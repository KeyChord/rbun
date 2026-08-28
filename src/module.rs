//! ES modules, mirroring `rquickjs::module::{Module, ModuleDef, Declarations, Exports}`.
//!
//! Modules declared from Rust live in a per-runtime registry that the
//! `Bun.plugin` installed at boot serves through `onResolve` / `onLoad`.
//! Evaluation always goes through Bun's own loader, so declared modules can
//! freely import Bun/Node builtins and files.

use core::cell::RefCell;
use core::marker::PhantomData;
use std::collections::HashMap;
use std::ffi::CStr;
use std::rc::Rc;

use crate::error::{Error, Result};
use crate::ffi;
use crate::function::{Function, Rest};
use crate::runtime::{Ctx, Runtime};
use crate::value::{FromJs, IntoJs, Object, Promise, Value};

/// Marker: a declared, not yet evaluated module.
pub struct Declared;
/// Marker: an evaluated module.
pub struct Evaluated;

type DefEvaluate = dyn for<'js> Fn(Ctx<'js>) -> Result<Object<'js>>;

pub(crate) enum ModuleSource {
    /// JavaScript / TypeScript source; Bun transpiles it.
    Source { contents: String, loader: &'static str },
    /// A native module definition, evaluated when Bun first loads it.
    Def(Rc<DefEvaluate>),
    /// A ready-made exports object.
    Exports(ffi::JSValueRef),
}

pub(crate) struct ModuleEntry {
    pub(crate) source: ModuleSource,
    /// Stand-in for `import.meta` kept for API parity (Bun sets the real
    /// `import.meta` itself).
    pub(crate) meta: ffi::JSValueRef,
}

#[derive(Default)]
pub(crate) struct ModuleRegistry {
    pub(crate) entries: HashMap<String, ModuleEntry>,
    eval_id: usize,
}

impl ModuleRegistry {
    pub(crate) fn next_eval_id(&self) -> usize {
        self.eval_id
    }
    pub(crate) fn bump_eval_id(&mut self) {
        self.eval_id += 1;
    }
}

/// A module handle.
pub struct Module<'js, T = Declared> {
    ctx: Ctx<'js>,
    name: String,
    _state: PhantomData<T>,
}

impl<'js, T> Module<'js, T> {
    pub fn ctx(&self) -> &Ctx<'js> {
        &self.ctx
    }

    pub fn name<N: FromJs<'js>>(&self) -> Result<N> {
        N::from_js(&self.ctx, self.ctx.string(&self.name))
    }

    pub fn name_str(&self) -> &str {
        &self.name
    }

    /// The module's `import.meta`-like object registered from Rust.
    pub fn meta(&self) -> Result<Object<'js>> {
        let raw = self
            .ctx
            .inner
            .modules
            .borrow()
            .entries
            .get(&self.name)
            .map(|e| e.meta)
            .ok_or_else(|| Error::new_loading(&self.name))?;
        // SAFETY: protected while registered.
        unsafe { Value::from_raw(self.ctx, raw) }
            .into_object()
            .ok_or_else(|| Error::new_loading(&self.name))
    }
}

impl<'js> Module<'js, Declared> {
    /// Declare a module from source. Bun transpiles TypeScript/JSX according
    /// to the specifier's extension (defaults to JS). Re-declaring a name
    /// replaces the previous module for future imports.
    pub fn declare<N: Into<Vec<u8>>, S: Into<Vec<u8>>>(ctx: Ctx<'js>, name: N, source: S) -> Result<Self> {
        let name = String::from_utf8(name.into())?;
        let contents = String::from_utf8(source.into())?;
        let loader = match name.rsplit('.').next() {
            Some("ts") | Some("mts") | Some("cts") => "ts",
            Some("tsx") => "tsx",
            Some("jsx") => "jsx",
            Some("json") => "json",
            _ => "js",
        };
        let meta = Object::new(ctx)?;
        meta.set("url", name.as_str())?;
        register(ctx, name.clone(), ModuleSource::Source { contents, loader }, meta.into_value());
        Ok(Module { ctx, name, _state: PhantomData })
    }

    /// Declare a native module from a [`ModuleDef`]. Like rquickjs the
    /// definition is evaluated lazily, when the module is first imported.
    pub fn declare_def<D: ModuleDef, N: Into<Vec<u8>>>(ctx: Ctx<'js>, name: N) -> Result<Self> {
        let name = String::from_utf8(name.into())?;
        let declarations = Declarations { names: RefCell::new(Vec::new()), _marker: PhantomData };
        D::declare(&declarations)?;
        let declared = declarations.names.into_inner();
        let evaluate: Rc<DefEvaluate> = Rc::new(move |cx: Ctx<'_>| {
            let exports = Exports { object: Object::new(cx)?, declared: declared.clone() };
            D::evaluate(&cx, &exports)?;
            Ok(exports.object)
        });
        let meta = Object::new(ctx)?;
        meta.set("url", name.as_str())?;
        register(ctx, name.clone(), ModuleSource::Def(evaluate), meta.into_value());
        Ok(Module { ctx, name, _state: PhantomData })
    }

    /// Evaluate the module: returns the evaluated handle and the promise of
    /// its evaluation (resolving to the namespace object).
    pub fn eval(self) -> Result<(Module<'js, Evaluated>, Promise<'js>)> {
        let promise = Module::import(&self.ctx, &self.name)?;
        Ok((Module { ctx: self.ctx, name: self.name, _state: PhantomData }, promise))
    }
}

impl<'js> Module<'js, Evaluated> {
    /// The namespace object. Bun only exposes it once evaluation finished, so
    /// this drives the event loop until the module's promise settles.
    pub fn namespace(&self) -> Result<Object<'js>> {
        let promise = Module::import(&self.ctx, &self.name)?;
        promise.finish()
    }

    pub fn get<N: AsRef<str>, V: FromJs<'js>>(&self, name: N) -> Result<V> {
        self.namespace()?.get(name.as_ref())
    }
}

impl<'js> Module<'js> {
    /// `import(specifier)` through Bun's loader. The promise resolves to the
    /// module namespace object.
    pub fn import<S: AsRef<str>>(ctx: &Ctx<'js>, specifier: S) -> Result<Promise<'js>> {
        let import = ctx.function("import")?;
        let value: Value<'js> = import.call((specifier.as_ref(),))?;
        value.into_promise().ok_or_else(|| Error::new_from_js("value", "promise"))
    }

    /// Declare and evaluate a module from source.
    pub fn evaluate<N: Into<Vec<u8>>, S: Into<Vec<u8>>>(ctx: Ctx<'js>, name: N, source: S) -> Result<Promise<'js>> {
        let (_module, promise) = Module::declare(ctx, name, source)?.eval()?;
        Ok(promise)
    }

    /// Declare and evaluate a [`ModuleDef`].
    pub fn evaluate_def<D: ModuleDef, N: Into<Vec<u8>>>(ctx: Ctx<'js>, name: N) -> Result<Promise<'js>> {
        let (_module, promise) = Module::declare_def::<D, _>(ctx, name)?.eval()?;
        Ok(promise)
    }
}

fn protect(value: &Value<'_>) -> ffi::JSValueRef {
    // SAFETY: live value; kept protected while registered.
    unsafe { ffi::JSValueProtect(value.ctx().raw(), value.as_raw()) };
    value.as_raw()
}

fn register(ctx: Ctx<'_>, name: String, source: ModuleSource, meta: Value<'_>) {
    let meta = protect(&meta);
    let previous = ctx.inner.modules.borrow_mut().entries.insert(name.clone(), ModuleEntry { source, meta });
    if let Some(previous) = previous {
        // Re-declaration: make bun forget the old module so the next import
        // loads the new source.
        for key in [name.clone(), format!("rbun:{name}")] {
            // SAFETY: the runtime's VM, on its thread; valid (ptr, len).
            unsafe { ffi::bun_embed_vm_delete_module_registry_entry(ctx.vm(), key.as_ptr(), key.len()) };
        }
        // SAFETY: balances `protect` for the replaced entry.
        unsafe {
            ffi::JSValueUnprotect(ctx.raw(), previous.meta);
            if let ModuleSource::Exports(exports) = previous.source {
                ffi::JSValueUnprotect(ctx.raw(), exports);
            }
        }
    }
}

// ─── ModuleDef ───────────────────────────────────────────────────────────

/// A native module: declares its export names, then fills them in.
pub trait ModuleDef {
    fn declare(declare: &Declarations<'_>) -> Result<()>;
    fn evaluate<'js>(ctx: &Ctx<'js>, exports: &Exports<'js>) -> Result<()>;
}

pub struct Declarations<'a> {
    names: RefCell<Vec<String>>,
    _marker: PhantomData<&'a ()>,
}

impl Declarations<'_> {
    pub fn declare<N: Into<Vec<u8>>>(&self, name: N) -> Result<&Self> {
        let name = String::from_utf8(name.into())?;
        let mut names = self.names.borrow_mut();
        if names.iter().any(|n| *n == name) {
            return Err(Error::DuplicateExports);
        }
        names.push(name);
        drop(names);
        Ok(self)
    }

    pub fn declare_c_str(&self, name: &CStr) -> Result<&Self> {
        self.declare(name.to_str().map_err(Error::Utf8)?)
    }

    pub fn declare_static(&self, name: &'static str) -> Result<&Self> {
        self.declare(name)
    }
}

pub struct Exports<'js> {
    object: Object<'js>,
    declared: Vec<String>,
}

impl<'js> Exports<'js> {
    pub fn export<N: AsRef<str>, T: IntoJs<'js>>(&self, name: N, value: T) -> Result<&Self> {
        let name = name.as_ref();
        if !self.declared.is_empty() && !self.declared.iter().any(|d| d == name) {
            return Err(Error::new_loading_message("module", format!("export `{name}` was not declared")));
        }
        self.object.set(name, value)?;
        Ok(self)
    }

    pub fn export_c_str<T: IntoJs<'js>>(&self, name: &CStr, value: T) -> Result<&Self> {
        self.export(name.to_str().map_err(Error::Utf8)?, value)
    }

    /// The exports object itself.
    pub fn object(&self) -> &Object<'js> {
        &self.object
    }
}

// ─── Host hooks behind the bootstrap plugin ──────────────────────────────

pub(crate) fn install_hooks(rt: &Runtime) -> Result<()> {
    rt.with(|ctx| {
        let rbun: Object<'_> = ctx.globals().get("__rbun")?;
        rbun.set("resolve", Function::new(ctx, host_resolve)?.with_name("resolve")?)?;
        rbun.set("load", Function::new(ctx, host_load)?.with_name("load")?)?;
        Ok(())
    })
}

/// `__rbun.resolve(importer, specifier)`: registry hit → itself; otherwise
/// ask the installed resolver and, when it claims the name, make sure the
/// loader can actually declare it (so a resolver that accepts everything —
/// as rquickjs-style resolvers commonly do — still lets `node:*`, files and
/// packages fall through to Bun); else `undefined`.
fn host_resolve<'js>(ctx: Ctx<'js>, importer: String, specifier: String) -> Result<Value<'js>> {
    if std::env::var_os("RBUN_DEBUG").is_some() {
        eprintln!("[rbun] resolve {specifier:?} from {importer:?}");
    }
    if ctx.inner.modules.borrow().entries.contains_key(&specifier) {
        return Ok(ctx.string(&specifier));
    }
    let resolved = {
        let mut resolver = ctx.inner.resolver.borrow_mut();
        let Some(resolver) = resolver.as_mut() else {
            return Ok(Value::new_undefined(ctx));
        };
        match resolver.resolve(&ctx, &importer, &specifier) {
            Ok(resolved) => resolved,
            Err(Error::Exception) => {
                let _ = ctx.catch();
                return Ok(Value::new_undefined(ctx));
            }
            Err(_) => return Ok(Value::new_undefined(ctx)),
        }
    };
    if ctx.inner.modules.borrow().entries.contains_key(&resolved) {
        return Ok(ctx.string(&resolved));
    }
    let loaded = {
        let mut loader = ctx.inner.loader.borrow_mut();
        match loader.as_mut() {
            Some(loader) => loader.load(&ctx, &resolved),
            None => return Ok(Value::new_undefined(ctx)),
        }
    };
    match loaded {
        Ok(_) => Ok(ctx.string(&resolved)),
        Err(Error::Exception) => {
            let _ = ctx.catch();
            Ok(Value::new_undefined(ctx))
        }
        Err(_) => Ok(Value::new_undefined(ctx)),
    }
}

/// `__rbun.load(name)`: registry entry as `{contents, loader}` /
/// `{exports}`, evaluating native definitions on first load.
fn host_load<'js>(ctx: Ctx<'js>, name: String, _rest: Rest<Value<'js>>) -> Result<Value<'js>> {
    if std::env::var_os("RBUN_DEBUG").is_some() {
        eprintln!("[rbun] load {name:?}");
    }
    if !ctx.inner.modules.borrow().entries.contains_key(&name) {
        let mut loader = ctx.inner.loader.borrow_mut();
        match loader.as_mut() {
            Some(loader) => {
                loader.load(&ctx, &name)?;
            }
            None => return Ok(Value::new_undefined(ctx)),
        }
    }
    let result = Object::new(ctx)?;
    enum Pending<'js> {
        Source(String, &'static str),
        Exports(Value<'js>),
        Def(Rc<DefEvaluate>),
    }
    let pending = {
        let registry = ctx.inner.modules.borrow();
        let Some(entry) = registry.entries.get(&name) else {
            return Ok(Value::new_undefined(ctx));
        };
        match &entry.source {
            ModuleSource::Source { contents, loader } => Pending::Source(contents.clone(), loader),
            // SAFETY: protected while registered.
            ModuleSource::Exports(exports) => Pending::Exports(unsafe { Value::from_raw(ctx, *exports) }),
            ModuleSource::Def(evaluate) => Pending::Def(evaluate.clone()),
        }
    };
    match pending {
        Pending::Source(contents, loader) => {
            result.set("contents", contents)?;
            result.set("loader", loader)?;
        }
        Pending::Exports(exports) => {
            result.set("exports", exports)?;
        }
        Pending::Def(evaluate) => {
            let exports = evaluate(ctx)?;
            // Cache the evaluated exports so re-imports see the same object.
            let raw = protect(exports.as_value());
            if let Some(entry) = ctx.inner.modules.borrow_mut().entries.get_mut(&name) {
                entry.source = ModuleSource::Exports(raw);
            }
            result.set("exports", exports)?;
        }
    }
    Ok(result.into_value())
}

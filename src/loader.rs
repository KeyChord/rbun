//! Module resolution hooks, mirroring `rquickjs::loader::{Resolver, Loader}`.
//!
//! Bun's module loader consults the installed [`Resolver`] first (through a
//! `Bun.plugin` `onResolve` hook); when it returns `Ok(name)` the module is
//! served from the runtime's module registry, asking the [`Loader`] to
//! declare it on first use. When every resolver fails, Bun falls back to its
//! own resolution (`node:*`, `bun:*`, files, `node_modules`, …) — unlike
//! rquickjs, an unresolved specifier is not an error yet.

use crate::error::{Error, Result};
use crate::module::{Declared, Module};
use crate::runtime::Ctx;

pub trait Resolver {
    /// Resolve `name` imported from `base` (the importing module's name, or
    /// `""` for imports made directly from Rust / the host script).
    fn resolve<'js>(&mut self, ctx: &Ctx<'js>, base: &str, name: &str) -> Result<String>;
}

pub trait Loader {
    /// Declare the module `name` (typically with [`Module::declare`] or
    /// [`Module::declare_def`]).
    fn load<'js>(&mut self, ctx: &Ctx<'js>, name: &str) -> Result<Module<'js, Declared>>;
}

macro_rules! impl_tuple {
    ($($t:ident),+) => {
        impl<$($t: Resolver),+> Resolver for ($($t,)+) {
            #[allow(non_snake_case, unused_assignments)]
            fn resolve<'js>(&mut self, ctx: &Ctx<'js>, base: &str, name: &str) -> Result<String> {
                let ($($t,)+) = self;
                let mut last = Error::new_resolving(base, name);
                $(
                    match $t.resolve(ctx, base, name) {
                        Ok(resolved) => return Ok(resolved),
                        Err(error) => last = error,
                    }
                )+
                Err(last)
            }
        }
        impl<$($t: Loader),+> Loader for ($($t,)+) {
            #[allow(non_snake_case, unused_assignments)]
            fn load<'js>(&mut self, ctx: &Ctx<'js>, name: &str) -> Result<Module<'js, Declared>> {
                let ($($t,)+) = self;
                let mut last = Error::new_loading(name);
                $(
                    match $t.load(ctx, name) {
                        Ok(module) => return Ok(module),
                        Err(error) => last = error,
                    }
                )+
                Err(last)
            }
        }
    };
}
impl_tuple!(A);
impl_tuple!(A, B);
impl_tuple!(A, B, C);
impl_tuple!(A, B, C, D);
impl_tuple!(A, B, C, D, E);
impl_tuple!(A, B, C, D, E, F);

impl Resolver for Box<dyn Resolver> {
    fn resolve<'js>(&mut self, ctx: &Ctx<'js>, base: &str, name: &str) -> Result<String> {
        (**self).resolve(ctx, base, name)
    }
}

impl Loader for Box<dyn Loader> {
    fn load<'js>(&mut self, ctx: &Ctx<'js>, name: &str) -> Result<Module<'js, Declared>> {
        (**self).load(ctx, name)
    }
}

/// Resolver that only accepts an explicit list of module names.
#[derive(Debug, Default)]
pub struct BuiltinResolver {
    modules: Vec<String>,
}

impl BuiltinResolver {
    pub fn add_module<N: Into<String>>(&mut self, name: N) -> &mut Self {
        self.modules.push(name.into());
        self
    }

    pub fn with_module<N: Into<String>>(mut self, name: N) -> Self {
        self.add_module(name);
        self
    }
}

impl Resolver for BuiltinResolver {
    fn resolve<'js>(&mut self, _ctx: &Ctx<'js>, base: &str, name: &str) -> Result<String> {
        if self.modules.iter().any(|m| m == name) { Ok(name.to_string()) } else { Err(Error::new_resolving(base, name)) }
    }
}

/// Loader serving modules from in-memory sources.
#[derive(Debug, Default)]
pub struct ScriptLoader {
    modules: std::collections::HashMap<String, String>,
}

impl ScriptLoader {
    pub fn add_module<N: Into<String>, S: Into<String>>(&mut self, name: N, source: S) -> &mut Self {
        self.modules.insert(name.into(), source.into());
        self
    }

    pub fn with_module<N: Into<String>, S: Into<String>>(mut self, name: N, source: S) -> Self {
        self.add_module(name, source);
        self
    }
}

impl Loader for ScriptLoader {
    fn load<'js>(&mut self, ctx: &Ctx<'js>, name: &str) -> Result<Module<'js, Declared>> {
        match self.modules.get(name) {
            Some(source) => Module::declare(*ctx, name, source.as_str()),
            None => Err(Error::new_loading(name)),
        }
    }
}

/// Loader for [`ModuleDef`](crate::module::ModuleDef) modules.
#[derive(Default)]
pub struct ModuleLoader {
    #[allow(clippy::type_complexity)]
    modules: std::collections::HashMap<String, Box<dyn for<'js> Fn(Ctx<'js>, &str) -> Result<Module<'js, Declared>>>>,
}

impl ModuleLoader {
    pub fn add_module<N: Into<String>, D: crate::module::ModuleDef>(&mut self, name: N, _def: D) -> &mut Self {
        self.modules.insert(name.into(), Box::new(|ctx, name| Module::declare_def::<D, _>(ctx, name)));
        self
    }

    pub fn with_module<N: Into<String>, D: crate::module::ModuleDef>(mut self, name: N, def: D) -> Self {
        self.add_module(name, def);
        self
    }
}

impl Loader for ModuleLoader {
    fn load<'js>(&mut self, ctx: &Ctx<'js>, name: &str) -> Result<Module<'js, Declared>> {
        match self.modules.get(name) {
            Some(declare) => declare(*ctx, name),
            None => Err(Error::new_loading(name)),
        }
    }
}

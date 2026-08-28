//! Probe: import a handler module from disk by absolute path (real
//! `import.meta.dir`, relative imports, `bun:ffi`) while a bare `chord`
//! import still resolves through the Rust resolver.
use rbun::loader::{Loader, Resolver};
use rbun::module::{Declarations, Exports, ModuleDef};
use rbun::{Context, Ctx, Function, Module, Object, Runtime, Value};

struct ChordModule;
impl ModuleDef for ChordModule {
    fn declare(decl: &Declarations) -> rbun::Result<()> {
        decl.declare("tap")?;
        Ok(())
    }
    fn evaluate<'js>(ctx: &Ctx<'js>, exports: &Exports<'js>) -> rbun::Result<()> {
        exports.export("tap", Function::new(ctx.clone(), |s: String| format!("tapped-{s}"))?)?;
        Ok(())
    }
}
struct R;
impl Resolver for R {
    fn resolve<'js>(&mut self, _ctx: &Ctx<'js>, _base: &str, name: &str) -> rbun::Result<String> {
        if name == "chord" { Ok(name.into()) } else { Err(rbun::Error::new_resolving(_base, name)) }
    }
}
struct L;
impl Loader for L {
    fn load<'js>(&mut self, ctx: &Ctx<'js>, name: &str) -> rbun::Result<Module<'js>> {
        if name == "chord" { Module::declare_def::<ChordModule, _>(ctx.clone(), name) } else { Err(rbun::Error::new_loading(name)) }
    }
}

fn main() {
    let path = std::env::args().nth(1).expect("abs path to js module");
    let rt = Runtime::new().unwrap();
    rt.set_loader(R, L);
    let ctx = Context::full(&rt).unwrap();
    ctx.with(|ctx| {
        let promise = ctx.import(&path).unwrap();
        let ns: Object = promise.finish().unwrap();
        let build: Function = ns.get("default").unwrap();
        let handler: Function = build.call(("pfx",)).unwrap();
        let this = Object::new(ctx.clone()).unwrap();
        this.set("focusedAppId", "com.example").unwrap();
        let out: Value = handler.call((rbun::function::This(this), "40", "2")).unwrap();
        println!("{}", out.to_string_lossy());
    });
}

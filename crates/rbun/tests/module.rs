//! Ported from rquickjs-core `src/value/module.rs` and `src/loader.rs`.

mod common;

use common::{js_thread, test_with};
use rbun::{CatchResultExt, Context, Ctx, Declarations, Error, Exports, Function, Loader, Module, ModuleDef, Object, Resolver, Runtime, Value};
use std::string::String as StdString;

pub struct RustModule;

impl ModuleDef for RustModule {
    fn declare(define: &Declarations) -> rbun::Result<()> {
        define.declare_c_str(c"hello")?;
        Ok(())
    }

    fn evaluate<'js>(_ctx: &Ctx<'js>, exports: &Exports<'js>) -> rbun::Result<()> {
        exports.export_c_str(c"hello", "world")?;
        Ok(())
    }
}

pub struct CrashingRustModule;

impl ModuleDef for CrashingRustModule {
    fn declare(_: &Declarations) -> rbun::Result<()> {
        Ok(())
    }

    fn evaluate<'js>(ctx: &Ctx<'js>, _exports: &Exports<'js>) -> rbun::Result<()> {
        ctx.eval::<(), _>(r#"throw new Error("kaboom")"#)?;
        Ok(())
    }
}

#[test]
fn from_rust_def() {
    test_with(|ctx| {
        Module::declare_def::<RustModule, _>(ctx, "rust_mod_decl").unwrap();
    })
}

#[test]
fn from_rust_def_eval() {
    test_with(|ctx| {
        let _ = Module::evaluate_def::<RustModule, _>(ctx, "rust_mod_eval").unwrap();
    })
}

#[test]
fn import_native() {
    test_with(|ctx| {
        Module::declare_def::<RustModule, _>(ctx.clone(), "rust_mod").unwrap();
        Module::evaluate(
            ctx.clone(),
            "test_import_native",
            r#"
            import { hello } from "rust_mod";

            globalThis.hello = hello;
        "#,
        )
        .unwrap()
        .finish::<()>()
        .unwrap();
        let text = ctx.globals().get::<_, rbun::String>("hello").unwrap().to_string().unwrap();
        assert_eq!(text.as_str(), "world");
    })
}

#[test]
fn import_async() {
    test_with(|ctx| {
        Module::declare(
            ctx.clone(),
            "rust_mod_async",
            "
            async function foo(){
                return 'world';
            };
            export let hello = await foo();
        ",
        )
        .unwrap();
        Module::evaluate(
            ctx.clone(),
            "test_import_async",
            r#"
            import { hello } from "rust_mod_async";
            globalThis.hello_async = hello;
        "#,
        )
        .unwrap()
        .finish::<()>()
        .unwrap();
        let text = ctx.globals().get::<_, rbun::String>("hello_async").unwrap().to_string().unwrap();
        assert_eq!(text.as_str(), "world");
    })
}

#[test]
fn import() {
    test_with(|ctx| {
        Module::declare_def::<RustModule, _>(ctx.clone(), "rust_mod_import").unwrap();
        let val: Object = Module::import(&ctx, "rust_mod_import").unwrap().finish().unwrap();
        let hello: StdString = val.get("hello").unwrap();

        assert_eq!(&hello, "world");
    })
}

#[test]
#[should_panic(expected = "kaboom")]
fn import_crashing() {
    js_thread(|| {
        let runtime = Runtime::new().unwrap();
        let ctx = Context::full(&runtime).unwrap();
        ctx.with(|ctx| {
            Module::declare_def::<CrashingRustModule, _>(ctx.clone(), "bad_rust_mod").unwrap();
            let _: Value = Module::import(&ctx, "bad_rust_mod").catch(&ctx).unwrap().finish().catch(&ctx).unwrap();
        });
    })
}

/// Bun's module loader is not re-entrant: synchronously evaluating another
/// module (`Module::evaluate(..).finish()`) from inside a host function that
/// runs during module evaluation crashes the VM.
#[test]
#[ignore = "nested synchronous module evaluation from a host call is not supported by Bun's loader"]
fn eval_crashing_module_inside_module() {
    js_thread(|| {
        let runtime = Runtime::new().unwrap();
        let ctx = Context::full(&runtime).unwrap();

        ctx.with(|ctx| {
            let globals = ctx.globals();
            let eval_crashing = |ctx: Ctx| Module::evaluate(ctx, "test2_crash", "throw new Error(1)").map(|x| x.finish::<()>());
            let function = Function::new(ctx.clone(), eval_crashing).unwrap();
            globals.set("eval_crashing", function).unwrap();

            let res = Module::evaluate(ctx, "test_crash", " eval_crashing(); ").unwrap().finish::<()>();
            assert!(res.is_err())
        });
    })
}

/// rbun can only hand out a module namespace once Bun finished evaluating
/// the module, so `namespace()` drives evaluation to completion instead of
/// exposing a half-initialised binding.
#[test]
fn access_before_fully_evaluating_module() {
    js_thread(|| {
        let runtime = Runtime::new().unwrap();
        let ctx = Context::full(&runtime).unwrap();

        ctx.with(|ctx| {
            let decl = Module::declare(
                ctx,
                "test_tla",
                r#"
                async function async_res(){
                    return await (async () => {
                        return "OK"
                    })()
                };

                export let res = await async_res()
            "#,
            )
            .unwrap();

            let (decl, promise) = decl.eval().unwrap();

            let ns = decl.namespace().unwrap();
            promise.finish::<()>().unwrap();

            assert_eq!(ns.get::<_, std::string::String>("res").unwrap(), "OK");
        });
    })
}

#[test]
fn from_javascript() {
    test_with(|ctx| {
        let (module, promise) = Module::declare(
            ctx.clone(),
            "Test",
            r#"
        export var a = 2;
        export function foo(){ return "bar"}
        export class Baz{
            quel = 3;
            constructor(){
            }
        }
            "#,
        )
        .unwrap()
        .eval()
        .unwrap();

        promise.finish::<()>().unwrap();

        assert_eq!(module.name::<StdString>().unwrap(), "Test");
        let _ = module.meta().unwrap();

        let ns = module.namespace().unwrap();

        assert!(ns.contains_key("a").unwrap());
        assert!(ns.contains_key("foo").unwrap());
        assert!(ns.contains_key("Baz").unwrap());

        assert_eq!(ns.get::<_, u32>("a").unwrap(), 2u32);
    });
}

// ─── loader.rs ───

struct TestResolver;

impl Resolver for TestResolver {
    fn resolve<'js>(&mut self, _ctx: &Ctx<'js>, base: &str, name: &str) -> rbun::Result<String> {
        if base == "loader" && name == "test" {
            Ok(name.into())
        } else {
            Err(Error::new_resolving_message(base, name, "unable to resolve"))
        }
    }
}

struct TestLoader;

impl Loader for TestLoader {
    fn load<'js>(&mut self, ctx: &Ctx<'js>, name: &str) -> rbun::Result<Module<'js>> {
        if name == "test" {
            Module::declare(
                ctx.clone(),
                "test",
                r#"
                  export const n = 123;
                  export const s = "abc";
                "#,
            )
        } else {
            Err(Error::new_loading_message(name, "unable to load"))
        }
    }
}

#[test]
fn custom_loader() {
    js_thread(|| {
        let rt = Runtime::new().unwrap();
        let ctx = Context::full(&rt).unwrap();
        rt.set_loader(TestResolver, TestLoader);
        ctx.with(|ctx| {
            Module::evaluate(
                ctx,
                "loader",
                r#"
                  import { n, s } from "test";
                  export default [n, s];
                "#,
            )
            .unwrap()
            .finish::<()>()
            .unwrap();
        })
    })
}

// When the Rust resolver declines, rbun falls through to Bun's own
// resolution, so the failure is Bun's "Cannot find package" rather than
// rquickjs' "Error resolving module".
#[test]
#[should_panic(expected = "Cannot find")]
fn resolving_error() {
    js_thread(|| {
        let rt = Runtime::new().unwrap();
        let ctx = Context::full(&rt).unwrap();
        rt.set_loader(TestResolver, TestLoader);
        ctx.with(|ctx| {
            Module::evaluate(
                ctx.clone(),
                "loader_error",
                r#"
                  import { n, s } from "test_";
                "#,
            )
            .catch(&ctx)
            .unwrap()
            .finish::<()>()
            .catch(&ctx)
            .expect("Unable to resolve");
        })
    })
}

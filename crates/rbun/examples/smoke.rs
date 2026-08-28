//! `cargo run --example smoke` — exercises the embedding end to end through
//! the rquickjs-shaped API.

use rbun::prelude::*;
use rbun::{AsyncContext, AsyncRuntime, CaughtError, Loader, Module, Persistent, Resolver, async_with};
use std::collections::HashMap;

#[rbun::class]
#[derive(rbun::derive::Trace, rbun::derive::JsLifetime)]
struct Counter {
    n: i64,
}

#[rbun::methods(rename_all = "camelCase")]
impl Counter {
    #[qjs(constructor)]
    fn new<'js>(_ctx: Ctx<'js>, args: Rest<Value<'js>>) -> rbun::Result<Self> {
        Ok(Counter { n: args.0.first().and_then(|a| a.as_number()).unwrap_or(0.0) as i64 })
    }

    fn inc<'js>(&mut self, _ctx: Ctx<'js>) -> rbun::Result<i64> {
        self.n += 1;
        Ok(self.n)
    }

    fn add_many<'js>(&mut self, ctx: Ctx<'js>, values: Vec<i64>) -> rbun::Result<Value<'js>> {
        self.n += values.iter().sum::<i64>();
        Ok(Value::new_int(ctx, self.n as i32))
    }
}

/// A native module, rquickjs-style.
struct MathModule;

impl ModuleDef for MathModule {
    fn declare(declare: &Declarations<'_>) -> rbun::Result<()> {
        declare.declare("add")?;
        declare.declare("slowAdd")?;
        declare.declare("Counter")?;
        Ok(())
    }

    fn evaluate<'js>(ctx: &Ctx<'js>, exports: &Exports<'js>) -> rbun::Result<()> {
        exports.export("add", Func::from(|a: f64, b: f64| a + b))?;
        exports.export(
            "slowAdd",
            Func::from(Async(|a: f64, b: f64| async move {
                tokio::time::sleep(std::time::Duration::from_millis(30)).await;
                Ok::<_, rbun::Error>(a + b)
            })),
        )?;
        Class::<Counter>::define(&ctx.globals())?;
        let ctor: Value<'js> = ctx.globals().get(Counter::NAME)?;
        exports.export("Counter", ctor)?;
        Ok(())
    }
}

struct PackageResolver;
impl Resolver for PackageResolver {
    fn resolve<'js>(&mut self, _ctx: &Ctx<'js>, base: &str, name: &str) -> rbun::Result<String> {
        if name.starts_with("@test/") { Ok(name.to_string()) } else { Err(rbun::Error::new_resolving(base, name)) }
    }
}

struct PackageLoader(HashMap<String, String>);
impl Loader for PackageLoader {
    fn load<'js>(&mut self, ctx: &Ctx<'js>, name: &str) -> rbun::Result<Module<'js>> {
        match self.0.get(name) {
            Some(source) => Module::declare(*ctx, name, source.as_str()),
            None => Err(rbun::Error::new_loading(name)),
        }
    }
}

struct AppUserData {
    tag: &'static str,
}
unsafe impl<'js> JsLifetime<'js> for AppUserData {
    type Changed<'to> = AppUserData;
}

fn main() -> anyhow::Result<()> {
    let tokio = tokio::runtime::Builder::new_current_thread().enable_all().build()?;
    tokio.block_on(async {
        let rt = AsyncRuntime::new()?;
        rt.set_max_stack_size(1024 * 1024).await;
        let mut sources = HashMap::new();
        sources.insert(
            "@test/pkg/js/a.js".to_string(),
            "import { b } from '@test/pkg/js/b.js'; import { add } from 'math'; export default async (x) => { await new Promise(r => setTimeout(r, 20)); return 'a+' + b + '=' + add(x, 1); }".to_string(),
        );
        sources.insert("@test/pkg/js/b.js".to_string(), "export const b = 'b';".to_string());
        rt.set_loader(
            (PackageResolver, rbun::loader::BuiltinResolver::default().with_module("math")),
            (PackageLoader(sources), rbun::loader::ModuleLoader::default().with_module("math", MathModule)),
        )
        .await;
        let context = AsyncContext::full(&rt).await?;

        async_with!(context => |ctx| {
            async {
            ctx.store_userdata(AppUserData { tag: "chord" })?;

            let v: f64 = ctx.eval("1 + 2")?;
            println!("1 + 2 = {v}");
            let v: String = ctx.eval("`${typeof Bun} ${Bun.version} ${typeof process} ${typeof setTimeout}`")?;
            println!("globals: {v}");

            let globals = ctx.globals();
            let f = Function::new(ctx.clone(), |cx: Ctx<'_>, this: This<Value<'_>>, first: String, rest: Rest<i32>| -> rbun::Result<String> {
                let tag = cx.userdata::<AppUserData>().map(|u| u.tag).unwrap_or("?");
                Ok(format!("{tag}:{}:{first}:{:?}", this.0.type_name(), rest.0))
            })?
            .with_name("hostFn")?;
            globals.set("hostFn", f)?;
            let v: String = ctx.eval("hostFn.call({}, 'x', 1, 2, 3) + ' ' + hostFn.name")?;
            println!("host fn: {v}");

            Class::<Counter>::define(&globals)?;
            let v: String = ctx.eval("const c = new Counter(40); c.inc(); [c.addMany([1, 1]), c instanceof Counter, typeof Counter].join(' ')")?;
            println!("class: {v}");
            let ctor: Function = globals.get("Counter")?;
            let saved = Persistent::<Function<'static>>::save(&ctx, ctor);
            let restored = saved.restore(&ctx)?;
            let instance: Object = restored.construct((5,))?;
            let inc: Function = instance.get("inc")?;
            let n: i64 = inc.call((This(instance.clone()),))?;
            println!("persistent ctor -> inc = {n}");

            let module = Module::import(&ctx, "@test/pkg/js/a.js")?.into_future::<Object>().await?;
            let default: Function = module.get("default")?;
            let result: Promise = default.call((41,))?;
            let result: String = result.into_future().await?;
            println!("module: {result}");

            let v: Promise = ctx.eval("(async () => { const { slowAdd } = await import('math'); return await slowAdd(2, 3); })()")?;
            let v: f64 = v.into_future().await?;
            println!("async host fn: {v}");

            let err = ctx.eval::<Promise, _>("(async () => { throw new TypeError('boom') })()")?
                .into_future::<()>().await.unwrap_err();
            match CaughtError::from_error(&ctx, err) {
                CaughtError::Exception(e) => println!("caught: {:?} {:?}", e.name(), e.message()),
                other => println!("unexpected: {other:?}"),
            }
            let err = ctx.eval::<Value, _>("throw 42").unwrap_err();
            println!("thrown value: {}", rbun::format_error(&ctx, err));

            let v = rbun::serde::to_value(ctx.clone(), serde_json::json!({"a": [1, 2, {"b": "c"}]}))?;
            let json = ctx.json_stringify(&v)?.map(|s| s.to_string()).transpose()?;
            println!("serde: {json:?}");

            let os = Module::import(&ctx, "node:os")?.into_future::<Object>().await?;
            let platform: Function = os.get("platform")?;
            println!("node:os platform: {}", platform.call::<_, String>(())?);

            let (_m, promise) = Module::declare(ctx.clone(), "mem/hello.ts", "export const hello: string = 'hi from ts'; globalThis.__hello = hello;")?.eval()?;
            promise.into_future::<()>().await?;
            let hello: String = ctx.eval("__hello")?;
            println!("declared module: {hello}");
            Ok::<_, rbun::Error>(())
            }.await.map_err(|e| anyhow::anyhow!("{}", rbun::format_error(&ctx, e)))
        })
        .await?;
        rt.idle().await;
        println!("ok");
        Ok::<_, anyhow::Error>(())
    })
}

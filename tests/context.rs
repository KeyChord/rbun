//! Ported from rquickjs-core `src/context/ctx.rs`, `src/context/base.rs`,
//! `src/context/builder.rs`, `src/runtime/base.rs`.

mod common;

use common::{js_thread, test_with};
use rbun::context::{EvalOptions, intrinsic};
use rbun::prelude::*;
use rbun::{Array, CatchResultExt, Context, Function, JsLifetime, Module, Object, Promise, Runtime, Type, Value};

#[test]
fn exports() {
    js_thread(|| {
        let runtime = Runtime::new().unwrap();
        let ctx = Context::custom::<(intrinsic::Promise, intrinsic::Eval)>(&runtime).unwrap();
        ctx.with(|ctx| {
            let (module, promise) = Module::declare(ctx, "test_exports", "export default async () => 1;")
                .unwrap()
                .eval()
                .unwrap();
            promise.finish::<()>().unwrap();
            let func: Function = module.get("default").unwrap();
            func.call::<(), Promise>(()).unwrap();
        });
    })
}

#[test]
fn eval() {
    test_with(|ctx| {
        let res: String = ctx
            .eval(
                r#"
                function test() {
                    var foo = "bar";
                    return foo;
                }

                test()
            "#,
            )
            .unwrap();

        assert_eq!("bar".to_string(), res);
    })
}

#[test]
fn eval_minimal_test() {
    test_with(|ctx| {
        let res: i32 = ctx.eval(" 1 + 1 ").unwrap();
        assert_eq!(2, res);
    })
}

// Tests share one global scope, so each sloppy-mode test uses its own name.
#[test]
#[should_panic(expected = "foo_sloppy is not defined")]
fn eval_with_sloppy_code() {
    test_with(|ctx| {
        let _: String = ctx
            .eval(
                r#"
                function test() {
                    foo_sloppy = "bar";
                    return foo_sloppy;
                }

                test()
            "#,
            )
            .catch(&ctx)
            .unwrap();
    })
}

#[test]
fn eval_with_options_no_strict_sloppy_code() {
    test_with(|ctx| {
        let res: String = ctx
            .eval_with_options(
                r#"
                function test() {
                    foo_nonstrict = "bar";
                    return foo_nonstrict;
                }

                test()
            "#,
                EvalOptions { strict: false, ..Default::default() },
            )
            .unwrap();

        assert_eq!("bar".to_string(), res);
    })
}

#[test]
#[should_panic(expected = "foo_strict is not defined")]
fn eval_with_options_strict_sloppy_code() {
    test_with(|ctx| {
        let _: String = ctx
            .eval_with_options(
                r#"
                function test() {
                    foo_strict = "bar";
                    return foo_strict;
                }

                test()
            "#,
                EvalOptions { strict: true, ..Default::default() },
            )
            .catch(&ctx)
            .unwrap();
    })
}

#[test]
fn json_parse() {
    test_with(|ctx| {
        let v = ctx.json_parse(r#"{ "a": { "b": 1, "c": true }, "d": [0,"foo"] }"#).unwrap();
        let obj = v.into_object().unwrap();
        let inner_obj: Object = obj.get("a").unwrap();
        assert_eq!(inner_obj.get::<_, i32>("b").unwrap(), 1);
        assert!(inner_obj.get::<_, bool>("c").unwrap());
        let inner_array: Array = obj.get("d").unwrap();
        assert_eq!(inner_array.get::<i32>(0).unwrap(), 0);
        assert_eq!(inner_array.get::<String>(1).unwrap(), "foo".to_string());
    })
}

#[test]
fn json_stringify() {
    test_with(|ctx| {
        let obj_inner = Object::new(ctx.clone()).unwrap();
        obj_inner.set("b", 1).unwrap();
        obj_inner.set("c", true).unwrap();

        let array_inner = Array::new(ctx.clone()).unwrap();
        array_inner.set(0, 0).unwrap();
        array_inner.set(1, "foo").unwrap();

        let obj = Object::new(ctx.clone()).unwrap();
        obj.set("a", obj_inner).unwrap();
        obj.set("d", array_inner).unwrap();

        let str = ctx.json_stringify(obj).unwrap().unwrap().to_string().unwrap();

        assert_eq!(str, r#"{"a":{"b":1,"c":true},"d":[0,"foo"]}"#);
    })
}

#[test]
fn userdata() {
    pub struct MyUserData<'js> {
        base: Function<'js>,
    }

    unsafe impl<'js> JsLifetime<'js> for MyUserData<'js> {
        type Changed<'to> = MyUserData<'to>;
    }

    js_thread(|| {
        let rt = Runtime::new().unwrap();
        let ctx = Context::full(&rt).unwrap();

        ctx.with(|ctx| {
            let func = ctx.eval("() => 42").catch(&ctx).unwrap();
            ctx.store_userdata(MyUserData { base: func }).unwrap();
        });

        ctx.with(|ctx| {
            let userdata = ctx.userdata::<MyUserData>().unwrap();

            assert!(ctx.remove_userdata::<MyUserData>().is_err());

            let r: usize = userdata.base.call(()).unwrap();
            assert_eq!(r, 42)
        });

        ctx.with(|ctx| {
            ctx.remove_userdata::<MyUserData>().unwrap().unwrap();
        })
    })
}

// ─── context/base.rs ───

#[test]
fn basic() {
    test_with(|ctx| {
        let val: Value = ctx.eval(r#"1+1"#).unwrap();

        assert_eq!(val.type_of(), Type::Int);
        assert_eq!(i32::from_js(&ctx, val).unwrap(), 2);
        println!("{:?}", ctx.globals());
    });
}

#[test]
fn minimal() {
    js_thread(|| {
        let rt = Runtime::new().unwrap();
        let ctx = Context::builder().with::<intrinsic::Eval>().build(&rt).unwrap();
        ctx.with(|ctx| {
            let val: i32 = ctx.eval(r#"1+1"#).unwrap();
            assert_eq!(val, 2);
        });
    })
}

#[test]
fn base() {
    js_thread(|| {
        let rt = Runtime::new().unwrap();
        let _ = Context::base(&rt).unwrap();
    })
}

#[test]
fn module() {
    test_with(|ctx| {
        Module::evaluate(
            ctx,
            "test_mod",
            r#"
                let t = "3";
                let b = (a) => a + 3;
                export { b, t}
            "#,
        )
        .unwrap()
        .finish::<()>()
        .unwrap();
    });
}

#[test]
fn clone_ctx() {
    js_thread(|| {
        let rt = Runtime::new().unwrap();
        let ctx = Context::builder().with::<intrinsic::Eval>().build(&rt).unwrap();

        let ctx_clone = ctx.clone();

        ctx.with(|ctx| {
            let val: i32 = ctx.eval(r#"1+1"#).unwrap();
            assert_eq!(val, 2);
        });

        ctx_clone.with(|ctx| {
            let val: i32 = ctx.eval(r#"1+1"#).unwrap();
            assert_eq!(val, 2);
        });
    })
}

// JSC's syntax error text differs from QuickJS'; only the class is checked.
#[test]
#[should_panic(expected = "SyntaxError")]
fn exception() {
    test_with(|ctx| {
        let val = ctx.eval::<(), _>("bla?#@!@ ").catch(&ctx);
        if let Err(e) = val {
            assert!(e.is_exception());
            panic!("{}", e);
        }
    });
}

// ─── context/builder.rs ───

#[test]
fn all_intrinsics() {
    js_thread(|| {
        let rt = Runtime::new().unwrap();
        let ctx = Context::builder().with::<intrinsic::All>().build(&rt).unwrap();
        let result: usize = ctx.with(|ctx| ctx.eval("1+1")).unwrap();
        assert_eq!(result, 2);
    })
}

// ─── runtime/base.rs ───

#[test]
fn base_runtime() {
    js_thread(|| {
        let rt = Runtime::new().unwrap();
        rt.set_info("test runtime").unwrap();
        rt.set_memory_limit(0xFFFF);
        rt.set_gc_threshold(0xFF);
        rt.run_gc();
    })
}

// ─── runtime/raw.rs ───
// Bun reports unhandled rejections through `process.on("unhandledRejection")`;
// the host tracker is stored but not driven by the engine.
#[test]
#[ignore = "host promise rejection tracker is not wired to Bun's unhandled-rejection reporting"]
fn promise_rejection_handler() {
    js_thread(|| {
        let counter = std::sync::Arc::new(std::sync::Mutex::new(0));
        let rt = Runtime::new().unwrap();
        {
            let counter = counter.clone();
            rt.set_host_promise_rejection_tracker(Some(Box::new(move |_, _, _, is_handled| {
                if !is_handled {
                    *counter.lock().unwrap() += 1;
                }
            })));
        }
        let context = Context::full(&rt).unwrap();
        context.with(|ctx| {
            let _: core::result::Result<(), _> = ctx.eval(
                r#"
                const x = async () => {
                    throw new Error("Uncaught")
                }
                x()
                throw new Error("Caught")
            "#,
            );
        });
        assert_eq!(*counter.lock().unwrap(), 1);
    })
}

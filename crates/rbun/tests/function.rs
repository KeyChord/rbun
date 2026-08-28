//! Ported from rquickjs-core `src/value/function.rs`.

mod common;

use approx::assert_abs_diff_eq as assert_approx_eq;
use common::{js_thread, test_with};
use rbun::prelude::*;
use rbun::{Context, Error, Exception, FromJs, Function, Object, Runtime, Type, Value};
use std::string::String as StdString;

#[test]
fn call_js_fn_with_no_args_and_no_return() {
    test_with(|ctx| {
        let f: Function = ctx.eval("() => {}").unwrap();

        let _: () = ().apply(&f).unwrap();
        let _: () = f.call(()).unwrap();
    })
}

#[test]
fn call_js_fn_with_no_args_and_return() {
    test_with(|ctx| {
        let f: Function = ctx.eval("() => 42").unwrap();

        let res: i32 = ().apply(&f).unwrap();
        assert_eq!(res, 42);

        let res: i32 = f.call(()).unwrap();
        assert_eq!(res, 42);
    })
}

#[test]
fn call_js_fn_with_1_arg_and_return() {
    test_with(|ctx| {
        let f: Function = ctx.eval("a => a + 4").unwrap();

        let res: i32 = (3,).apply(&f).unwrap();
        assert_eq!(res, 7);

        let res: i32 = f.call((1,)).unwrap();
        assert_eq!(res, 5);
    })
}

#[test]
fn call_js_fn_with_2_args_and_return() {
    test_with(|ctx| {
        let f: Function = ctx.eval("(a, b) => a * b + 4").unwrap();

        let res: i32 = (3, 4).apply(&f).unwrap();
        assert_eq!(res, 16);

        let res: i32 = f.call((5, 1)).unwrap();
        assert_eq!(res, 9);
    })
}

#[test]
fn call_js_fn_with_var_args_and_return() {
    let res: Vec<i8> = test_with(|ctx| {
        let func: Function = ctx
            .eval(
                r#"
              (...x) => [x.length, ...x]
            "#,
            )
            .unwrap();
        func.call((Rest(vec![1, 2, 3]),)).unwrap()
    });
    assert_eq!(res.len(), 4);
    assert_eq!(res[0], 3);
    assert_eq!(res[1], 1);
    assert_eq!(res[2], 2);
    assert_eq!(res[3], 3);
}

#[test]
fn call_js_fn_with_rest_args_and_return() {
    let res: Vec<i8> = test_with(|ctx| {
        let func: Function = ctx
            .eval(
                r#"
              (a, b, ...x) => [a, b, x.length, ...x]
            "#,
            )
            .unwrap();
        func.call((-2, -1, Rest(vec![1, 2]))).unwrap()
    });
    assert_eq!(res.len(), 5);
    assert_eq!(res[0], -2);
    assert_eq!(res[1], -1);
    assert_eq!(res[2], 2);
    assert_eq!(res[3], 1);
    assert_eq!(res[4], 2);
}

#[test]
fn call_js_fn_with_no_args_and_throw() {
    test_with(|ctx| {
        let f: Function = ctx.eval("() => { throw new Error('unimplemented'); }").unwrap();

        if let Err(Error::Exception) = f.call::<_, ()>(()) {
            let exception = Exception::from_js(&ctx, ctx.catch()).unwrap();
            assert_eq!(exception.message().as_deref(), Some("unimplemented"));
        } else {
            panic!("Should throws");
        }
    })
}

#[test]
fn call_js_fn_with_this_and_no_args_and_return() {
    test_with(|ctx| {
        let f: Function = ctx.eval("function f() { return this.val; } f").unwrap();
        let obj = Object::new(ctx).unwrap();
        obj.set("val", 42).unwrap();

        let res: i32 = (This(obj.clone()),).apply(&f).unwrap();
        assert_eq!(res, 42);
        let res: i32 = f.call((This(obj),)).unwrap();
        assert_eq!(res, 42);
    })
}

#[test]
fn call_js_fn_with_this_and_1_arg_and_return() {
    test_with(|ctx| {
        let f: Function = ctx.eval("function f(a) { return this.val * a; } f").unwrap();
        let obj = Object::new(ctx).unwrap();
        obj.set("val", 3).unwrap();

        let res: i32 = (This(obj.clone()), 2).apply(&f).unwrap();
        assert_eq!(res, 6);
        let res: i32 = f.call((This(obj), 3)).unwrap();
        assert_eq!(res, 9);
    })
}

#[test]
fn call_js_fn_with_1_arg_deferred() {
    js_thread(|| {
        let rt = Runtime::new().unwrap();
        let ctx = Context::full(&rt).unwrap();
        // The thread's runtime may have pending bun work from earlier tests;
        // only deferred calls are asserted on.
        ctx.with(|ctx| {
            let g = ctx.globals();
            let f: Function = ctx.eval("(obj) => { obj.called_deferred = true; }").unwrap();
            f.defer((g.clone(),)).unwrap();
            let c: Value = g.get("called_deferred").unwrap();
            assert_eq!(c.type_of(), Type::Undefined);
        });
        assert!(rt.is_job_pending());
        rt.execute_pending_job().unwrap();
        ctx.with(|ctx| {
            let g = ctx.globals();
            let c: Value = g.get("called_deferred").unwrap();
            assert_eq!(c.type_of(), Type::Bool);
        });
    })
}

fn test() {
    println!("test");
}

#[test]
fn static_callback() {
    test_with(|ctx| {
        let f = Function::new(ctx.clone(), test).unwrap();
        f.set_name("test").unwrap();
        let eval: Function = ctx.eval("a => { a() }").unwrap();
        (f.clone(),).apply::<()>(&eval).unwrap();
        f.call::<_, ()>(()).unwrap();

        let name: StdString = f.clone().into_inner().get("name").unwrap();
        assert_eq!(name, "test");

        let get_name: Function = ctx.eval("a => a.name").unwrap();
        let name: StdString = get_name.call((f.clone(),)).unwrap();
        assert_eq!(name, "test");
    })
}

#[test]
fn const_callback() {
    use std::sync::{Arc, Mutex};
    test_with(|ctx| {
        #[allow(clippy::mutex_atomic)]
        let called = Arc::new(Mutex::new(false));
        let called_clone = called.clone();
        let f = Function::new(ctx.clone(), move || {
            (*called_clone.lock().unwrap()) = true;
        })
        .unwrap();
        f.set_name("test").unwrap();

        let eval: Function = ctx.eval("a => { a() }").unwrap();
        eval.call::<_, ()>((f.clone(),)).unwrap();
        f.call::<_, ()>(()).unwrap();
        assert!(*called.lock().unwrap());

        let name: StdString = f.clone().into_inner().get("name").unwrap();
        assert_eq!(name, "test");

        let get_name: Function = ctx.eval("a => a.name").unwrap();
        let name: StdString = get_name.call((f.clone(),)).unwrap();
        assert_eq!(name, "test");
    })
}

#[test]
fn mutable_callback() {
    test_with(|ctx| {
        let mut v = 0;
        let f = Function::new(
            ctx.clone(),
            MutFn::new(move || {
                v += 1;
                v
            }),
        )
        .unwrap();
        f.set_name("test").unwrap();

        let eval: Function = ctx.eval("a => a()").unwrap();
        assert_eq!(eval.call::<_, i32>((f.clone(),)).unwrap(), 1);
        assert_eq!(eval.call::<_, i32>((f.clone(),)).unwrap(), 2);
        assert_eq!(eval.call::<_, i32>((f.clone(),)).unwrap(), 3);

        let name: StdString = f.clone().into_inner().get("name").unwrap();
        assert_eq!(name, "test");

        let get_name: Function = ctx.eval("a => a.name").unwrap();
        let name: StdString = get_name.call((f.clone(),)).unwrap();
        assert_eq!(name, "test");
    })
}

#[test]
#[should_panic(expected = "Error borrowing function: can't borrow a value as it is already borrowed")]
fn recursively_called_mutable_callback() {
    test_with(|ctx| {
        let mut v = 0;
        let f = Function::new(
            ctx.clone(),
            MutFn::new(move |ctx: rbun::Ctx| {
                v += 1;
                ctx.globals()
                    .get::<_, Function>("foo_recursive")
                    .unwrap()
                    .call::<_, ()>(())
                    .catch(&ctx)
                    .unwrap();
                v
            }),
        )
        .unwrap();
        ctx.globals().set("foo_recursive", f.clone()).unwrap();
        f.call::<_, ()>(()).unwrap();
    })
}

#[test]
#[should_panic(expected = "Error borrowing function: tried to use a value, which can only be used once, again.")]
fn repeatedly_called_once_callback() {
    test_with(|ctx| {
        let mut v = 0;
        let f = Function::new(
            ctx.clone(),
            OnceFn::from(move || {
                v += 1;
                v
            }),
        )
        .unwrap();
        ctx.globals().set("foo_once", f.clone()).unwrap();
        f.call::<_, ()>(()).catch(&ctx).unwrap();
        f.call::<_, ()>(()).catch(&ctx).unwrap();
    })
}

#[test]
fn multiple_const_callbacks() {
    test_with(|ctx| {
        let globals = ctx.globals();
        globals.set("one", Func::new(|| 1f64)).unwrap();
        globals.set("neg", Func::new(|a: f64| -a)).unwrap();
        globals.set("add", Func::new(|a: f64, b: f64| a + b)).unwrap();

        let r: f64 = ctx.eval("neg(add(one(), 2))").unwrap();
        assert_approx_eq!(r, -3.0);
    })
}

#[test]
fn mutable_callback_which_can_fail() {
    test_with(|ctx| {
        let globals = ctx.globals();
        let mut id_alloc = 0;
        globals
            .set(
                "new_id",
                Func::from(MutFn::from(move || {
                    id_alloc += 1;
                    if id_alloc < 4 { Ok(id_alloc) } else { Err(Error::Unknown) }
                })),
            )
            .unwrap();

        let id: u32 = ctx.eval("new_id()").unwrap();
        assert_eq!(id, 1);
        let id: u32 = ctx.eval("new_id()").unwrap();
        assert_eq!(id, 2);
        let id: u32 = ctx.eval("new_id()").unwrap();
        assert_eq!(id, 3);
        let _err = ctx.eval::<u32, _>("new_id()").unwrap_err();
    })
}

#[test]
fn mutable_callback_with_ctx_which_reads_globals() {
    test_with(|ctx| {
        let globals = ctx.globals();
        let mut id_alloc = 0;
        globals
            .set(
                "new_id2",
                Func::from(MutFn::from(move |ctx: rbun::Ctx| {
                    let initial: Option<u32> = ctx.globals().get("initial_id")?;
                    if let Some(initial) = initial {
                        id_alloc += 1;
                        Ok(id_alloc + initial)
                    } else {
                        Err(Error::Unknown)
                    }
                })),
            )
            .unwrap();

        let _err = ctx.eval::<u32, _>("new_id2()").unwrap_err();
        globals.set("initial_id", 10).unwrap();

        let id: u32 = ctx.eval("new_id2()").unwrap();
        assert_eq!(id, 11);
        let id: u32 = ctx.eval("new_id2()").unwrap();
        assert_eq!(id, 12);
        let id: u32 = ctx.eval("new_id2()").unwrap();
        assert_eq!(id, 13);
    })
}

#[test]
fn call_rust_fn_with_ctx_and_value() {
    test_with(|ctx| {
        let func = Func::from(|ctx, val| {
            struct Args<'js>(rbun::Ctx<'js>, Value<'js>);
            let Args(ctx, val) = Args(ctx, val);
            ctx.globals().set("test_str", val).unwrap();
        });
        ctx.globals().set("test_fn", func).unwrap();
        ctx.eval::<(), _>(
            r#"
              test_fn("test_str")
            "#,
        )
        .unwrap();
        let val: StdString = ctx.globals().get("test_str").unwrap();
        assert_eq!(val, "test_str");
    });
}

#[test]
fn call_rust_fn_with_this_and_args() {
    let res: f64 = test_with(|ctx| {
        let func = Function::new(ctx.clone(), |this: This<Object>, a: f64, b: f64| {
            let x: f64 = this.get("x").unwrap();
            let y: f64 = this.get("y").unwrap();
            this.set("r", a * x + b * y).unwrap();
        })
        .unwrap();
        ctx.globals().set("test_fn_this", func).unwrap();
        ctx.eval(
            r#"
              let test_obj = { x: 1, y: 2 };
              test_fn_this.call(test_obj, 3, 4);
              test_obj.r
            "#,
        )
        .unwrap()
    });
    assert_eq!(res, 11.0);
}

#[test]
fn apply_rust_fn_with_this_and_args() {
    let res: f32 = test_with(|ctx| {
        let func = Function::new(ctx.clone(), |this: This<Object>, x: f32, y: f32| {
            let a: f32 = this.get("a").unwrap();
            let b: f32 = this.get("b").unwrap();
            a * x + b * y
        })
        .unwrap();
        ctx.globals().set("test_fn_apply", func).unwrap();
        ctx.eval(
            r#"
              let test_obj2 = { a: 1, b: 2 };
              test_fn_apply.apply(test_obj2, [3, 4])
            "#,
        )
        .unwrap()
    });
    assert_eq!(res, 11.0);
}

#[test]
fn bind_rust_fn_with_this_and_call_with_args() {
    let res: f32 = test_with(|ctx| {
        let func = Function::new(ctx.clone(), |this: This<Object>, x: f32, y: f32| {
            let a: f32 = this.get("a").unwrap();
            let b: f32 = this.get("b").unwrap();
            a * x + b * y
        })
        .unwrap();
        ctx.globals().set("test_fn_bind", func).unwrap();
        ctx.eval(
            r#"
              let test_obj3 = { a: 1, b: 2 };
              test_fn_bind.bind(test_obj3)(3, 4)
            "#,
        )
        .unwrap()
    });
    assert_eq!(res, 11.0);
}

#[test]
fn call_rust_fn_with_var_args() {
    let res: Vec<i8> = test_with(|ctx| {
        let func = Function::new(ctx.clone(), |args: Rest<i8>| {
            use std::iter::once;
            once(args.len() as i8).chain(args.iter().cloned()).collect::<Vec<_>>()
        })
        .unwrap();
        ctx.globals().set("test_fn_var", func).unwrap();
        ctx.eval(
            r#"
              test_fn_var(1, 2, 3)
            "#,
        )
        .unwrap()
    });
    assert_eq!(res.len(), 4);
    assert_eq!(res[0], 3);
    assert_eq!(res[1], 1);
    assert_eq!(res[2], 2);
    assert_eq!(res[3], 3);
}

#[test]
fn call_rust_fn_with_rest_args() {
    let res: Vec<i8> = test_with(|ctx| {
        let func = Function::new(ctx.clone(), |arg1: i8, arg2: i8, args: Rest<i8>| {
            use std::iter::once;
            once(arg1)
                .chain(once(arg2))
                .chain(once(args.len() as i8))
                .chain(args.iter().cloned())
                .collect::<Vec<_>>()
        })
        .unwrap();
        ctx.globals().set("test_fn_rest", func).unwrap();
        ctx.eval(
            r#"
              test_fn_rest(-2, -1, 1, 2)
            "#,
        )
        .unwrap()
    });
    assert_eq!(res.len(), 5);
    assert_eq!(res[0], -2);
    assert_eq!(res[1], -1);
    assert_eq!(res[2], 2);
    assert_eq!(res[3], 1);
    assert_eq!(res[4], 2);
}

#[test]
fn js_fn_wrappers() {
    test_with(|ctx| {
        let global = ctx.globals();
        global
            .set("cat", Func::from(|a: StdString, b: StdString| format!("{a}{b}")))
            .unwrap();
        let res: StdString = ctx.eval("cat(\"foo\", \"bar\")").unwrap();
        assert_eq!(res, "foobar");

        let mut log = Vec::<StdString>::new();
        global
            .set(
                "log",
                Func::from(MutFn::from(move |msg: StdString| {
                    log.push(msg);
                    log.len() as u32
                })),
            )
            .unwrap();
        let n: u32 = ctx.eval("log(\"foo\") + log(\"bar\")").unwrap();
        assert_eq!(n, 3);
    });
}

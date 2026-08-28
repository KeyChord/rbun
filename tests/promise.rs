//! Ported from rquickjs-core `src/value/promise.rs`, `src/persistent.rs` and
//! `src/runtime/async.rs` / `src/context/async.rs`.

mod common;

use common::{block_on, js_thread, test_with};
use rbun::context::EvalOptions;
use rbun::prelude::*;
use rbun::promise::Promised;
use rbun::{AsyncContext, AsyncRuntime, CatchResultExt, CaughtError, Context, Exception, Function, Object, Persistent, Promise, PromiseState, Runtime, Value, async_with};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

async fn set_timeout<'js>(cb: Function<'js>, number: f64) -> rbun::Result<()> {
    tokio::time::sleep(Duration::from_secs_f64(number / 1000.0)).await;
    cb.call::<_, ()>(())
}

#[test]
fn promise() {
    block_on(|| async {
        let rt = AsyncRuntime::new().unwrap();
        let ctx = AsyncContext::full(&rt).await.unwrap();

        async_with!(ctx => |ctx| {
            ctx.globals().set("setTimeoutRust", Func::from(Async(set_timeout))).unwrap();

            let func = ctx
                .eval::<Function, _>(
                    r"
                    (function(){
                        return new Promise((resolve) => {
                            setTimeoutRust(x => {
                                resolve(42)
                            },100)
                        })
                    })
                    ",
                )
                .catch(&ctx)
                .unwrap();
            let promise: Promise = func.call(()).unwrap();
            assert_eq!(promise.into_future::<i32>().await.catch(&ctx).unwrap(), 42);

            let func = ctx
                .eval::<Function, _>(
                    r"
                    (function(){
                        return new Promise((_,reject) => {
                            setTimeoutRust(x => {
                                reject(42)
                            },100)
                        })
                    })
                    ",
                )
                .catch(&ctx)
                .unwrap();
            let promise: Promise = func.call(()).unwrap();
            let err = promise.into_future::<()>().await.catch(&ctx);
            match err {
                Err(CaughtError::Value(v)) => {
                    assert_eq!(v.as_int().unwrap(), 42)
                }
                _ => panic!(),
            }
        })
        .await
    })
}

#[test]
fn promised() {
    block_on(|| async {
        let rt = AsyncRuntime::new().unwrap();
        let ctx = AsyncContext::full(&rt).await.unwrap();

        async_with!(ctx => |ctx| {
            let promised = Promised::from(async {
                tokio::time::sleep(Duration::from_millis(100)).await;
                42
            });

            let function = ctx.eval::<Function,_>(r"
                (async function(v){
                    let val = await v;
                    if(val !== 42){
                        throw new Error('not correct value')
                    }
                })
            ").catch(&ctx).unwrap();

            function.call::<_,Promise>((promised,)).unwrap().into_future::<()>().await.unwrap();

            let ctx_clone = ctx.clone();
            let promised = Promised::from(async move {
                tokio::time::sleep(Duration::from_millis(100)).await;
                rbun::Result::<()>::Err(Exception::throw_message(&ctx_clone, "some_message"))
            });

            let function = ctx.eval::<Function,_>(r"
                (async function(v){
                    try{
                        await v;
                    }catch(e) {
                        if (e.message !== 'some_message'){
                            throw new Error('wrong error')
                        }
                        return
                    }
                    throw new Error('no error thrown')
                })
            ")
                .catch(&ctx)
                .unwrap();

            function.call::<_,Promise>((promised,)).unwrap().into_future::<()>().await.unwrap()
        })
        .await
    })
}

#[test]
fn promise_then() {
    static DID_EXECUTE: AtomicBool = AtomicBool::new(false);

    js_thread(|| {
        let rt = Runtime::new().unwrap();
        let ctx = Context::full(&rt).unwrap();

        ctx.with(|ctx| {
            let (promise, resolve, _) = Promise::new(&ctx).unwrap();

            let cb = Func::new(|s: String| {
                assert_eq!(s, "FOO");
                DID_EXECUTE.store(true, Ordering::SeqCst);
            });

            assert_eq!(promise.state(), PromiseState::Pending);

            promise
                .get::<_, Function>("then")
                .catch(&ctx)
                .unwrap()
                .call::<_, ()>((This(promise.clone()), cb))
                .catch(&ctx)
                .unwrap();

            resolve.call::<_, ()>(("FOO",)).unwrap();
            assert_eq!(promise.state(), PromiseState::Resolved);

            while ctx.execute_pending_job() {}
            ctx.drain_microtasks();

            assert!(DID_EXECUTE.load(Ordering::SeqCst));
        })
    })
}

// ─── persistent.rs ───

/// Every runtime on a thread is the same Bun VM, so a persistent can always
/// be restored there; rquickjs' `UnrelatedRuntime` error cannot occur.
#[test]
#[ignore = "one VM per thread: there is no unrelated runtime to restore into"]
#[should_panic(expected = "UnrelatedRuntime")]
fn different_runtime() {
    js_thread(|| {
        let rt1 = Runtime::new().unwrap();
        let ctx = Context::full(&rt1).unwrap();

        let persistent_v = ctx.with(|ctx| {
            let v: Value = ctx.eval("1").unwrap();
            Persistent::save(&ctx, v)
        });

        let rt2 = Runtime::new().unwrap();
        let ctx = Context::full(&rt2).unwrap();
        ctx.with(|ctx| {
            let _ = persistent_v.clone().restore(&ctx).unwrap();
        });
    })
}

#[test]
fn different_context() {
    js_thread(|| {
        let rt1 = Runtime::new().unwrap();
        let ctx1 = Context::full(&rt1).unwrap();
        let ctx2 = Context::full(&rt1).unwrap();

        let persistent_v = ctx1.with(|ctx| {
            let v: Object = ctx.eval("({ a: 1 })").unwrap();
            Persistent::save(&ctx, v)
        });

        std::mem::drop(ctx1);

        ctx2.with(|ctx| {
            let obj: Object = persistent_v.clone().restore(&ctx).unwrap();
            assert_eq!(obj.get::<_, i32>("a").unwrap(), 1);
        });
    })
}

#[test]
fn persistent_function() {
    js_thread(|| {
        let rt = Runtime::new().unwrap();
        let ctx = Context::full(&rt).unwrap();

        let func = ctx.with(|ctx| {
            let func: Function = ctx.eval("a => a + 1").unwrap();
            Persistent::save(&ctx, func)
        });

        let res: i32 = ctx.with(|ctx| {
            let func = func.clone().restore(&ctx).unwrap();
            func.call((2,)).unwrap()
        });
        assert_eq!(res, 3);

        let ctx2 = Context::full(&rt).unwrap();
        let res: i32 = ctx2.with(|ctx| {
            let func = func.restore(&ctx).unwrap();
            func.call((0,)).unwrap()
        });
        assert_eq!(res, 1);
    })
}

#[test]
fn persistent_value() {
    js_thread(|| {
        let rt = Runtime::new().unwrap();
        let ctx = Context::full(&rt).unwrap();

        let persistent_v = ctx.with(|ctx| {
            let v: Value = ctx.eval("1").unwrap();
            Persistent::save(&ctx, v)
        });

        ctx.with(|ctx| {
            let v = persistent_v.clone().restore(&ctx).unwrap();
            ctx.globals().set("v", v).unwrap();
            let eq: Value = ctx.eval("v == 1").unwrap();
            assert!(eq.as_bool().unwrap());
        });
    })
}

// ─── runtime/async.rs ───

async_test_case!(basic => (_rt, ctx) {
    async_with!(&ctx => |ctx|{
        let res: i32 = ctx.eval("1 + 1").unwrap();
        assert_eq!(res,2i32);
    }).await;
});

async_test_case!(sleep_closure => (_rt, ctx) {
    let mut a = 1;
    let a_ref = &mut a;

    async_with!(&ctx => |ctx|{
        tokio::time::sleep(Duration::from_secs_f64(0.01)).await;
        ctx.globals().set("foo","bar").unwrap();
        *a_ref += 1;
    }).await;
    assert_eq!(a,2);
});

async_test_case!(drive => (rt, ctx) {
    tokio::task::spawn_local(rt.drive());

    // Give drive time to start.
    tokio::time::sleep(Duration::from_secs_f64(0.01)).await;

    let number = Arc::new(AtomicUsize::new(0));
    let number_clone = number.clone();

    async_with!(&ctx => |ctx|{
        ctx.spawn(async move {
            tokio::task::yield_now().await;
            number_clone.store(1,Ordering::SeqCst);
        });
    }).await;
    assert_eq!(number.load(Ordering::SeqCst),0);
    // Give drive time to finish the task.
    tokio::time::sleep(Duration::from_secs_f64(0.01)).await;
    assert_eq!(number.load(Ordering::SeqCst),1);
});

async_test_case!(no_drive => (_rt, ctx) {
    let number = Arc::new(AtomicUsize::new(0));
    let number_clone = number.clone();

    async_with!(&ctx => |ctx|{
        ctx.spawn(async move {
            tokio::task::yield_now().await;
            number_clone.store(1,Ordering::SeqCst);
        });
    }).await;
    assert_eq!(number.load(Ordering::SeqCst),0);
    tokio::time::sleep(Duration::from_secs_f64(0.01)).await;
    assert_eq!(number.load(Ordering::SeqCst),0);
});

async_test_case!(idle => (rt, ctx) {
    let number = Arc::new(AtomicUsize::new(0));
    let number_clone = number.clone();

    async_with!(&ctx => |ctx|{
        ctx.spawn(async move {
            tokio::task::yield_now().await;
            number_clone.store(1,Ordering::SeqCst);
        });
    }).await;
    assert_eq!(number.load(Ordering::SeqCst),0);
    rt.idle().await;
    assert_eq!(number.load(Ordering::SeqCst),1);
});

async_test_case!(recursive_spawn => (_rt, ctx) {
    use tokio::sync::oneshot;

    async_with!(&ctx => |ctx|{
        let ctx_clone = ctx.clone();
        let (tx,rx) = oneshot::channel::<()>();
        let (tx2,rx2) = oneshot::channel::<()>();
        ctx.spawn(async move {
            tokio::task::yield_now().await;

            let ctx = ctx_clone.clone();

            ctx_clone.spawn(async move {
                tokio::task::yield_now().await;
                ctx.spawn(async move {
                    tokio::task::yield_now().await;
                    tx2.send(()).unwrap();
                    tokio::task::yield_now().await;
                });
                tokio::task::yield_now().await;
                tx.send(()).unwrap();
            });

            for _ in 0..32{
                ctx_clone.spawn(async move {})
            }
        });
        tokio::time::timeout(Duration::from_millis(500), rx).await.unwrap().unwrap();
        tokio::time::timeout(Duration::from_millis(500), rx2).await.unwrap().unwrap();
    }).await;
});

async_test_case!(recursive_spawn_from_script => (rt, ctx) {
    static COUNT: AtomicUsize = AtomicUsize::new(0);
    static SCRIPT: &str = r#"
    async function main() {
      setTimeoutSpawn(() => {
        inc_count()
        setTimeoutSpawn(async () => {
            inc_count()
        }, 100);
      }, 100);
    }

    main().catch(print);
    "#;

    fn inc_count(){
        COUNT.fetch_add(1,Ordering::Relaxed);
    }

    fn set_timeout_spawn<'js>(ctx: rbun::Ctx<'js>, callback: Function<'js>, millis: usize) -> rbun::Result<()> {
        ctx.spawn(async move {
            tokio::time::sleep(Duration::from_millis(millis as u64)).await;
            callback.call::<_, ()>(()).unwrap();
        });

        Ok(())
    }

    async_with!(ctx => |ctx|{
        let res: rbun::Result<Promise> = (|| {
            let globals = ctx.globals();

            globals.set("inc_count", Func::from(inc_count))?;
            globals.set("setTimeoutSpawn", Func::from(set_timeout_spawn))?;
            let options = EvalOptions{
                promise: true,
                strict: false,
                ..EvalOptions::default()
            };

            ctx.eval_with_options(SCRIPT, options)?
        })();

        match res.catch(&ctx){
            Ok(promise) => {
                if let Err(err) = promise.into_future::<Value>().await.catch(&ctx){
                    eprintln!("{}", err)
                }
            },
            Err(err) => {
                eprintln!("{}", err)
            },
        };
    })
    .await;

    rt.idle().await;

    assert_eq!(COUNT.load(Ordering::Relaxed),2);
});

// ─── context/async.rs ───

#[test]
fn base_async_context() {
    block_on(|| async {
        let rt = AsyncRuntime::new().unwrap();
        let ctx = AsyncContext::builder().build_async(&rt).await.unwrap();
        async_with!(&ctx => |ctx|{
            ctx.globals();
        })
        .await;
    })
}

#[test]
fn async_clone_ctx() {
    block_on(|| async {
        let rt = AsyncRuntime::new().unwrap();
        let ctx = AsyncContext::full(&rt).await.unwrap();

        let ctx_clone = ctx.clone();

        ctx.with(|ctx| {
            let val: i32 = ctx.eval(r#"1+1"#).unwrap();
            assert_eq!(val, 2);
        })
        .await;

        ctx_clone
            .with(|ctx| {
                let val: i32 = ctx.eval(r#"1+1"#).unwrap();
                assert_eq!(val, 2);
            })
            .await;
    })
}

#[test]
fn test_with_helper() {
    let n: i32 = test_with(|ctx| ctx.eval("40 + 2").unwrap());
    assert_eq!(n, 42);
}

//! Ported from rquickjs-core `src/class.rs`, plus `#[rbun::class]` /
//! `#[rbun::methods]` coverage.

mod common;

use common::{js_thread, test_with};
use rbun::class::{JsClass, Readable, Trace, Tracer, Writable};
use rbun::function::This;
use rbun::value::Constructor;
use rbun::{CatchResultExt, Class, Context, FromJs, Function, IntoJs, JsLifetime, Object, Runtime};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Cycles through Rust-held `Class` handles are GC roots in rbun (values are
/// protected, `Trace` is a no-op), so a self-referencing instance is never
/// collected. Kept to document the difference.
#[test]
#[ignore = "rbun protects every Rust-held value: cycles through Class handles are not collected"]
fn trace() {
    pub struct Container<'js> {
        inner: Vec<Class<'js, Container<'js>>>,
        test: Arc<AtomicBool>,
    }

    impl<'js> Drop for Container<'js> {
        fn drop(&mut self) {
            self.test.store(true, Ordering::SeqCst);
        }
    }

    impl<'js> Trace<'js> for Container<'js> {
        fn trace<'a>(&self, tracer: Tracer<'a, 'js>) {
            self.inner.iter().for_each(|x| x.trace(tracer))
        }
    }

    unsafe impl<'js> JsLifetime<'js> for Container<'js> {
        type Changed<'to> = Container<'to>;
    }

    impl<'js> JsClass<'js> for Container<'js> {
        const NAME: &'static str = "Container";
        type Mutable = Writable;

        fn prototype(ctx: &rbun::Ctx<'js>) -> rbun::Result<Option<rbun::Object<'js>>> {
            Ok(Some(Object::new(ctx.clone())?))
        }

        fn constructor(_ctx: &rbun::Ctx<'js>) -> rbun::Result<Option<Constructor<'js>>> {
            Ok(None)
        }
    }

    js_thread(|| {
        let rt = Runtime::new().unwrap();
        let ctx = Context::full(&rt).unwrap();

        let drop_test = Arc::new(AtomicBool::new(false));

        ctx.with(|ctx| {
            let cls = Class::instance(ctx.clone(), Container { inner: Vec::new(), test: drop_test.clone() }).unwrap();
            assert!(cls.instance_of::<Container>());
            let cls_clone = cls.clone();
            cls.borrow_mut().inner.push(cls_clone);
        });
        rt.run_gc();
        assert!(drop_test.load(Ordering::SeqCst));
    })
}

/// Instances without cycles are finalized by JSC's GC.
#[test]
fn finalize() {
    pub struct Droppable(Arc<AtomicBool>);

    impl Drop for Droppable {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }
    impl<'js> Trace<'js> for Droppable {
        fn trace<'a>(&self, _tracer: Tracer<'a, 'js>) {}
    }
    unsafe impl<'js> JsLifetime<'js> for Droppable {
        type Changed<'to> = Droppable;
    }
    impl<'js> JsClass<'js> for Droppable {
        const NAME: &'static str = "Droppable";
        type Mutable = Readable;
    }

    js_thread(|| {
        let rt = Runtime::new().unwrap();
        let ctx = Context::full(&rt).unwrap();
        let dropped = Arc::new(AtomicBool::new(false));
        ctx.with(|ctx| {
            let cls = Class::instance(ctx.clone(), Droppable(dropped.clone())).unwrap();
            assert!(cls.instance_of::<Droppable>());
            drop(cls);
        });
        for _ in 0..10 {
            rt.run_gc();
            if dropped.load(Ordering::SeqCst) {
                break;
            }
        }
        assert!(dropped.load(Ordering::SeqCst));
    })
}

#[derive(Clone, Copy)]
pub struct Vec3 {
    x: f32,
    y: f32,
    z: f32,
}

impl Vec3 {
    pub fn new(x: f32, y: f32, z: f32) -> Self {
        Vec3 { x, y, z }
    }

    pub fn add(self, v: Vec3) -> Self {
        Vec3 { x: self.x + v.x, y: self.y + v.y, z: self.z + v.z }
    }
}

impl<'js> Trace<'js> for Vec3 {
    fn trace<'a>(&self, _tracer: Tracer<'a, 'js>) {}
}

impl<'js> FromJs<'js> for Vec3 {
    fn from_js(ctx: &rbun::Ctx<'js>, value: rbun::Value<'js>) -> rbun::Result<Self> {
        Ok(*Class::<Vec3>::from_js(ctx, value)?.try_borrow()?)
    }
}

impl<'js> IntoJs<'js> for Vec3 {
    fn into_js(self, ctx: &rbun::Ctx<'js>) -> rbun::Result<rbun::Value<'js>> {
        Class::instance(ctx.clone(), self).into_js(ctx)
    }
}

unsafe impl<'js> JsLifetime<'js> for Vec3 {
    type Changed<'to> = Vec3;
}

impl<'js> JsClass<'js> for Vec3 {
    const NAME: &'static str = "Vec3";
    type Mutable = Writable;

    fn prototype(ctx: &rbun::Ctx<'js>) -> rbun::Result<Option<rbun::Object<'js>>> {
        let proto = Object::new(ctx.clone())?;
        let func = Function::new(ctx.clone(), |this: This<Vec3>, other: Vec3| this.add(other))?.with_name("add")?;
        proto.set("add", func)?;
        Ok(Some(proto))
    }

    fn constructor(ctx: &rbun::Ctx<'js>) -> rbun::Result<Option<Constructor<'js>>> {
        let constr = Constructor::new_class::<Vec3, _, _>(ctx.clone(), |x: f32, y: f32, z: f32| Vec3::new(x, y, z))?;
        Ok(Some(constr))
    }
}

#[test]
fn constructor() {
    test_with(|ctx| {
        Class::<Vec3>::define(&ctx.globals()).unwrap();

        let v = ctx
            .eval::<Vec3, _>(
                r"
            let a = new Vec3(1,2,3);
            let b = new Vec3(4,2,8);
            a.add(b)
        ",
            )
            .catch(&ctx)
            .unwrap();

        approx::assert_abs_diff_eq!(v.x, 5.0);
        approx::assert_abs_diff_eq!(v.y, 4.0);
        approx::assert_abs_diff_eq!(v.z, 11.0);

        let name: String = ctx.eval("new Vec3(1,2,3).constructor.name").unwrap();
        assert_eq!(name, Vec3::NAME);
    })
}

#[test]
fn extend_class() {
    test_with(|ctx| {
        Class::<Vec3>::define(&ctx.globals()).unwrap();

        let v = ctx
            .eval::<Vec3, _>(
                r"
                class Vec4 extends Vec3 {
                    w = 0;
                    constructor(x,y,z,w){
                        super(x,y,z);
                        this.w
                    }
                }

                new Vec4(1,2,3,4);
            ",
            )
            .catch(&ctx)
            .unwrap();

        approx::assert_abs_diff_eq!(v.x, 1.0);
        approx::assert_abs_diff_eq!(v.y, 2.0);
        approx::assert_abs_diff_eq!(v.z, 3.0);
    })
}

#[test]
fn get_prototype() {
    pub struct X;

    impl<'js> Trace<'js> for X {
        fn trace<'a>(&self, _tracer: Tracer<'a, 'js>) {}
    }

    unsafe impl<'js> JsLifetime<'js> for X {
        type Changed<'to> = X;
    }

    impl<'js> JsClass<'js> for X {
        const NAME: &'static str = "X";
        type Mutable = Readable;

        fn prototype(ctx: &rbun::Ctx<'js>) -> rbun::Result<Option<Object<'js>>> {
            let object = Object::new(ctx.clone())?;
            object.set("foo", "bar")?;
            Ok(Some(object))
        }

        fn constructor(_ctx: &rbun::Ctx<'js>) -> rbun::Result<Option<Constructor<'js>>> {
            Ok(None)
        }
    }

    test_with(|ctx| {
        let proto = Class::<X>::prototype(&ctx).unwrap().unwrap();
        assert_eq!(proto.get::<_, String>("foo").unwrap(), "bar")
    })
}

#[test]
fn generic_types() {
    pub struct DebugPrinter<D: std::fmt::Debug> {
        d: D,
    }

    impl<'js, D: std::fmt::Debug> Trace<'js> for DebugPrinter<D> {
        fn trace<'a>(&self, _tracer: Tracer<'a, 'js>) {}
    }

    unsafe impl<'js, D: std::fmt::Debug + 'static> JsLifetime<'js> for DebugPrinter<D> {
        type Changed<'to> = DebugPrinter<D>;
    }

    impl<'js, D: std::fmt::Debug + 'static> JsClass<'js> for DebugPrinter<D> {
        const NAME: &'static str = "DebugPrinter";
        type Mutable = Readable;

        fn prototype(ctx: &rbun::Ctx<'js>) -> rbun::Result<Option<Object<'js>>> {
            let object = Object::new(ctx.clone())?;
            object.set(
                "to_debug_string",
                Function::new(ctx.clone(), |this: This<Class<DebugPrinter<D>>>| -> rbun::Result<String> {
                    Ok(format!("{:?}", &this.0.borrow().d))
                }),
            )?;
            Ok(Some(object))
        }

        fn constructor(_ctx: &rbun::Ctx<'js>) -> rbun::Result<Option<Constructor<'js>>> {
            Ok(None)
        }
    }

    test_with(|ctx| {
        let a = Class::instance(ctx.clone(), DebugPrinter { d: 42usize });
        let b = Class::instance(ctx.clone(), DebugPrinter { d: "foo".to_string() });

        // Tests share one global scope (`let a` from `constructor` would
        // shadow a global property named `a`), so use distinct names.
        ctx.globals().set("dp_a", a).unwrap();
        ctx.globals().set("dp_b", b).unwrap();

        assert_eq!(ctx.eval::<String, _>(r#" dp_a.to_debug_string() "#).catch(&ctx).unwrap(), "42");
        assert_eq!(ctx.eval::<String, _>(r#" dp_b.to_debug_string() "#).catch(&ctx).unwrap(), "\"foo\"");

        if ctx.globals().get::<_, Class<DebugPrinter<String>>>("dp_a").is_ok() {
            panic!("Conversion should fail")
        }
        if ctx.globals().get::<_, Class<DebugPrinter<usize>>>("dp_b").is_ok() {
            panic!("Conversion should fail")
        }

        ctx.globals().get::<_, Class<DebugPrinter<usize>>>("dp_a").unwrap();
        ctx.globals().get::<_, Class<DebugPrinter<String>>>("dp_b").unwrap();
    })
}

// ─── #[rbun::class] / #[rbun::methods] ───

#[rbun::class]
#[derive(rbun::derive::Trace, rbun::derive::JsLifetime)]
struct Counter {
    n: i64,
}

#[rbun::methods(rename_all = "camelCase")]
impl Counter {
    #[qjs(constructor)]
    fn new<'js>(_ctx: rbun::Ctx<'js>, args: rbun::Rest<rbun::Value<'js>>) -> rbun::Result<Self> {
        Ok(Counter { n: args.0.first().and_then(|a| a.as_number()).unwrap_or(0.0) as i64 })
    }

    fn inc<'js>(&mut self, _ctx: rbun::Ctx<'js>) -> rbun::Result<i64> {
        self.n += 1;
        Ok(self.n)
    }

    fn add_many(&mut self, values: Vec<i64>) -> i64 {
        self.n += values.iter().sum::<i64>();
        self.n
    }

    #[qjs(get)]
    fn value(&self) -> i64 {
        self.n
    }

    #[qjs(static)]
    fn zero() -> i64 {
        0
    }
}

#[test]
fn macro_class() {
    test_with(|ctx| {
        Class::<Counter>::define(&ctx.globals()).unwrap();
        let v: String = ctx
            .eval("const c = new Counter(40); c.inc(); [c.addMany([1, 1]), c.value, c instanceof Counter, typeof Counter, Counter.zero(), c.constructor.name].join(' ')")
            .catch(&ctx)
            .unwrap();
        assert_eq!(v, "43 43 true function 0 Counter");

        let instance = Class::instance(ctx.clone(), Counter { n: 7 }).unwrap();
        assert_eq!(instance.borrow().n, 7);
        instance.borrow_mut().n = 8;
        ctx.globals().set("c2", instance).unwrap();
        let v: i64 = ctx.eval("c2.inc()").catch(&ctx).unwrap();
        assert_eq!(v, 9);
        let c2: Class<Counter> = ctx.globals().get("c2").unwrap();
        assert_eq!(c2.borrow().n, 9);
    })
}

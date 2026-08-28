//! rquickjs vs rbun on identical workloads, through the same API shapes.
//!
//!   cargo bench                       # everything
//!   cargo bench -- fib                # one group
//!
//! Every benchmark body is written twice, once per crate, with the same
//! JavaScript and the same Rust-side calls; the only differences are the
//! crate paths. rquickjs gets a fresh `Runtime` + `Context` per benchmark,
//! rbun uses the thread's single Bun VM (it cannot be torn down), so
//! "runtime_create" is measured for rquickjs only and reported as an absolute
//! one-time cost for rbun.

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use std::hint::black_box;
use std::time::{Duration, Instant};

// ─── Workloads (shared JavaScript) ───────────────────────────────────────

const FIB: &str = r#"
function fib(n) { return n < 2 ? n : fib(n - 1) + fib(n - 2); }
fib(22)
"#;

const SORT: &str = r#"
let seed = 42;
const arr = Array.from({ length: 20000 }, () => (seed = (seed * 1103515245 + 12345) % 2147483648));
arr.sort((a, b) => a - b);
arr[0] + arr[arr.length - 1]
"#;

const STRINGS: &str = r#"
let s = "";
for (let i = 0; i < 2000; i++) s += i.toString(36) + ",";
s.split(",").map(x => x.toUpperCase()).join("|").length
"#;

const OBJECTS: &str = r#"
const objs = [];
for (let i = 0; i < 5000; i++) objs.push({ id: i, name: "item" + i, tags: [i, i * 2], nested: { ok: i % 2 === 0 } });
objs.filter(o => o.nested.ok).map(o => o.tags[1]).reduce((a, b) => a + b, 0)
"#;

const CALL_HOST_LOOP: &str = "let acc = 0; for (let i = 0; i < 1000; i++) acc += host(i); acc";

const JSON_DOC: &str = r#"{"users":[{"id":1,"name":"a","tags":["x","y"],"nested":{"k":1.5,"ok":true}},{"id":2,"name":"b","tags":[],"nested":{"k":-2,"ok":false}},{"id":3,"name":"c","tags":["z"],"nested":{"k":0,"ok":true}}],"count":3,"meta":{"page":1,"total":10,"note":"lorem ipsum dolor sit amet"}}"#;

const MODULE_SRC: &str = r#"
export const answer = 21 * 2;
export function greet(name) { return "hello " + name; }
"#;

// ─── rquickjs side ───────────────────────────────────────────────────────

mod qjs {
    use rquickjs::{Context, Function, Module, Object, Runtime};

    pub struct Engine {
        pub rt: Runtime,
        pub ctx: Context,
    }

    pub fn engine() -> Engine {
        let rt = Runtime::new().unwrap();
        let ctx = Context::full(&rt).unwrap();
        Engine { rt, ctx }
    }

    pub fn eval_i32(engine: &Engine, src: &str) -> i32 {
        engine.ctx.with(|ctx| ctx.eval::<i32, _>(src).unwrap())
    }

    pub fn eval_f64(engine: &Engine, src: &str) -> f64 {
        engine.ctx.with(|ctx| ctx.eval::<f64, _>(src).unwrap())
    }

    pub fn call_js_function(engine: &Engine, iterations: u32) -> i32 {
        engine.ctx.with(|ctx| {
            let f: Function = ctx.eval("(a) => a + 1").unwrap();
            let mut acc = 0i32;
            for i in 0..iterations as i32 {
                acc += f.call::<_, i32>((i,)).unwrap();
            }
            acc
        })
    }

    pub fn call_host_function(engine: &Engine, src: &str) -> i32 {
        engine.ctx.with(|ctx| {
            ctx.globals()
                .set("host", rquickjs::Function::new(ctx.clone(), |a: i32| a + 1).unwrap())
                .unwrap();
            ctx.eval::<i32, _>(src).unwrap()
        })
    }

    pub fn object_properties(engine: &Engine, iterations: u32) -> i32 {
        engine.ctx.with(|ctx| {
            let obj = Object::new(ctx.clone()).unwrap();
            let mut acc = 0i32;
            for i in 0..iterations as i32 {
                obj.set("x", i).unwrap();
                obj.set("y", "text").unwrap();
                acc += obj.get::<_, i32>("x").unwrap();
                let _: String = obj.get("y").unwrap();
            }
            acc
        })
    }

    pub fn json_roundtrip(engine: &Engine, doc: &str) -> usize {
        engine.ctx.with(|ctx| {
            let value = ctx.json_parse(doc).unwrap();
            let out: rquickjs::String = ctx.json_stringify(value).unwrap().unwrap();
            out.to_string().unwrap().len()
        })
    }

    pub fn module_evaluate(engine: &Engine, name: &str, src: &str) -> i32 {
        engine.ctx.with(|ctx| {
            let (module, promise) = Module::declare(ctx.clone(), name, src).unwrap().eval().unwrap();
            promise.finish::<()>().unwrap();
            module.get::<_, i32>("answer").unwrap()
        })
    }

    pub fn promise_roundtrip(engine: &Engine, iterations: u32) -> i32 {
        engine.ctx.with(|ctx| {
            let f: Function = ctx.eval("async (a) => a + 1").unwrap();
            let mut acc = 0i32;
            for i in 0..iterations as i32 {
                let promise: rquickjs::Promise = f.call((i,)).unwrap();
                acc += promise.finish::<i32>().unwrap();
            }
            acc
        })
    }
}

// ─── rbun side ───────────────────────────────────────────────────────────

mod bun {
    use rbun::{Context, Function, Module, Object, Runtime};

    pub struct Engine {
        pub rt: Runtime,
        pub ctx: Context,
    }

    pub fn engine() -> Engine {
        let rt = Runtime::new().unwrap();
        let ctx = Context::full(&rt).unwrap();
        Engine { rt, ctx }
    }

    pub fn eval_i32(engine: &Engine, src: &str) -> i32 {
        engine.ctx.with(|ctx| ctx.eval::<i32, _>(src).unwrap())
    }

    pub fn eval_f64(engine: &Engine, src: &str) -> f64 {
        engine.ctx.with(|ctx| ctx.eval::<f64, _>(src).unwrap())
    }

    pub fn call_js_function(engine: &Engine, iterations: u32) -> i32 {
        engine.ctx.with(|ctx| {
            let f: Function = ctx.eval("(a) => a + 1").unwrap();
            let mut acc = 0i32;
            for i in 0..iterations as i32 {
                acc += f.call::<_, i32>((i,)).unwrap();
            }
            acc
        })
    }

    pub fn call_host_function(engine: &Engine, src: &str) -> i32 {
        engine.ctx.with(|ctx| {
            ctx.globals()
                .set("host", rbun::Function::new(ctx.clone(), |a: i32| a + 1).unwrap())
                .unwrap();
            ctx.eval::<i32, _>(src).unwrap()
        })
    }

    pub fn object_properties(engine: &Engine, iterations: u32) -> i32 {
        engine.ctx.with(|ctx| {
            let obj = Object::new(ctx.clone()).unwrap();
            let mut acc = 0i32;
            for i in 0..iterations as i32 {
                obj.set("x", i).unwrap();
                obj.set("y", "text").unwrap();
                acc += obj.get::<_, i32>("x").unwrap();
                let _: String = obj.get("y").unwrap();
            }
            acc
        })
    }

    pub fn json_roundtrip(engine: &Engine, doc: &str) -> usize {
        engine.ctx.with(|ctx| {
            let value = ctx.json_parse(doc).unwrap();
            let out: rbun::String = ctx.json_stringify(value).unwrap().unwrap();
            out.to_string().unwrap().len()
        })
    }

    pub fn module_evaluate(engine: &Engine, name: &str, src: &str) -> i32 {
        engine.ctx.with(|ctx| {
            let (module, promise) = Module::declare(ctx.clone(), name, src).unwrap().eval().unwrap();
            promise.finish::<()>().unwrap();
            module.get::<_, i32>("answer").unwrap()
        })
    }

    pub fn promise_roundtrip(engine: &Engine, iterations: u32) -> i32 {
        engine.ctx.with(|ctx| {
            let f: Function = ctx.eval("async (a) => a + 1").unwrap();
            let mut acc = 0i32;
            for i in 0..iterations as i32 {
                let promise: rbun::Promise = f.call((i,)).unwrap();
                acc += promise.finish::<i32>().unwrap();
            }
            acc
        })
    }
}

// ─── Benchmarks ──────────────────────────────────────────────────────────

fn runtime_create(c: &mut Criterion) {
    let mut group = c.benchmark_group("runtime_create");
    group.bench_function("rquickjs", |b| {
        b.iter(|| {
            let engine = qjs::engine();
            black_box(qjs::eval_i32(&engine, "1"))
        })
    });
    // Bun boots exactly once per thread; report that one-time cost.
    let start = Instant::now();
    let engine = bun::engine();
    let boot = start.elapsed();
    black_box(bun::eval_i32(&engine, "1"));
    println!("rbun: one-time VM boot on this thread took {boot:?} (not repeatable, see README)");
    group.finish();
}

fn eval_expression(c: &mut Criterion) {
    let mut group = c.benchmark_group("eval_expression");
    let q = qjs::engine();
    let r = bun::engine();
    group.bench_function("rquickjs", |b| b.iter(|| black_box(qjs::eval_i32(&q, "1 + 1"))));
    group.bench_function("rbun", |b| b.iter(|| black_box(bun::eval_i32(&r, "1 + 1"))));
    group.finish();
}

fn call_js_function(c: &mut Criterion) {
    let mut group = c.benchmark_group("call_js_function");
    let iterations = 1000u32;
    group.throughput(Throughput::Elements(iterations as u64));
    let q = qjs::engine();
    let r = bun::engine();
    group.bench_function(BenchmarkId::new("rquickjs", iterations), |b| b.iter(|| black_box(qjs::call_js_function(&q, iterations))));
    group.bench_function(BenchmarkId::new("rbun", iterations), |b| b.iter(|| black_box(bun::call_js_function(&r, iterations))));
    group.finish();
}

fn call_host_function(c: &mut Criterion) {
    let mut group = c.benchmark_group("call_host_function");
    group.throughput(Throughput::Elements(1000));
    let q = qjs::engine();
    let r = bun::engine();
    group.bench_function("rquickjs", |b| b.iter(|| black_box(qjs::call_host_function(&q, CALL_HOST_LOOP))));
    group.bench_function("rbun", |b| b.iter(|| black_box(bun::call_host_function(&r, CALL_HOST_LOOP))));
    group.finish();
}

fn object_properties(c: &mut Criterion) {
    let mut group = c.benchmark_group("object_properties");
    let iterations = 1000u32;
    group.throughput(Throughput::Elements(iterations as u64 * 4));
    let q = qjs::engine();
    let r = bun::engine();
    group.bench_function("rquickjs", |b| b.iter(|| black_box(qjs::object_properties(&q, iterations))));
    group.bench_function("rbun", |b| b.iter(|| black_box(bun::object_properties(&r, iterations))));
    group.finish();
}

fn json_roundtrip(c: &mut Criterion) {
    let mut group = c.benchmark_group("json_roundtrip");
    group.throughput(Throughput::Bytes(JSON_DOC.len() as u64));
    let q = qjs::engine();
    let r = bun::engine();
    group.bench_function("rquickjs", |b| b.iter(|| black_box(qjs::json_roundtrip(&q, JSON_DOC))));
    group.bench_function("rbun", |b| b.iter(|| black_box(bun::json_roundtrip(&r, JSON_DOC))));
    group.finish();
}

fn scripts(c: &mut Criterion) {
    let q = qjs::engine();
    let r = bun::engine();
    for (name, src) in [("fib_22", FIB), ("sort_20k", SORT), ("strings", STRINGS), ("objects", OBJECTS)] {
        let mut group = c.benchmark_group(format!("script_{name}"));
        group.bench_function("rquickjs", |b| b.iter(|| black_box(qjs::eval_f64(&q, src))));
        group.bench_function("rbun", |b| b.iter(|| black_box(bun::eval_f64(&r, src))));
        group.finish();
    }
}

fn module_evaluate(c: &mut Criterion) {
    let mut group = c.benchmark_group("module_evaluate");
    let q = qjs::engine();
    let r = bun::engine();
    let mut n = 0u64;
    group.bench_function("rquickjs", |b| {
        b.iter(|| {
            n += 1;
            black_box(qjs::module_evaluate(&q, &format!("bench_mod_{n}"), MODULE_SRC))
        })
    });
    let mut n = 0u64;
    group.bench_function("rbun", |b| {
        b.iter(|| {
            n += 1;
            black_box(bun::module_evaluate(&r, &format!("bench_mod_{n}"), MODULE_SRC))
        })
    });
    group.finish();
}

fn promise_roundtrip(c: &mut Criterion) {
    let mut group = c.benchmark_group("promise_roundtrip");
    let iterations = 200u32;
    group.throughput(Throughput::Elements(iterations as u64));
    let q = qjs::engine();
    let r = bun::engine();
    group.bench_function("rquickjs", |b| b.iter(|| black_box(qjs::promise_roundtrip(&q, iterations))));
    group.bench_function("rbun", |b| b.iter(|| black_box(bun::promise_roundtrip(&r, iterations))));
    group.finish();
}

criterion_group! {
    name = benches;
    config = Criterion::default().sample_size(30).measurement_time(Duration::from_secs(3)).warm_up_time(Duration::from_secs(1));
    targets = runtime_create, eval_expression, call_js_function, call_host_function, object_properties, json_roundtrip, scripts, module_evaluate, promise_roundtrip
}
criterion_main!(benches);

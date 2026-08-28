# rbun

Embed [Bun](https://bun.com)'s JavaScript runtime in Rust with an API modelled
on [rquickjs](https://docs.rs/rquickjs): `Runtime` / `Context` / `Ctx`,
`Value` / `Object` / `Function` / `Promise` / `Array` / `String`,
`Func` / `Async` / `MutFn` / `This` / `Rest`, `Class` + `#[rbun::class]` /
`#[rbun::methods]`, `Module` + `ModuleDef` + `Resolver` / `Loader`,
`Persistent`, `async_with!`, `Promise::into_future`, `serde`, …

The engine is JavaScriptCore with Bun's event loop, module loader (TypeScript,
JSX, `node:*`, `bun:*`, `node_modules`), and Bun / Node runtime APIs. Values
are handled through JavaScriptCore's public C API; the runtime is Bun's own
Rust code (Bun ≥ 1.4 is written in Rust) linked as `libbun_embed.dylib`.

```rust
use rbun::prelude::*;
use rbun::{AsyncContext, AsyncRuntime, Module, async_with};

#[tokio::main(flavor = "current_thread")]
async fn main() -> rbun::Result<()> {
    let rt = AsyncRuntime::new()?;
    let ctx = AsyncContext::full(&rt).await?;
    async_with!(ctx => |ctx| {
        ctx.globals().set("add", Func::from(|a: f64, b: f64| a + b))?;
        let os = Module::import(&ctx, "node:os")?.into_future::<Object>().await?;
        let platform: String = os.get::<_, Function>("platform")?.call(())?;
        let sum: f64 = ctx.eval("add(1, 2)")?;
        println!("{platform} {sum}");
        Ok::<_, rbun::Error>(())
    })
    .await
}
```

## Building

rbun links `vendor/bun/build/release/libbun_embed.dylib`, built from the
vendored Bun checkout. The checkout is upstream plus a small set of patches
maintained as a JSSG codemod in [`vendor-patches/`](vendor-patches/README.md)
(`bun.gen.patch` there is the reviewable diff):

```sh
# macOS prerequisites
brew install llvm@21 automake ccache cmake coreutils gnu-sed go icu4c libiconv libtool ninja pkg-config ruby
curl -fsSL https://bun.com/install | bash          # a release bun drives bun's build
scripts/build-bun.sh                                # ~20 min cold; installs the pinned nightly via rustup
cargo test                                           # the ported rquickjs test suite
```

`RBUN_BUN_LIB_DIR` overrides where the dylib is looked up. Binaries that link
rbun need an rpath to it (rbun's own examples/tests get one; a dependent
crate's `build.rs` can read `DEP_BUN_EMBED_LIB_DIR`).

### Upgrading Bun

```sh
bun vendor-patches/generate.ts upgrade <sha|tag|branch>   # re-vendor + re-apply the patches
scripts/build-bun.sh && cargo test
```

If an upstream change moved one of the patch anchors the codemod fails loudly
(`JSSG patch anchor drifted`); fix `vendor-patches/codemods/bun/codemod.ts`
and its fixtures, then re-run `bun vendor-patches/generate.ts apply`. See
[`vendor-patches/README.md`](vendor-patches/README.md).

## Compatibility with rquickjs

The test suite in `tests/` ports the applicable public-API tests from
`rquickjs-core` 0.11.0. Tests behind rquickjs-only optional features, such as
its custom allocator and `parallel` modes, are outside the port's scope.

### Five rquickjs tests not ported

Exactly five tests in the covered source modules are intentionally absent:

| rquickjs source test | Why it is not ported |
| --- | --- |
| `value/proxy.rs::test::from_javascript` | It requires rquickjs's `Proxy` wrapper and QuickJS's non-standard `JS_GetProxyTarget` / `JS_GetProxyHandler` introspection. JavaScriptCore's public API has no equivalent. JavaScript-created proxies still work as ordinary rbun `Object`s. |
| `value/proxy.rs::test::from_rust` | It constructs a proxy from a Rust `ProxyHandler` whose traps are Rust closures. rbun does not provide that QuickJS-specific Rust proxy facade; create a native JavaScript `Proxy` instead. |
| `value/proxy.rs::test::class_proxy` | This is the same unsupported `ProxyHandler` bridge with a Rust-backed `Class` as its target. Rust-backed classes themselves are supported. |
| `value/string.rs::test::from_javascript_c` | It converts JS directly into rquickjs's engine-owned `CString`, which wraps `JS_ToCStringLen` / `JS_FreeCString`. JavaScriptCore exposes strings differently; use `rbun::String::to_string()` for JS-to-Rust conversion. |
| `value/string.rs::test::to_javascript_c` | It converts through that same rquickjs `CString` handle. rbun instead supports Rust `&CStr` / `std::ffi::CString` as `IntoJs` inputs. |

These are omitted tests, not hidden failures. Four other upstream-derived
tests remain in the suite with `#[ignore]` so their engine-level differences
stay visible: class-cycle tracing, restoring a `Persistent` into an unrelated
runtime, nested synchronous module evaluation, and the QuickJS host promise
rejection tracker.

## Differences and intentionally unsupported features

rbun models the rquickjs API where the two engines have compatible concepts;
it is not a drop-in replacement for every rquickjs or Bun executable feature.

### Compared with rquickjs

- **One VM per thread, one realm.** Bun boots once per thread and never tears
  the VM down. Every `Runtime::new()` on a thread returns a handle to that VM
  and every `Context` refers to the same global object, so state (globals,
  declared modules, user data) is shared. Run JS on a dedicated thread with a
  large stack (16 MB works well) and send work to it, as `tests/common` does.
- **`Persistent::restore` never fails with `UnrelatedRuntime`** (there is no
  unrelated runtime).
- **GC:** every Rust-held value is protected for its lifetime, so values can
  live anywhere (boxed futures, thread-locals). `Trace` is a no-op; a cycle
  through Rust-held `Class` handles is never collected.
- **Modules:** `Module::declare` registers source with Bun's loader (Bun
  transpiles TS/JSX and declared modules may import anything Bun can);
  `declare_def` is evaluated lazily on first import, like rquickjs; when a
  `Resolver` declines a specifier rbun falls back to Bun's own resolution
  instead of failing; the module namespace is only available after
  evaluation; nested synchronous evaluation from inside a host call during
  module evaluation is not supported.
- **Async:** `AsyncContext::async_with` drives Bun's loop and host futures
  (`ctx.spawn`, `Async` host functions, `Promised`) while the block is
  pending. `AsyncRuntime::drive()` only moves host futures; use `idle()` /
  `async_with` to run Bun's timers and I/O.
- **Errors:** exception messages are JavaScriptCore's / Bun's, not QuickJS'.
  `Error::Exception` + `Ctx::catch` work like rquickjs.
- **Eval:** `ctx.eval` is strict by default (like rquickjs); `EvalOptions {
  promise: true }` evaluates the source as an async module.
- **Intentionally unsupported rquickjs APIs:** `Proxy` / `ProxyHandler` and
  rquickjs's engine-owned `CString`; these account for the five omitted tests
  above. JavaScript's native `Proxy` and Rust standard-library C-string inputs
  remain available.
- **Compatibility no-ops:** `Context::base`, `Context::custom`, and
  `ContextBuilder` all return Bun's full global realm; intrinsic selections
  are ignored. `Runtime::set_memory_limit`, `set_gc_threshold`, and
  `set_max_stack_size` are accepted but ignored because JavaScriptCore owns
  heap/GC policy and sizes its stack from the host thread. Spawn the JS thread
  with the stack size you need.
- **Other accepted-but-ignored options:** `EvalOptions::global` and
  `backtrace_barrier`, `json_parse_ext`'s extension flag,
  `json_stringify_replacer`'s replacer, `json_stringify_replacer_space`'s
  replacer/space, and private-key filtering. Use JavaScript's `JSON` methods
  directly when replacer, reviver, or spacing behavior is required.
- **Promise rejection tracking:** `set_host_promise_rejection_tracker` stores
  the callback but Bun does not invoke it for engine-level unhandled
  rejections; use `process.on("unhandledRejection")`.

### Compared with the Bun executable

- **Runtime embedding, not the CLI.** rbun boots Bun's VM, APIs, transpiler,
  module loader, and event loop inside the host process. It does not expose
  Bun's command-line workflows (`bun install`, `bun run`, `bun test`, or CLI
  `bun build`), CLI argument parsing, or bunfig-driven startup configuration.
  Invoke the Bun executable separately for those workflows.
- **Host-driven lifetime and event loop.** A VM is process-lifetime and bound
  to its creating thread. Unlike the Bun executable's automatic
  run-to-completion loop, the Rust host drives work with `async_with!`,
  `AsyncRuntime::idle`, `Runtime::idle`, or explicit ticks.
- **Platform support.** The current shared-library build/link workflow is
  macOS-only (`libbun_embed.dylib`). Bun itself supports more platforms, but
  rbun's embedding link script has not yet been ported to them.

## Benchmarks

`cargo bench` runs `benches/compare.rs`, a criterion suite that drives
rquickjs 0.11 and rbun through the same workloads (each engine on its own
thread; rbun's one-time VM boot is printed once and excluded). Numbers below
are from an Apple Silicon Mac, `--profile=release` Bun, 30 samples × 3 s; the
last column is rbun's time relative to rquickjs (lower is better).

| Benchmark | What it measures | rquickjs | rbun | rbun / rquickjs |
| --- | --- | ---: | ---: | ---: |
| `runtime_create` | `Runtime::new` + `Context::full` | 128 µs | — (one-time ≈3 ms boot, then a no-op) | n/a |
| `eval_expression` | `ctx.eval::<i32>("1 + 1")` | 1.86 µs | 1.02 µs | 0.55× |
| `call_js_function/1000` | 1000 × `Function::call((i,))` from Rust | 59 µs | 118 µs | 2.0× |
| `call_host_function` | JS loop calling a `Func` 1000 times | 73 µs | 98 µs | 1.3× |
| `object_properties` | 1000 × (`set` ×2 + `get` ×2) on one object | 138 µs | 735 µs | 5.3× |
| `json_roundtrip` | `json_parse` + `json_stringify` of a small doc | 14.9 µs | 3.6 µs | 0.24× |
| `script_fib_22` | recursive `fib(22)` | 1.58 ms | 0.35 ms | 0.22× |
| `script_sort_20k` | `Array.prototype.sort` of 20 000 numbers | 10.5 ms | 4.9 ms | 0.46× |
| `script_strings` | 2000 concatenations + split/map/join | 725 µs | 330 µs | 0.45× |
| `script_objects` | 5000 object literals + filter/map/reduce | 4.02 ms | 0.69 ms | 0.17× |
| `module_evaluate` | declare + evaluate a tiny ES module | 8.5 µs | 27 µs | 3.2× |
| `promise_roundtrip/200` | 200 × resolve a JS promise from Rust and await it | 162 µs | 73 µs | 0.45× |

Reading the table:

- Anything that runs *inside* JS (scripts, JSON, promises) is 2–6× faster on
  rbun thanks to JavaScriptCore's JIT.
- Crossing the Rust ↔ JS boundary is slower on rbun. Values are NaN-boxed so
  numbers/booleans/`undefined` never touch the FFI, `this`/callee are only
  GC-rooted on demand and short property keys are interned, but each call
  still goes through the JavaScriptCore C API (`JSObjectCallAsFunction`,
  `JSObjectGetPropertyForKey`) and `Object::set` goes through a strict-mode
  JS helper so read-only assignments throw like they do in rquickjs. Property
  access is the biggest remaining gap.
- `module_evaluate` pays for the Bun module-loader round trip (`Bun.plugin`
  `onResolve`/`onLoad`) instead of QuickJS's in-process module table.

## Layout

- `src/` — the crate (`ffi.rs` is the JavaScriptCore C API + `bun_embed_*`).
- `macros/` — `#[rbun::class]`, `#[rbun::methods]`.
- `vendor/bun/` — Bun at the commit in `VENDORED_COMMIT` (tests/benchmarks
  dropped) with the patches from `vendor-patches/` applied: `src/runtime/embed.rs`
  (the embedding C ABI) and `scripts/embed-dylib.ts` (links
  `libbun_embed.dylib`) are added, `src/runtime/lib.rs` and
  `src/bundler/transpiler.rs` are edited. Never hand-edit these in `vendor/`;
  change the codemod / `files/` and re-apply.
- `vendor-patches/` — the JSSG codemod, the added files, the generator that
  (re)vendors Bun and applies them, and the generated `bun.gen.patch`.
- `tests/` — rquickjs-core's tests, ported.

# rbun

Embed [Bun](https://bun.com)'s JavaScript runtime in Rust with an API modelled
on [rquickjs](https://docs.rs/rquickjs): `Runtime` / `Context` / `Ctx`,
`Value` / `Object` / `Function` / `Promise` / `Array` / `String`,
`Func` / `Async` / `MutFn` / `This` / `Rest`, `Class` + `#[rbun::class]` /
`#[rbun::methods]`, `Module` + `ModuleDef` + `Resolver` / `Loader`,
`Persistent`, `async_with!`, `Promise::into_future`, `serde`, …

The engine is JavaScriptCore with Bun's event loop, module loader (TypeScript,
JSX, `node:*`, `bun:*`, `node_modules`), and every Bun / Node API. Values are
handled through JavaScriptCore's public C API; the runtime is Bun's own Rust
code (Bun ≥ 1.4 is written in Rust) linked as `libbun_embed.dylib`.

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
vendored Bun checkout (a few small patches; see
[`vendor/bun/RBUN_PATCHES.md`](vendor/bun/RBUN_PATCHES.md)):

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

## Compatibility with rquickjs

The test suite in `tests/` is rquickjs-core's own test suite ported to rbun;
everything runs unchanged except where noted below.

Deviations:

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
- **`set_host_promise_rejection_tracker`** is stored but not invoked; use
  `process.on("unhandledRejection")`.
- **Not implemented:** `Proxy` / `ProxyHandler`, `CString` conversions,
  `Context::custom` intrinsics (accepted, ignored), `Runtime::set_memory_limit`
  / `set_max_stack_size` (accepted, ignored — spawn the JS thread with the
  stack size you need).

## Layout

- `src/` — the crate (`ffi.rs` is the JavaScriptCore C API + `bun_embed_*`).
- `macros/` — `#[rbun::class]`, `#[rbun::methods]`.
- `vendor/bun/` — Bun at the commit in `VENDORED_COMMIT` (tests/benchmarks
  dropped), with `src/runtime/embed.rs` (the embedding C ABI) and
  `scripts/embed-dylib.ts` (links `libbun_embed.dylib`).
- `tests/` — rquickjs-core's tests, ported.

# rbun

> [!NOTE]
> This document is largely AI-written, so parts of it may not be fully accurate. It will be replaced with a hand-written version when I get the time.

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

rbun is the JavaScript runtime embedded in
[Chord](https://github.com/KeyChord/Chord).

## Status

- **macOS only.** Bun is linked as the shared library `libbun_embed.dylib`,
  and rbun's link step only produces the macOS dylib.
- **Bun 1.4.4** is the pinned upstream revision (`oven-sh/bun` commit
  `38afee6c84fc`).
- **Rust crates:** [`rbun`](https://crates.io/crates/rbun) and
  [`rbun-macros`](https://crates.io/crates/rbun-macros) are published as `0.1.0`.
  The native Bun library is distributed separately.
- **Prebuilt dylibs:** CI publishes `libbun_embed.dylib` as GitHub release
  assets. For the current Bun pin only the Apple Silicon (`aarch64`) build is
  published, targeting macOS 13 or newer. Homebrew no longer supplies an Intel
  LLVM bottle, so the Intel (`x86_64`) dependency build exceeds GitHub Actions'
  job limit. Intel CI is opt-in via `include_intel`; on an Intel Mac, build the
  dylib from source. Each published dylib has a `.sha256` checksum file.

## Using rbun

### Add the dependency

```toml
[dependencies]
rbun = "0.1.0"
```

The default `macro` feature provides `#[rbun::class]` and `#[rbun::methods]`.
For local development, clone this repository and use
`rbun = { path = "../rbun/crates/rbun" }` instead, as Chord does. The Bun
submodule is only needed to build Bun itself.

### Get `libbun_embed.dylib`

rbun's build script links `libbun_embed.dylib` from `RBUN_BUN_LIB_DIR` or, when
that is unset, from the checkout's
`packages/com/github/oven-sh/bun/dist/build/release/` (where a
[local build](#building-libbun_embeddylib) puts it). If the file is missing,
the build fails with instructions.

To use a prebuilt dylib instead, download the release that matches the
crate's pinned Bun commit. The `build-bun-embed` workflow tags each release
`bun-embed-<first 12 characters of that commit>` and names the assets
`libbun_embed-<arch>-apple-darwin.dylib`:

```sh
sha="38afee6c84fcf48c0edb2466aa4b27d287a59c30" # rbun 0.1.0 / Bun 1.4.4
dir="$HOME/.cache/rbun/bun-embed-${sha:0:12}"
mkdir -p "$dir"
curl -fL -o "$dir/libbun_embed.dylib" \
  "https://github.com/KeyChord/rbun/releases/download/bun-embed-${sha:0:12}/libbun_embed-aarch64-apple-darwin.dylib"
export RBUN_BUN_LIB_DIR="$dir"
```

The asset has to be saved as `libbun_embed.dylib`, the file name the build
script looks for.

### Link it

rbun passes the dylib's directory to the build script of every crate that
depends on it directly, as `DEP_BUN_EMBED_LIB_DIR`. The dylib's install name is
`@rpath/libbun_embed.dylib`, so an executable needs an rpath to that directory.
rbun adds one for its own tests and examples only; add it for yours in
`build.rs`:

```rust
fn main() {
    if let Ok(dir) = std::env::var("DEP_BUN_EMBED_LIB_DIR") {
        println!("cargo:rustc-link-arg=-Wl,-rpath,{dir}");
    }
}
```

An app you distribute has to ship the dylib and set an rpath to its bundled
copy. Chord, for example, bundles it in `Contents/Frameworks` and links with
`-Wl,-rpath,@executable_path/../Frameworks`.

### Example

This needs `tokio` with the `rt` and `macros` features:

```rust
use rbun::prelude::*;
use rbun::{AsyncContext, AsyncRuntime, Module, async_with};

#[tokio::main(flavor = "current_thread")]
async fn main() -> rbun::Result<()> {
    rbun::run_internal_process_mode();
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

Call `rbun::run_internal_process_mode()` first in `main` (see
[Compared with the Bun executable](#compared-with-the-bun-executable)). Each
thread gets one Bun VM that is never torn down (see
[Compared with rquickjs](#compared-with-rquickjs)).
`cargo run --example smoke` exercises more of the API, including classes,
native modules, and a custom resolver and loader.

## Building `libbun_embed.dylib`

rbun links `packages/com/github/oven-sh/bun/dist/build/release/libbun_embed.dylib`.
Pristine upstream Bun is pinned as the `packages/com/github/oven-sh/bun/src` Git
submodule; [`_vendor.ts`](packages/com/github/oven-sh/bun/_vendor.ts) copies it into the
ignored `dist/` tree and applies the JSSG patch configuration from
[`packages/dev/improve/rbun/configs/patching/`](packages/dev/improve/rbun/configs/patching/README.md).
The generated `bun.gen.patch` there is the reviewable source-to-dist diff.

```sh
# macOS prerequisites
brew install llvm@23 automake ccache cmake coreutils gnu-sed go icu4c libiconv libtool ninja pkg-config ruby
brew install nasm                                    # Intel Macs only
curl -fsSL https://bun.com/install | bash          # a release bun drives bun's build
# plus rustup (https://rustup.rs); the build installs Bun's pinned nightly with it
git submodule update --init --recursive

bun install

bun run _build-bun                                           # ~20 min cold; installs the pinned nightly via rustup

cargo test                                           # Rust API + Bun differential compatibility suites
```

`bun install` links the workspace bins `_vendor`, `_build-bun`, and
`_run-upstream-bun-tests` into `node_modules/.bin`; `bun run <bin>` runs them.
`_build-bun` runs `_vendor generate` before building, so a normal
build always consumes the pinned submodule plus the declared patches. It
writes both `libbun_embed.dylib` and the matching `bun` executable to
`packages/com/github/oven-sh/bun/dist/build/release/`.

### Running the tests

`cargo test` runs the Rust API tests in `crates/rbun/tests/`, the doc tests,
and the [differential suite](#bun-differential-compatibility-suite). Two of
those tests, `bun_runtime_compatibility` and
`one_shot_runtime_host_matches_bun_process_identity_and_lifecycle`, compare
against the reference `bun` executable from a local `_build-bun` build. With
only a prebuilt dylib, skip them:

```sh
cargo test -- --skip bun_runtime_compatibility --skip one_shot_runtime_host
```

### Upgrading Bun

```sh
bun run _vendor update <sha|tag|branch>
bun run _build-bun && cargo test
```

If an upstream change moved one of the patch anchors the codemod fails loudly
(`JSSG patch anchor drifted`); fix
`packages/dev/improve/rbun/configs/patching/codemods/bun/codemod.ts` and its fixtures,
then regenerate. Commit the changed submodule gitlink along with the patch
updates. See the [patching README](packages/dev/improve/rbun/configs/patching/README.md).

## Compatibility with rquickjs

The test suite in `crates/rbun/tests/` ports the applicable public-API tests from
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
  large stack (16 MB works well) and send work to it, as `crates/rbun/tests/common` does.
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
- **`import.meta`:** rquickjs' `Module::meta` wraps QuickJS'
  `JS_GetImportMeta`, which returns any module's live `import.meta` on demand.
  JavaScriptCore has no public equivalent — a module's materialized
  `import.meta` lives in the bytecode layer, not on `AbstractModuleRecord` — so
  rbun offers the same capability push-first:
  `Runtime::set_import_meta_hook` is handed each module's real `import.meta`
  once, at creation, before JSC caches it, and properties set there are what
  that module sees. `Module::meta` still returns the metadata object for
  modules rbun itself declared.
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
  the callback but rbun never invokes it; Bun reports unhandled rejections
  itself, so use `process.on("unhandledRejection")`.

### Compared with the Bun executable

- **Runtime embedding, not the CLI.** rbun boots Bun's VM, APIs, transpiler,
  module loader, and event loop inside the host process. It does not expose
  package-manager/bundler workflows (`bun install`, CLI `bun build`, etc.),
  general CLI argument parsing, watch mode, or bunfig-driven startup
  configuration. The native test runner and one-shot `bun run` lifecycle are
  available as embedding APIs, but rbun is not a general replacement `bun`
  CLI.
- **Host-driven lifetime and event loop.** A VM is process-lifetime and bound
  to its creating thread. Unlike the Bun executable's automatic
  run-to-completion loop, the Rust host drives work with `async_with!`,
  `AsyncRuntime::idle`, `Runtime::idle`, or explicit ticks. A one-shot host can
  instead use `Runtime::configure_entrypoint`, `Runtime::run_eval_source`, and
  the non-returning `Runtime::finish_process` for Bun-native entrypoint, argv,
  `beforeExit`/`exit`, teardown, and exit-code behavior.
- **Internal child modes.** Call `rbun::run_internal_process_mode()` at the
  start of an embedding executable. On macOS, `Bun.WebView` re-executes that
  executable with a private environment marker; this hook transfers the child
  to Bun's native WebKit host loop before argument parsing or JSC startup.
- **Platform support.** The shared-library build/link workflow is macOS-only:
  rbun's embedding link script (`embed-dylib.ts`) only produces
  `libbun_embed.dylib`, although Bun itself supports more platforms.

## Bun differential compatibility suite

`crates/rbun/tests/bun_compat.rs` runs 32 hermetic JS/TS fixtures in fresh
processes under both the same-commit vendored Bun executable and rbun's
production embedding path. It compares exit status, stdout/stderr, and
filesystem side effects. Unlisted differences fail; intentional embedding
differences are locked to exact snapshots in `compat/expected-deviations.json`.
The reference executable comes from a local `_build-bun` build.

```sh
cargo test --test bun_compat -- --nocapture
RBUN_COMPAT_FILTER=modules/ cargo test --test bun_compat -- --nocapture
```

That suite intentionally exercises the continuing, host-driven API rather
than Bun's CLI commands. See [`compat/README.md`](compat/README.md) for the
test contract, extension format, overrides, and expected-deviation policy.

### Native `bun:test` and pinned upstream suites

`Runtime::new_test_with` creates a process-exclusive VM with Bun's real Jest
runner, command-line reporter, hooks, matchers, snapshots, fake timers, test
environment, and exit lifecycle. `Runtime::run_test_file` executes one test
file and returns Bun's native pass/fail/skip/todo/assertion counters. The
`rbun-test-host` binary is the one-shot adapter used by the upstream harness;
when an upstream test calls `bunExe()`, runtime-only script and `-e` children
are routed back through that same embedded rbun host.

The upstream harness reads unchanged tests directly from the pinned `src/`
submodule, runs each file once with the matching Bun built in `dist/` and once
with embedded rbun, and requires both success plus identical summary counters:

```sh
bun run _run-upstream-bun-tests sync
bun run _run-upstream-bun-tests classify

bun run _run-upstream-bun-tests run image
bun run _run-upstream-bun-tests run webview-webkit       # macOS/WebKit
bun run _run-upstream-bun-tests run runtime-smoke
bun run _run-upstream-bun-tests run runtime-subprocess-smoke

# Broad, environment-dependent sweeps; optional substring narrows either set.
bun run _run-upstream-bun-tests run portable-runtime [substring]
bun run _run-upstream-bun-tests run runtime-subprocess [substring]
```

At pinned Bun 1.4.4 revision `38afee6c8`, classification finds 1,091 Bun/Node/Web test
files: 444 in-process, 534 runtime-subprocess, 59 mixed runtime/CLI, and 54
CLI-only. Thus 647 files call `bunExe()`/`bunRun()`; the 534 runtime-only files
are the broad subprocess target, while mixed and CLI files are excluded
because they invoke Bun's package-manager, bundler, or test CLI subcommands.

Passing suites are strong evidence for the covered surfaces, not proof that
every Bun program behaves identically. The broad sets contain platform-, service-,
privilege-, fixture-, and dependency-sensitive tests; the harness reports a
reference failure rather than treating two unavailable/broken executions as
compatible. Named suites and classification policy live in
[`compat/upstream-suites.json`](compat/upstream-suites.json).

## Benchmarks

`cargo bench` runs `crates/rbun/benches/compare.rs`, a criterion suite that drives
rquickjs 0.11 and rbun through the same workloads (each engine on its own
thread; rbun's one-time VM boot is printed once and excluded). Numbers below
are from an Apple Silicon Mac, `--profile=release` Bun, 30 samples × 3 s,
measured in August 2026 against an earlier Bun revision than the current pin;
the last column is rbun's time relative to rquickjs (lower is better).

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

- `crates/rbun/` — library, integration tests, examples, and host binaries
  (`rbun-test-host`, `rbun-compat-host`).
- `crates/rbun-macros/` — `#[rbun::class]`, `#[rbun::methods]`.
- `compat/` — hermetic fixtures, the exact expected-deviation manifest, and
  pinned upstream-suite selection/classification policy.
- `packages/com/github/oven-sh/bun/src/` — pristine Bun pinned as a Git submodule;
  upstream tests and the source revision come directly from this tree.
- `packages/com/github/oven-sh/bun/dist/` — ignored generated build input and local Bun
  artifacts. `_vendor` recreates it from `src/`, then adds the embedding C
  ABI/linker files and applies the codemod's source patches (three files).
- `packages/dev/improve/rbun/configs/patching/` — canonical JSSG codemod, added files,
  fixtures, dependencies, and generated `bun.gen.patch` review diff.
- `packages/dev/improve/rbun/configs/bun/` — `_build-bun` and upstream test harness bins.
- `.github/workflows/build-bun-embed.yaml` — builds `libbun_embed.dylib` on
  macOS runners and publishes it to the `bun-embed-<sha>` release.

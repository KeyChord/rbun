# rbun modifications to the vendored Bun checkout

Upstream: https://github.com/oven-sh/bun @ `VENDORED_COMMIT` (see that file).
`test/`, `bench/` and `.git` are not vendored; `src/runtime/cli/test/` (a
source directory) is.

| Where | What | Why |
| --- | --- | --- |
| `Cargo.toml` `[profile.release]` | `lto = "thin"`, `codegen-units = 16` (upstream: fat LTO, 1 CGU) | local build time; the shipped `bun` binary is not what we consume |
| `src/runtime/embed.rs` (+ `pub mod embed;` in `src/runtime/lib.rs`) | `bun_embed_*` C ABI: process init, VM creation, event-loop ticking, promise helpers | the embedding surface rbun links against |
| `src/bundler/transpiler.rs` `PluginRunner::could_be_plugin` | also forward bare specifiers (no extension, no `ns:`) to runtime `onResolve` hooks | rbun `Resolver`s (rquickjs semantics) must see `import "chord"`; hooks that return `undefined` still fall through to bun's resolver |
| `scripts/embed-dylib.ts` | new: relink the executable's objects as `libbun_embed.dylib` exporting `bun_embed_*` + the JavaScriptCore C API + NAPI | the artifact rbun loads |

Build (from the rbun repo root):

```sh
scripts/build-bun.sh                 # ~20 min cold, needs LLVM 21 + the pinned nightly
# = cd vendor/bun && GIT_SHA=$(cat VENDORED_COMMIT) bun run build --profile=release && bun scripts/embed-dylib.ts
#   -> vendor/bun/build/release/libbun_embed.dylib
```

`GIT_SHA` must be set because bun bakes the enclosing repository's `HEAD` into
a `const` (an empty sha fails const evaluation).

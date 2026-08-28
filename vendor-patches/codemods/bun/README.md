# bun embed patch

JSSG codemod that makes Bun's Rust runtime (`bun_runtime`, Bun ≥ 1.4) embeddable
by rbun. Applied to `vendor/bun` by `../../generate.ts`; see the parent README
for the commands.

## What it touches

| File | Edit | Why |
| --- | --- | --- |
| `src/runtime/lib.rs` | Insert `pub mod embed;` after `pub mod dispatch;` (`mod_item`). | Mounts the C ABI module below in the runtime crate. |
| `src/bundler/transpiler.rs` | In `PluginRunner::could_be_plugin` (`function_item`), replace the `index_of_char_usize(specifier, b':')` gate with "not absolute and not relative". | Upstream only forwards `ns:` specifiers to runtime `onResolve` plugins; rbun's `Resolver` (rquickjs semantics) must see bare names like `chord`. Hooks return `undefined` for names they don't own and Bun's resolver takes over. |
| `files/src/runtime/embed.rs` (new) | `bun_embed_*` extern "C" surface: process and default CLI-context init, per-thread VM creation (holding the JSC API lock), event-loop ticking, promise status/result helpers, module-registry eviction, GC, last-error. | The surface `rbun::ffi` links against. |
| `files/scripts/embed-dylib.ts` (new) | Relinks the executable's link edge as `libbun_embed.dylib` exporting `bun_embed_*`, the JavaScriptCore C API and Bun's `symbols.txt`. | The artifact rbun loads (`vendor/bun/build/release/libbun_embed.dylib`). |

Each transform is routed by a content signature (`pub mod dispatch;` +
`pub mod hw_exports;` for the crate root, `fn could_be_plugin(` for the
transpiler) so the same codemod serves the real files and the fixtures.
Re-running over an already patched file is a no-op (`[rbun patch]` markers).

## Tests

```sh
bun run test      # = bunx codemod jssg test -l rust ./codemod.ts ./@fixtures
```

Fixtures: `runtime-lib` (crate root), `transpiler` (the pre-filter), and
`idempotent` (an already patched input must come out unchanged).

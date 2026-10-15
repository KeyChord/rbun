# bun embed patch

JSSG codemod that makes Bun's Rust runtime (`bun_runtime`, Bun >= 1.4)
embeddable by rbun. Applied to the generated Bun `dist/` by
`packages/com/github/oven-sh/bun/_vendor.ts`; see the parent README for the commands.

## What it touches

| File | Edit | Why |
| --- | --- | --- |
| `src/runtime/lib.rs` | Insert `pub(crate) mod embed;` after `pub(crate) mod dispatch;` (`mod_item`). | Mounts the C ABI module below in the runtime crate. |
| `src/jsc/ModuleLoader.rs` | In `could_be_plugin` (`function_item`), replace the `index_of_char_usize(specifier, b':')` gate with "not absolute and not relative". | Upstream only forwards `ns:` specifiers to runtime `onResolve` plugins; rbun's `Resolver` (rquickjs semantics) must see bare names like `chord`. Hooks return `undefined` for names they don't own and Bun's resolver takes over. |
| `src/jsc/bindings/ZigGlobalObject.cpp` | In `GlobalObject::moduleLoaderCreateImportMetaProperties` (`function_definition`), hand the freshly created `import.meta` to an `extern "C"` callback after upstream's module-graph setup and before returning it. | JSC caches whatever that hook returns as the module's `import.meta` and exposes no public getter for it afterwards, so this is the only point at which an embedder can attach its own properties to a module's real `import.meta`. Gives rbun rquickjs' `Module::meta` capability without patching JSC internals. All three Bun method tables (and Bake, via `INHERIT_HOOK_METHOD`) route to this one body. |
| `files/src/runtime/embed.rs` (new) | `bun_embed_*` extern "C" surface: process and default CLI-context init, per-thread VM creation (holding the JSC API lock), event-loop ticking, promise status/result helpers, module-registry eviction, GC, last-error. | The surface `rbun::ffi` links against. |
| `files/scripts/embed-dylib.ts` (new) | Relinks the executable's link edge, including Rust `.rlib` archives, as `libbun_embed.dylib` exporting `bun_embed_*`, the JavaScriptCore C API and Bun's `symbols.txt`. Uses the matching `llvm-nm` for WebKit LTO symbols. | The artifact rbun loads (`packages/com/github/oven-sh/bun/dist/build/release/libbun_embed.dylib`). |

Each transform is routed by a content signature (`pub(crate) mod dispatch;` +
`pub(crate) mod hw_exports;` for the crate root, `fn could_be_plugin(` for the
module loader, `GlobalObject::moduleLoaderCreateImportMetaProperties` for the
`import.meta` hook) so the same codemod serves the real files and the fixtures.
Re-running over an already patched file is a no-op (`[rbun patch]` markers).

## Tests

```sh
bun run test      # both languages, via `_vendor test`
```

`codemod jssg test` parses every fixture with the one language it is given, so
fixtures are split per language and the runner invokes it once each:

```sh
bunx codemod jssg test -l rust ./codemod.ts ./@fixtures
bunx codemod jssg test -l cpp  ./codemod.ts ./@fixtures-cpp
```

Rust fixtures: `runtime-lib` (crate root), `transpiler` (the pre-filter), and
`idempotent` (an already patched input must come out unchanged).
C++ fixtures: `import-meta` (the hook) and `idempotent`.

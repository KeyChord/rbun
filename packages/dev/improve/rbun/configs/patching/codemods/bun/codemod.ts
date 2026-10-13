/**
 * JSSG codemod that makes Bun's Rust runtime embeddable by rbun.
 *
 * Two source files are touched; each is routed by a content signature so the
 * same codemod can be pointed at either file (and at the fixtures under
 * `@fixtures/`, whose names don't match the real paths):
 *
 *   src/runtime/lib.rs          Mount `pub mod embed;` next to the other
 *                               runtime modules. The module body itself is
 *                               `files/src/runtime/embed.rs`, copied verbatim
 *                               by `packages/com/github/oven-sh/bun/_vendor.ts`.
 *   src/jsc/bindings/ZigGlobalObject.cpp
 *                               `GlobalObject::moduleLoaderCreateImportMetaProperties`
 *                               is JSC's per-module `import.meta` factory. Its
 *                               return value is what JSC caches for the module,
 *                               and all three Bun method tables (plus Bake, via
 *                               `INHERIT_HOOK_METHOD`) route to this one body.
 *                               Give embedders a callback there so they can
 *                               decorate a module's real `import.meta` before it
 *                               is cached. JavaScriptCore has no public
 *                               `JS_GetImportMeta` equivalent (the materialized
 *                               object lives in the bytecode layer, not on
 *                               `AbstractModuleRecord`), so this push-shaped hook
 *                               is how rbun offers rquickjs's `Module::meta`
 *                               capability without patching JSC internals.
 *   src/bundler/transpiler.rs   `PluginRunner::could_be_plugin` upstream only
 *                               forwards `ns:` specifiers to runtime
 *                               `onResolve` hooks, so a bare name such as
 *                               `chord` never reaches rbun's `Resolver` (which
 *                               follows rquickjs semantics and must see every
 *                               specifier). Forward everything that is neither
 *                               absolute nor relative; hooks return `undefined`
 *                               for names they don't own and Bun's regular
 *                               resolution takes over.
 *
 * Idempotent: every replacement carries a `[rbun patch]` marker and is skipped
 * when the marker is already present, so re-running over an already patched
 * file returns `null`. An anchor that no longer matches upstream throws — a
 * loud signal to revisit the patch after a Bun upgrade rather than shipping a
 * silently unpatched runtime.
 */

import type { SgRoot } from "@codemod.com/jssg-types/main";
import type Cpp from "@codemod.com/jssg-types/langs/cpp";
import type Rust from "@codemod.com/jssg-types/langs/rust";

import { applyJssgNodePatches } from "../../jssg-node-patch.ts";

const MARKER = "[rbun patch]";

async function codemod(root: SgRoot<Cpp | Rust>): Promise<string | null> {
  const source = root.root().text();

  // src/runtime/lib.rs: the runtime crate root, identified by its module list.
  if (source.includes("pub mod dispatch;") && source.includes("pub mod hw_exports;")) {
    return patchRuntimeLib(root);
  }
  // src/bundler/transpiler.rs: the plugin pre-filter.
  if (source.includes("fn could_be_plugin(")) {
    return patchTranspiler(root);
  }
  // src/jsc/bindings/ZigGlobalObject.cpp: the per-module `import.meta` factory.
  if (source.includes("GlobalObject::moduleLoaderCreateImportMetaProperties")) {
    return patchImportMeta(root);
  }
  return null;
}

function patchRuntimeLib(root: SgRoot<Rust>): string | null {
  return applyJssgNodePatches(root, [
    {
      contains: "pub mod dispatch;",
      kinds: ["mod_item"],
      replacements: [
        {
          from: "pub mod dispatch;",
          marker: MARKER,
          to: `pub mod dispatch;
// ${MARKER} the \`bun_embed_*\` C ABI rbun links against (process init, VM
// creation, event-loop ticking, promise helpers). Source lives in
// rbun/packages/dev/improve/rbun/configs/patching/codemods/bun/files/src/runtime/embed.rs.
pub mod embed;`,
        },
      ],
    },
  ]);
}

function patchTranspiler(root: SgRoot<Rust>): string | null {
  return applyJssgNodePatches(root, [
    {
      contains: "fn could_be_plugin(specifier: &[u8]) -> bool {",
      kinds: ["function_item"],
      replacements: [
        {
          from: `        !bun_paths::is_absolute(specifier)
            && bun_core::strings::index_of_char_usize(specifier, b':').is_some()
`,
          marker: MARKER,
          to: `        // ${MARKER} upstream only forwards \`ns:\` specifiers here, so a bare
        // name such as \`chord\` never reaches a runtime \`onResolve\` hook.
        // Embedders using rbun's \`Resolver\` (modelled on rquickjs, whose
        // resolvers see every specifier) need bare names too, so forward
        // everything that is neither absolute nor relative. The hook still
        // returns \`undefined\` for names it does not own, falling through to
        // bun's regular resolution.
        !bun_paths::is_absolute(specifier)
            && !(specifier.starts_with(b"./")
                || specifier.starts_with(b"../")
                || specifier == b"."
                || specifier == b"..")
`,
        },
      ],
    },
  ]);
}

/**
 * `src/jsc/bindings/ZigGlobalObject.cpp` — let the embedder decorate each
 * module's `import.meta`.
 *
 * The hook fires once per module, and whatever it returns is the object JSC
 * caches for that module, so a callback placed here can add properties that
 * behave exactly like the built-in ones. `key` is the module specifier, which
 * is what an embedder needs to decide what to attach.
 *
 * Declared `extern "C"` and weak-linked: a host that never sets a callback
 * pays one null check per module.
 */
function patchImportMeta(root: SgRoot<Cpp>): string | null {
  return applyJssgNodePatches(root, [
    {
      contains: "GlobalObject::moduleLoaderCreateImportMetaProperties",
      kinds: ["function_definition"],
      replacements: [
        {
          from: `    return Zig::ImportMetaObject::create(globalObject, key);`,
          marker: MARKER,
          to: `    // ${MARKER} JSC caches whatever this hook returns as the module's
    // \`import.meta\`, and JavaScriptCore exposes no public getter for that
    // cached object afterwards (unlike QuickJS' \`JS_GetImportMeta\`, which
    // rquickjs' \`Module::meta\` wraps). Handing the fresh object to the
    // embedder here is therefore the only place a host can attach its own
    // properties to a module's real \`import.meta\`.
    JSC::JSObject* meta = Zig::ImportMetaObject::create(globalObject, key);
    if (bun_embed_import_meta_callback != nullptr) {
        bun_embed_import_meta_callback(
            reinterpret_cast<void*>(globalObject),
            JSC::JSValue::encode(key),
            JSC::JSValue::encode(JSC::JSValue(meta)));
    }
    return meta;`,
        },
      ],
    },
    {
      contains: "GlobalObject::moduleLoaderCreateImportMetaProperties",
      kinds: ["function_definition"],
      replacements: [
        {
          from: `JSC::JSObject* GlobalObject::moduleLoaderCreateImportMetaProperties(JSGlobalObject* globalObject,`,
          marker: `${MARKER} import.meta embedder callback`,
          to: `// ${MARKER} import.meta embedder callback. Set by the host through
// \`bun_embed_vm_set_import_meta_callback\` (src/runtime/embed.rs); null when no
// host installed one, which is the case for the normal bun executable.
extern "C" void (*bun_embed_import_meta_callback)(void* globalObject, JSC::EncodedJSValue key, JSC::EncodedJSValue meta) = nullptr;

JSC::JSObject* GlobalObject::moduleLoaderCreateImportMetaProperties(JSGlobalObject* globalObject,`,
        },
      ],
    },
  ]);
}

export default codemod;

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
 *                               by `vendor-patches/generate.ts`.
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
import type Rust from "@codemod.com/jssg-types/langs/rust";

import { applyJssgNodePatches } from "../../jssg-node-patch.ts";

const MARKER = "[rbun patch]";

async function codemod(root: SgRoot<Rust>): Promise<string | null> {
  const source = root.root().text();

  // src/runtime/lib.rs: the runtime crate root, identified by its module list.
  if (source.includes("pub mod dispatch;") && source.includes("pub mod hw_exports;")) {
    return patchRuntimeLib(root);
  }
  // src/bundler/transpiler.rs: the plugin pre-filter.
  if (source.includes("fn could_be_plugin(")) {
    return patchTranspiler(root);
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
// rbun/vendor-patches/codemods/bun/files/src/runtime/embed.rs.
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

export default codemod;

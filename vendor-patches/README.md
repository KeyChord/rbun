# vendor-patches

rbun's modifications to the vendored Bun checkout (`../vendor/bun`), kept as a
[JSSG](https://docs.codemod.com/jssg) codemod plus verbatim added files so that
upgrading Bun is a one-command re-vendor rather than a hand merge. The layout
mirrors the `configs/patching` convention in the Han monorepo: one package per
patched upstream under `codemods/`, each with a `patch.json` manifest, a
`codemod.ts`, fixtures, and a generator that applies them.

```
generate.ts             (re)vendor Bun, copy files, run codemods, write bun.gen.patch
jssg-node-patch.ts      shared helper: exact edits inside structurally selected AST nodes
bun.gen.patch           GENERATED — pristine upstream vs patched vendor/bun, for review
codemods/bun/
  patch.json            { repo, target, bundles: [{ path, language }], files: [...] }
  codemod.ts            the JSSG transformation (routes each file by content signature)
  files/<path>          new files copied verbatim into vendor/bun/<path>
  @fixtures/<case>/     input.rs / expected.rs pairs for `bun run test`
  codemod.yaml          codemod package metadata
  workflow.yaml         standalone codemod workflow (unused by the generator)
  README.md             what the patch does and why
```

## Commands

Run from the rbun repo root (needs `bun`, `git`, `rsync`; the `codemod` CLI is
installed into `vendor-patches/node_modules` on first use):

| Command | Effect |
| --- | --- |
| `bun vendor-patches/generate.ts apply` | Copy `files/`, run the codemod over every bundle in `vendor/bun`, rewrite `bun.gen.patch`. Idempotent. |
| `bun vendor-patches/generate.ts check` | Verify `vendor/bun` already carries every patch (files byte-identical, codemod a no-op). Never mutates the vendored tree. |
| `bun vendor-patches/generate.ts upgrade <ref>` | Shallow-clone `oven-sh/bun` at `<ref>`, rsync it over `vendor/bun` (dropping upstream `test/` and `bench/`, keeping local `build/`, `node_modules/`, `vendor/`, `.cache/`), write `VENDORED_COMMIT`, then `apply`. |
| `bun vendor-patches/generate.ts diff` | Rewrite `bun.gen.patch` only (fetches pristine files from GitHub at `VENDORED_COMMIT`). |
| `bun run --cwd vendor-patches test` | Run the codemod's fixture tests (`codemod jssg test -l rust`). |

After `upgrade`: `scripts/build-bun.sh` (rebuilds `libbun_embed.dylib`, ~20 min
cold) and `cargo test`.

## Upstream drift

Every replacement is anchored on exact upstream source inside a structurally
selected node (`mod_item`, `function_item`, …) and carries a `[rbun patch]`
marker for idempotency. When Bun changes an anchor the codemod throws
`JSSG patch anchor drifted …` instead of emitting nothing, so an upgrade can't
silently ship an unpatched runtime. To fix: update the `from` text in
`codemods/bun/codemod.ts`, mirror the change in `@fixtures/`, run the fixture
tests, then `apply`.

`files/` (the `bun_embed_*` C ABI in `src/runtime/embed.rs` and the
`libbun_embed.dylib` link script) are plain copies: if they stop compiling
against a newer Bun, edit them **here** — `vendor/bun` is regenerated output —
and re-run `apply`.

## Not a patch any more

Earlier iterations also edited `vendor/bun/Cargo.toml` (`codegen-units = 16`
for faster local builds). That is now an env override in `scripts/build-bun.sh`
(`CARGO_PROFILE_RELEASE_CODEGEN_UNITS`), which Cargo lets win over the manifest,
so the vendored `Cargo.toml` stays pristine and jssg (which has no TOML parser)
never needs to touch it.

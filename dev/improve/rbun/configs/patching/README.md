# Bun patch configuration

rbun's changes to Bun live here as a
[JSSG](https://docs.codemod.com/jssg) codemod plus verbatim added files. The
upstream source itself is the Git submodule at
`com/github/oven-sh/bun/src`; generated, patched build input goes to the
ignored `com/github/oven-sh/bun/dist` directory.

```text
com/github/oven-sh/bun/
  src/                    pristine, pinned oven-sh/bun submodule
  dist/                   GENERATED source plus rbun patches and local builds
  _vendor.ts              generation, verification, diff, and update commands (`_vendor` bin)

dev/improve/rbun/configs/patching/
  jssg-node-patch.ts      shared exact-edit helper
  bun.gen.patch           GENERATED review diff: src versus patched dist
  codemods/bun/
    patch.json            upstream, dist target, edited bundles, added files
    codemod.ts            JSSG transformation
    files/<path>          files copied verbatim into dist/<path>
    @fixtures/<case>/     codemod input/expected fixtures
```

## Commands

Run these from the rbun repository root after `bun install` (which links the
`_vendor` bin). They require `bun`, `git`, and `rsync`.

| Command | Effect |
| --- | --- |
| `git submodule update --init --recursive` | Initialize the pinned pristine Bun checkout. `_vendor` also initializes its direct submodule automatically. |
| `_vendor generate` | Synchronize `src/` into `dist/`, preserve excluded local build state, apply every patch, and rewrite `bun.gen.patch`. |
| `_vendor check` | Generate an independent scratch distribution and verify both `dist/` and `bun.gen.patch` byte-for-byte. |
| `_vendor diff` | Rewrite only the review patch from the current `src/` and `dist/`. |
| `_vendor update <ref>` | Fetch and detach the source submodule at `<ref>`, then generate `dist/`. Commit the resulting outer-repository gitlink change. |
| `_vendor test` | Run the codemod fixture suite. |

`generate` omits upstream `test/` and `bench/` from `dist/` because the pristine
submodule already carries them. It preserves `build/`, `node_modules/`,
`vendor/`, and `.cache/` in `dist/` so regeneration does not discard expensive
local build state. The generated source itself is ignored by Git; only the
submodule pin, patch configuration, and review diff are committed.

After generating, run `_build-bun` and `cargo test`.

## Upstream drift

Every replacement is anchored on exact upstream source inside a structurally
selected node and carries an `[rbun patch]` marker for idempotency. When Bun
changes an anchor, the codemod throws `JSSG patch anchor drifted` instead of
silently emitting an incomplete distribution. Update `codemods/bun/codemod.ts`
and its fixtures, run `_vendor test`, then regenerate.

The `files/` tree contains the `bun_embed_*` C ABI implementation and the
`libbun_embed` link script. Edit those canonical copies here; never edit
generated `dist/` directly.

The build script sets `CARGO_PROFILE_RELEASE_CODEGEN_UNITS` rather than patching
Bun's `Cargo.toml`, keeping that upstream manifest untouched.

#!/usr/bin/env bash
# Build the vendored Bun and link libbun_embed.dylib.
#
#   scripts/build-bun.sh            # release (the profile rbun expects)
#   scripts/build-bun.sh --profile=debug-no-asan   # any bun build flag
#
# Requirements (macOS): brew install llvm@21 automake ccache cmake coreutils
# gnu-sed go icu4c libiconv libtool ninja pkg-config ruby, rustup (the
# pinned nightly in vendor/bun/rust-toolchain.toml is installed on demand),
# and a release `bun` on PATH.
set -euo pipefail
cd "$(dirname "$0")/../vendor/bun"
# Bun bakes the enclosing repo's HEAD into the binary; report the vendored
# upstream commit instead of whatever rbun's git says.
export GIT_SHA="${GIT_SHA:-$(cat VENDORED_COMMIT)}"
# Upstream pins `codegen-units = 1` for its shipped binary; we only consume the
# dylib locally, so trade a little codegen quality for a much faster build.
# (LTO is unaffected: bun's build already sets CARGO_PROFILE_RELEASE_LTO for
# its cross-language ThinLTO link.) Env overrides the manifest, so the
# vendored Cargo.toml stays pristine.
export CARGO_PROFILE_RELEASE_CODEGEN_UNITS="${CARGO_PROFILE_RELEASE_CODEGEN_UNITS:-16}"
[ -d node_modules ] || bun install
bun scripts/build.ts --profile=release "$@"
bun scripts/embed-dylib.ts
echo "built $(pwd)/build/release/libbun_embed.dylib"

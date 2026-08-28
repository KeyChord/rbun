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
[ -d node_modules ] || bun install
bun scripts/build.ts --profile=release "$@"
bun scripts/embed-dylib.ts
echo "built $(pwd)/build/release/libbun_embed.dylib"

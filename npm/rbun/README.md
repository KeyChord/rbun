# @keychord/rbun

The prebuilt `libbun_embed.dylib` for [rbun](https://github.com/KeyChord/rbun),
bundling **Bun 1.4.5** (`a7733435cf40429e3b35c36072769df2052b80f1`).
Requires Apple Silicon and macOS 13 or newer.

```sh
npm install @keychord/rbun@0.2.0
export RBUN_BUN_LIB_DIR="$(node -p 'require("@keychord/rbun").libDir')"
cargo build
```

Alternatively, install the same tarball directly from the GitHub release:

```sh
npm install https://github.com/KeyChord/rbun/releases/download/bun-embed-a7733435cf40/keychord-rbun-0.2.0.tgz
```

Add the Rust `rbun` crate separately from a checkout of the repository. See the
[linking instructions](https://github.com/KeyChord/rbun#link-it) for the rpath
your executable needs. Applications must bundle the dylib when distributing
their executable.

JavaScript consumers can resolve the library and inspect its Bun version:

```js
const { libDir, libPath, bunVersion, bunRevision } = require("@keychord/rbun");
```

The package contains the dylib, its SHA-256 checksum, and Bun's license notices.
The binary comes from the matching rbun GitHub release. npm installation
requires no build tools or installation scripts.

To publish from the repository, run `npm publish` in `npm/rbun`. The prepack step
downloads the pinned GitHub release, verifies its checksum, architecture and
minimum macOS version, and stages the package contents.

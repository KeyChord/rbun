# rbun

Embed Bun's JavaScript runtime in Rust, with a public API modeled on rquickjs.
The pinned engine is **Bun 1.4.5** (`a7733435cf40429e3b35c36072769df2052b80f1`).

```toml
[dependencies]
rbun = "0.2.0"
```

rbun requires `libbun_embed.dylib` at build time and runtime. Prebuilt libraries
support Apple Silicon and Intel on macOS 13 or newer.

Download the prebuilt library and its checksum:

```sh
mkdir -p .rbun
cd .rbun
base=https://github.com/KeyChord/rbun/releases/download/bun-embed-a7733435cf40
curl -fLO "$base/libbun_embed-aarch64-apple-darwin.dylib"
curl -fLO "$base/libbun_embed-aarch64-apple-darwin.dylib.sha256"
shasum -a 256 -c libbun_embed-aarch64-apple-darwin.dylib.sha256
mv libbun_embed-aarch64-apple-darwin.dylib libbun_embed.dylib
cd ..
export RBUN_BUN_LIB_DIR="$PWD/.rbun"
cargo build
```

The final executable also needs an rpath to the library. Add this to your
application's `build.rs`:

```rust
fn main() {
    if let Ok(dir) = std::env::var("DEP_BUN_EMBED_LIB_DIR") {
        println!("cargo:rustc-link-arg=-Wl,-rpath,{dir}");
    }
}
```

Call `rbun::run_internal_process_mode()` first in `main`. Each thread gets one
Bun VM shared by its contexts. Applications must distribute the dylib alongside
their executable and point an rpath at the bundled library.

The default `macro` feature enables `#[rbun::class]` and `#[rbun::methods]`.
See the [repository documentation](https://github.com/KeyChord/rbun) for examples,
the async API, source builds, and differences from rquickjs and the Bun CLI.

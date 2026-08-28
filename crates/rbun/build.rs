//! Locates `libbun_embed.dylib` (produced from the generated, patched Bun tree
//! at `com/github/oven-sh/bun/dist`) and links it. Override the directory with
//! `RBUN_BUN_LIB_DIR`.
//!
//! The directory is also published as `DEP_BUN_EMBED_LIB_DIR` so the final
//! binary's build script can add an rpath for it.

use std::env;
use std::path::{Path, PathBuf};

fn repo_root(manifest_dir: &Path) -> PathBuf {
    manifest_dir
        .join("../..")
        .canonicalize()
        .unwrap_or_else(|_| manifest_dir.join("../.."))
}

fn main() {
    println!("cargo:rerun-if-env-changed=RBUN_BUN_LIB_DIR");

    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let root = repo_root(&manifest_dir);
    let lib_dir = match env::var("RBUN_BUN_LIB_DIR") {
        Ok(dir) => PathBuf::from(dir),
        Err(_) => root.join("com/github/oven-sh/bun/dist/build/release"),
    };
    let lib_dir = lib_dir.canonicalize().unwrap_or(lib_dir);

    let lib_name = if cfg!(target_os = "macos") {
        "libbun_embed.dylib"
    } else if cfg!(target_os = "windows") {
        "bun_embed.dll"
    } else {
        "libbun_embed.so"
    };
    let lib_path = lib_dir.join(lib_name);
    if !lib_path.exists() {
        panic!(
            "rbun: {} not found.\n\
             Build it first:\n\
             \x20 git submodule update --init --recursive\n\
             \x20 _vendor generate\n\
             \x20 _build-bun\n\
             or point RBUN_BUN_LIB_DIR at a directory containing it.",
            lib_path.display()
        );
    }
    println!("cargo:rerun-if-changed={}", lib_path.display());
    println!("cargo:rustc-link-search=native={}", lib_dir.display());
    println!("cargo:rustc-link-lib=dylib=bun_embed");
    println!("cargo:lib_dir={}", lib_dir.display());
    // Applies to this package's own examples/tests only; the final binary
    // adds its own rpath from `DEP_BUN_EMBED_LIB_DIR` (see src-tauri/build.rs).
    #[cfg(target_os = "macos")]
    println!("cargo:rustc-link-arg=-Wl,-rpath,{}", lib_dir.display());
}

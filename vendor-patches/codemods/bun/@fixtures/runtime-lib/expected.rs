//! The `bun_runtime` crate root (trimmed fixture).
#[path = "api.rs"]
pub mod api;
pub mod dispatch;
// [rbun patch] the `bun_embed_*` C ABI rbun links against (process init, VM
// creation, event-loop ticking, promise helpers). Source lives in
// rbun/vendor-patches/codemods/bun/files/src/runtime/embed.rs.
pub mod embed;
pub mod hw_exports;
pub mod ipc;
pub mod timer;

pub mod generated_classes; // include!()s ${BUN_CODEGEN_DIR}/generated_classes.rs

//! The `bun_runtime` crate root (trimmed fixture).
#[path = "api.rs"]
pub(crate) mod api;
pub(crate) mod dispatch;
// [rbun patch] the `bun_embed_*` C ABI rbun links against (process init, VM
// creation, event-loop ticking, promise helpers). Source lives in
// rbun/packages/dev/improve/rbun/configs/patching/codemods/bun/files/src/runtime/embed.rs.
pub(crate) mod embed;
pub(crate) mod hw_exports;
pub(crate) mod ipc;
pub(crate) mod timer;

pub(crate) mod generated_classes; // include!()s ${BUN_CODEGEN_DIR}/generated_classes.rs

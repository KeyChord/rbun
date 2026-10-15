//! The `bun_runtime` crate root (trimmed fixture).
#[path = "api.rs"]
pub(crate) mod api;
pub(crate) mod dispatch;
pub(crate) mod hw_exports;
pub(crate) mod ipc;
pub(crate) mod timer;

pub(crate) mod generated_classes; // include!()s ${BUN_CODEGEN_DIR}/generated_classes.rs

//! The `bun_runtime` crate root (trimmed fixture).
#[path = "api.rs"]
pub mod api;
pub mod dispatch;
pub mod hw_exports;
pub mod ipc;
pub mod timer;

pub mod generated_classes; // include!()s ${BUN_CODEGEN_DIR}/generated_classes.rs

//! Helpers ported from `llrt_core::libs::utils::result::ResultExt`, so code
//! written against LLRT keeps compiling.

use crate::error::{Error, Exception, Result};
use crate::runtime::Ctx;

pub trait ResultExt<T> {
    /// Turn an error into a pending JS exception with `message: cause`.
    fn or_throw_msg(self, ctx: &Ctx<'_>, msg: &str) -> Result<T>;
    /// Turn an error into a pending JS exception.
    fn or_throw(self, ctx: &Ctx<'_>) -> Result<T>;
    fn or_throw_range(self, ctx: &Ctx<'_>, msg: Option<&str>) -> Result<T>;
    fn or_throw_type(self, ctx: &Ctx<'_>, msg: Option<&str>) -> Result<T>;
}

impl<T, E: core::fmt::Display> ResultExt<T> for core::result::Result<T, E> {
    fn or_throw_msg(self, ctx: &Ctx<'_>, msg: &str) -> Result<T> {
        self.map_err(|e| Exception::throw_message(ctx, &format!("{msg}: {e}")))
    }

    fn or_throw(self, ctx: &Ctx<'_>) -> Result<T> {
        self.map_err(|e| Exception::throw_message(ctx, &e.to_string()))
    }

    fn or_throw_range(self, ctx: &Ctx<'_>, msg: Option<&str>) -> Result<T> {
        self.map_err(|e| match msg {
            Some(msg) => Exception::throw_range(ctx, &format!("{msg}: {e}")),
            None => Exception::throw_range(ctx, &e.to_string()),
        })
    }

    fn or_throw_type(self, ctx: &Ctx<'_>, msg: Option<&str>) -> Result<T> {
        self.map_err(|e| match msg {
            Some(msg) => Exception::throw_type(ctx, &format!("{msg}: {e}")),
            None => Exception::throw_type(ctx, &e.to_string()),
        })
    }
}

/// `Option` support of the same trait, like LLRT's `ResultExt`.
impl<T> ResultExt<T> for Option<T> {
    fn or_throw_msg(self, ctx: &Ctx<'_>, msg: &str) -> Result<T> {
        self.ok_or_else(|| Exception::throw_message(ctx, msg))
    }

    fn or_throw(self, ctx: &Ctx<'_>) -> Result<T> {
        self.ok_or_else(|| Exception::throw_message(ctx, "value was None"))
    }

    fn or_throw_range(self, ctx: &Ctx<'_>, msg: Option<&str>) -> Result<T> {
        self.ok_or_else(|| Exception::throw_range(ctx, msg.unwrap_or("value was None")))
    }

    fn or_throw_type(self, ctx: &Ctx<'_>, msg: Option<&str>) -> Result<T> {
        self.ok_or_else(|| Exception::throw_type(ctx, msg.unwrap_or("value was None")))
    }
}

/// Kept as an alias of [`ResultExt`] for older code.
pub use ResultExt as OptionExt;

/// Render an [`Error`] produced on `ctx` with the pending exception's
/// message and stack, the way LLRT prints uncaught errors.
pub fn format_error(ctx: &Ctx<'_>, error: Error) -> String {
    match crate::error::CaughtError::from_error(ctx, error) {
        crate::error::CaughtError::Error(error) => format!("Internal Engine Error: {error}"),
        crate::error::CaughtError::Exception(exception) => exception.to_string(),
        crate::error::CaughtError::Value(value) => {
            let rendered = value.to_json().ok().flatten().unwrap_or_else(|| value.to_string_lossy());
            format!("JavaScript threw a non-Error value: {rendered}")
        }
    }
}

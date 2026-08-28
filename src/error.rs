//! Error handling, mirroring `rquickjs::{Error, Exception, CaughtError}`.
//!
//! Like rquickjs, a thrown JavaScript value is not carried inside [`Error`]:
//! `Error::Exception` means "an exception is pending on the context", and
//! [`Ctx::catch`](crate::Ctx::catch) / [`CaughtError::from_error`] retrieve it.

use core::fmt;
use core::ops::Range;

use crate::runtime::Ctx;
use crate::value::{Object, Value};

/// Why a Rust value guarded by rbun could not be borrowed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BorrowError {
    AlreadyBorrowed,
    AlreadyUsed,
    NotWritable,
}

impl fmt::Display for BorrowError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BorrowError::AlreadyBorrowed => write!(f, "can't borrow a value as it is already borrowed"),
            BorrowError::AlreadyUsed => write!(f, "tried to use a value, which can only be used once, again."),
            BorrowError::NotWritable => write!(f, "tried to borrow a value which is not writable"),
        }
    }
}

impl std::error::Error for BorrowError {}

/// Why a typed array / array buffer could not be viewed as a slice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AsSliceError {
    BufferUsed,
    InvalidAlignment,
    NotABuffer,
}

impl fmt::Display for AsSliceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AsSliceError::BufferUsed => write!(f, "Tried to use a buffer which was already used"),
            AsSliceError::InvalidAlignment => write!(f, "Buffer had a size unaligned to the requested type"),
            AsSliceError::NotABuffer => write!(f, "Not a buffer"),
        }
    }
}

impl std::error::Error for AsSliceError {}

#[derive(Debug)]
pub enum Error {
    /// Could not allocate memory.
    Allocation,
    /// A module declared the same export twice.
    DuplicateExports,
    /// A string contained an interior NUL.
    InvalidString(std::ffi::NulError),
    /// Invalid UTF-8.
    Utf8(core::str::Utf8Error),
    FromUtf8(std::string::FromUtf8Error),
    Io(std::io::Error),
    /// A value could not be converted from JavaScript.
    FromJs { from: &'static str, to: &'static str, message: Option<String> },
    /// A value could not be converted into JavaScript.
    IntoJs { from: &'static str, to: &'static str, message: Option<String> },
    /// Wrong number of arguments passed to a host function.
    NumArgs { expected: Range<usize>, given: usize },
    /// More arguments than the platform supports.
    TooManyArgs,
    /// Error while resolving a module specifier.
    Resolving { base: String, name: String, message: Option<String> },
    /// Error while loading a module.
    Loading { name: String, message: Option<String> },
    /// A JavaScript exception is pending; fetch it with `Ctx::catch`.
    Exception,
    /// A class instance is already borrowed.
    ClassBorrow(BorrowError),
    /// A host function's closure is already borrowed / used up.
    FunctionBorrow(BorrowError),
    /// User data is still borrowed.
    UserDataBorrow,
    /// A `Persistent` was restored on a different runtime.
    UnrelatedRuntime,
    /// A typed array / array buffer could not be viewed as a slice.
    AsSlice(AsSliceError),
    /// Anything else.
    Unknown,
    /// JSON (de)serialisation failed (rbun addition).
    Json(serde_json::Error),
    /// The runtime stopped executing (rbun addition).
    Stopped,
    /// Bun could not be initialised (rbun addition).
    Init(String),
}

impl Error {
    pub fn new_resolving<B: Into<String>, N: Into<String>>(base: B, name: N) -> Self {
        Error::Resolving { base: base.into(), name: name.into(), message: None }
    }
    pub fn new_resolving_message<B: Into<String>, N: Into<String>, M: Into<String>>(base: B, name: N, message: M) -> Self {
        Error::Resolving { base: base.into(), name: name.into(), message: Some(message.into()) }
    }
    pub fn new_loading<N: Into<String>>(name: N) -> Self {
        Error::Loading { name: name.into(), message: None }
    }
    pub fn new_loading_message<N: Into<String>, M: Into<String>>(name: N, message: M) -> Self {
        Error::Loading { name: name.into(), message: Some(message.into()) }
    }
    pub fn new_from_js(from: &'static str, to: &'static str) -> Self {
        Error::FromJs { from, to, message: None }
    }
    pub fn new_from_js_message(from: &'static str, to: &'static str, message: impl Into<String>) -> Self {
        Error::FromJs { from, to, message: Some(message.into()) }
    }
    pub fn new_into_js(from: &'static str, to: &'static str) -> Self {
        Error::IntoJs { from, to, message: None }
    }
    pub fn new_into_js_message(from: &'static str, to: &'static str, message: impl Into<String>) -> Self {
        Error::IntoJs { from, to, message: Some(message.into()) }
    }

    pub fn is_exception(&self) -> bool {
        matches!(self, Error::Exception)
    }
    pub fn is_from_js(&self) -> bool {
        matches!(self, Error::FromJs { .. })
    }
    pub fn is_from_js_to_js(&self) -> bool {
        matches!(self, Error::FromJs { .. } | Error::IntoJs { .. })
    }
    pub fn is_loading(&self) -> bool {
        matches!(self, Error::Loading { .. })
    }
    pub fn is_resolving(&self) -> bool {
        matches!(self, Error::Resolving { .. })
    }

    /// Convert this error into a JavaScript value suitable for throwing. An
    /// `Error::Exception` yields the pending exception itself.
    pub fn throw<'js>(&self, ctx: &Ctx<'js>) -> Value<'js> {
        match self {
            Error::Exception => ctx.catch(),
            Error::FromJs { .. } | Error::IntoJs { .. } | Error::NumArgs { .. } | Error::TooManyArgs => {
                ctx.new_type_error(&self.to_string())
            }
            Error::Resolving { .. } | Error::Loading { .. } => ctx.new_error_of("ReferenceError", &self.to_string()),
            Error::Allocation => ctx.new_error_of("RangeError", &self.to_string()),
            _ => ctx.new_error(&self.to_string()),
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Allocation => write!(f, "Allocation failed while creating object"),
            Error::DuplicateExports => write!(f, "Tried to export a duplicate export name"),
            Error::InvalidString(e) => write!(f, "String contained internal null bytes: {e}"),
            Error::Utf8(e) => write!(f, "Conversion from string failed: {e}"),
            Error::FromUtf8(e) => write!(f, "Conversion from string failed: {e}"),
            Error::Io(e) => write!(f, "IO Error: {e}"),
            Error::FromJs { from, to, message } => {
                write!(f, "Error converting from js '{from}' into type '{to}'")?;
                if let Some(message) = message {
                    write!(f, ": {message}")?;
                }
                Ok(())
            }
            Error::IntoJs { from, to, message } => {
                write!(f, "Error converting from '{from}' into js '{to}'")?;
                if let Some(message) = message {
                    write!(f, ": {message}")?;
                }
                Ok(())
            }
            Error::NumArgs { expected, given } => {
                write!(f, "Error calling function with {given} argument(s) while {} to {} expected", expected.start, expected.end.saturating_sub(1))
            }
            Error::TooManyArgs => write!(f, "Too many arguments"),
            Error::Resolving { base, name, message } => {
                write!(f, "Error resolving module '{name}' from '{base}'")?;
                if let Some(message) = message {
                    write!(f, ": {message}")?;
                }
                Ok(())
            }
            Error::Loading { name, message } => {
                write!(f, "Error loading module '{name}'")?;
                if let Some(message) = message {
                    write!(f, ": {message}")?;
                }
                Ok(())
            }
            Error::Exception => write!(f, "Exception generated by JavaScript"),
            Error::ClassBorrow(e) => write!(f, "Error borrowing class: {e}"),
            Error::FunctionBorrow(e) => write!(f, "Error borrowing function: {e}"),
            Error::UserDataBorrow => write!(f, "Error borrowing userdata: it is still in use"),
            Error::UnrelatedRuntime => write!(f, "Restoring Persistent in an unrelated runtime"),
            Error::AsSlice(e) => write!(f, "Could not convert buffer to slice: {e}"),
            Error::Unknown => write!(f, "Unknown error"),
            Error::Json(e) => write!(f, "{e}"),
            Error::Stopped => write!(f, "JavaScript execution was stopped"),
            Error::Init(message) => write!(f, "Bun runtime initialisation failed: {message}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<core::str::Utf8Error> for Error {
    fn from(e: core::str::Utf8Error) -> Self {
        Error::Utf8(e)
    }
}
impl From<std::string::FromUtf8Error> for Error {
    fn from(e: std::string::FromUtf8Error) -> Self {
        Error::FromUtf8(e)
    }
}
impl From<std::ffi::NulError> for Error {
    fn from(e: std::ffi::NulError) -> Self {
        Error::InvalidString(e)
    }
}
impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e)
    }
}
impl From<serde_json::Error> for Error {
    fn from(e: serde_json::Error) -> Self {
        Error::Json(e)
    }
}
impl From<BorrowError> for Error {
    fn from(e: BorrowError) -> Self {
        Error::ClassBorrow(e)
    }
}
impl From<AsSliceError> for Error {
    fn from(e: AsSliceError) -> Self {
        Error::AsSlice(e)
    }
}

pub type Result<T> = core::result::Result<T, Error>;

// ─── Exception ───────────────────────────────────────────────────────────

/// A JavaScript `Error` object.
#[repr(transparent)]
#[derive(Clone)]
pub struct Exception<'js>(pub(crate) Object<'js>);

impl<'js> Exception<'js> {
    /// Wrap an object if it is an `Error` instance.
    pub fn from_object(object: Object<'js>) -> Option<Self> {
        object.is_error().then(|| Exception(object))
    }

    pub fn from_value(value: Value<'js>) -> Result<Self> {
        let type_name = value.type_name();
        value.into_exception().ok_or_else(|| Error::new_from_js(type_name, "exception"))
    }

    pub fn as_value(&self) -> &Value<'js> {
        self.0.as_value()
    }

    pub fn into_value(self) -> Value<'js> {
        self.0.into_value()
    }

    pub fn as_object(&self) -> &Object<'js> {
        &self.0
    }

    pub fn into_object(self) -> Object<'js> {
        self.0
    }

    pub fn ctx(&self) -> &Ctx<'js> {
        self.0.ctx()
    }

    fn text(&self, key: &str) -> Option<String> {
        let value: Value<'js> = self.0.get(key).ok()?;
        (!value.is_undefined_or_null()).then(|| value.to_string_lossy())
    }

    pub fn name(&self) -> Option<String> {
        self.text("name")
    }

    pub fn message(&self) -> Option<String> {
        self.text("message")
    }

    pub fn stack(&self) -> Option<String> {
        self.text("stack")
    }

    /// Create `new Error(message)`.
    pub fn from_message(ctx: Ctx<'js>, message: &str) -> Result<Self> {
        Exception::from_value(ctx.new_error(message))
    }

    /// Set `new Error(message)` as the pending exception and return
    /// `Error::Exception`.
    pub fn throw_message(ctx: &Ctx<'js>, message: &str) -> Error {
        ctx.throw(ctx.new_error(message))
    }

    pub fn throw_type(ctx: &Ctx<'js>, message: &str) -> Error {
        ctx.throw(ctx.new_type_error(message))
    }

    pub fn throw_range(ctx: &Ctx<'js>, message: &str) -> Error {
        ctx.throw(ctx.new_error_of("RangeError", message))
    }

    pub fn throw_syntax(ctx: &Ctx<'js>, message: &str) -> Error {
        ctx.throw(ctx.new_error_of("SyntaxError", message))
    }

    pub fn throw_reference(ctx: &Ctx<'js>, message: &str) -> Error {
        ctx.throw(ctx.new_error_of("ReferenceError", message))
    }

    pub fn throw_internal(ctx: &Ctx<'js>, message: &str) -> Error {
        ctx.throw(ctx.new_error(message))
    }

    /// Set this exception as pending and return `Error::Exception`.
    pub fn throw(self) -> Error {
        let ctx = *self.ctx();
        ctx.throw(self.into_value())
    }
}

impl fmt::Debug for Exception<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Exception")
            .field("name", &self.name())
            .field("message", &self.message())
            .field("stack", &self.stack())
            .finish()
    }
}

impl fmt::Display for Exception<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = self.name().unwrap_or_else(|| "Error".into());
        let message = self.message().map(|m| m.trim().to_string()).filter(|m| !m.is_empty());
        let stack = self.stack().map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
        match (message, stack) {
            // JSC stacks do not repeat the message; print `Name: message` then the frames.
            (Some(message), Some(stack)) if stack.contains(&message) => write!(f, "{stack}"),
            (Some(message), Some(stack)) => write!(f, "{name}: {message}\n{stack}"),
            (Some(message), None) => write!(f, "{name}: {message}"),
            (None, Some(stack)) => write!(f, "{name}\n{stack}"),
            (None, None) => write!(f, "{}", self.as_value().to_string_lossy()),
        }
    }
}

// ─── CaughtError ─────────────────────────────────────────────────────────

/// An [`Error`] with the pending exception resolved: either a plain error, a
/// thrown `Error` object, or an arbitrary thrown value.
pub enum CaughtError<'js> {
    Error(Error),
    Exception(Exception<'js>),
    Value(Value<'js>),
}

impl<'js> CaughtError<'js> {
    pub fn from_error(ctx: &Ctx<'js>, error: Error) -> Self {
        if !error.is_exception() {
            return CaughtError::Error(error);
        }
        let value = ctx.catch();
        match value.try_into_exception() {
            Ok(exception) => CaughtError::Exception(exception),
            Err(value) => CaughtError::Value(value),
        }
    }

    /// Alias of [`from_error`](Self::from_error).
    pub fn catch(ctx: &Ctx<'js>, error: Error) -> Self {
        Self::from_error(ctx, error)
    }

    pub fn is_exception(&self) -> bool {
        matches!(self, CaughtError::Exception(_))
    }

    /// Re-throw: set the value as pending and return `Error::Exception`.
    pub fn throw(self, ctx: &Ctx<'js>) -> Error {
        match self {
            CaughtError::Error(error) => {
                let value = error.throw(ctx);
                ctx.throw(value)
            }
            CaughtError::Exception(exception) => ctx.throw(exception.into_value()),
            CaughtError::Value(value) => ctx.throw(value),
        }
    }
}

impl fmt::Debug for CaughtError<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CaughtError::Error(e) => f.debug_tuple("Error").field(e).finish(),
            CaughtError::Exception(e) => f.debug_tuple("Exception").field(e).finish(),
            CaughtError::Value(v) => f.debug_tuple("Value").field(v).finish(),
        }
    }
}

impl fmt::Display for CaughtError<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CaughtError::Error(e) => write!(f, "{e}"),
            CaughtError::Exception(e) => write!(f, "{e}"),
            CaughtError::Value(v) => write!(f, "Exception generated by JavaScript: {}", v.to_string_lossy()),
        }
    }
}

impl std::error::Error for CaughtError<'_> {}

pub type CaughtResult<'js, T> = core::result::Result<T, CaughtError<'js>>;

/// `.catch(&ctx)`: resolve a pending exception into a [`CaughtError`].
pub trait CatchResultExt<'js, T> {
    fn catch(self, ctx: &Ctx<'js>) -> CaughtResult<'js, T>;
}

impl<'js, T> CatchResultExt<'js, T> for Result<T> {
    fn catch(self, ctx: &Ctx<'js>) -> CaughtResult<'js, T> {
        self.map_err(|error| CaughtError::from_error(ctx, error))
    }
}

/// `.throw(&ctx)`: turn a [`CaughtError`] back into a pending exception.
pub trait ThrowResultExt<'js, T> {
    fn throw(self, ctx: &Ctx<'js>) -> Result<T>;
}

impl<'js, T> ThrowResultExt<'js, T> for CaughtResult<'js, T> {
    fn throw(self, ctx: &Ctx<'js>) -> Result<T> {
        self.map_err(|error| error.throw(ctx))
    }
}

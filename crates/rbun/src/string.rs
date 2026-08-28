use crate::ffi;

/// Owned `JSStringRef`.
pub(crate) struct JsString(ffi::JSStringRef);

impl JsString {
    pub(crate) fn new(s: &str) -> Self {
        let utf16: Vec<u16> = s.encode_utf16().collect();
        // SAFETY: JSC copies the buffer.
        JsString(unsafe { ffi::JSStringCreateWithCharacters(utf16.as_ptr(), utf16.len()) })
    }

    /// Takes ownership of a string returned by the C API (already retained).
    pub(crate) unsafe fn from_raw(raw: ffi::JSStringRef) -> Option<Self> {
        if raw.is_null() { None } else { Some(JsString(raw)) }
    }

    pub(crate) fn as_raw(&self) -> ffi::JSStringRef {
        self.0
    }

    pub(crate) fn to_rust_string(&self) -> String {
        // SAFETY: `self.0` is a live JSStringRef; JSC guarantees the UTF-16
        // buffer stays valid for the string's lifetime.
        unsafe {
            let len = ffi::JSStringGetLength(self.0);
            let ptr = ffi::JSStringGetCharactersPtr(self.0);
            if ptr.is_null() || len == 0 {
                return String::new();
            }
            String::from_utf16_lossy(core::slice::from_raw_parts(ptr, len))
        }
    }
}

impl Drop for JsString {
    fn drop(&mut self) {
        // SAFETY: owned reference.
        unsafe { ffi::JSStringRelease(self.0) };
    }
}

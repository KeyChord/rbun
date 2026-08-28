//! `ArrayBuffer` and typed arrays, mirroring `rquickjs::{ArrayBuffer, TypedArray}`.

use core::fmt;
use core::marker::PhantomData;
use core::ops::Deref;

use crate::error::{AsSliceError, Error, Result};
use crate::ffi;
use crate::runtime::Ctx;
use crate::value::{FromJs, IntoJs, Object, Value};

/// Element types of typed arrays.
pub trait TypedArrayItem: Copy + 'static {
    const CLASS_NAME: &'static str;
    const ARRAY_TYPE: ffi::JSTypedArrayType;
}

macro_rules! typed_array_item {
    ($($t:ty => $name:literal, $ty:ident);* $(;)?) => {$(
        impl TypedArrayItem for $t {
            const CLASS_NAME: &'static str = $name;
            const ARRAY_TYPE: ffi::JSTypedArrayType = ffi::$ty;
        }
    )*};
}
typed_array_item! {
    i8 => "Int8Array", kJSTypedArrayTypeInt8Array;
    u8 => "Uint8Array", kJSTypedArrayTypeUint8Array;
    i16 => "Int16Array", kJSTypedArrayTypeInt16Array;
    u16 => "Uint16Array", kJSTypedArrayTypeUint16Array;
    i32 => "Int32Array", kJSTypedArrayTypeInt32Array;
    u32 => "Uint32Array", kJSTypedArrayTypeUint32Array;
    i64 => "BigInt64Array", kJSTypedArrayTypeBigInt64Array;
    u64 => "BigUint64Array", kJSTypedArrayTypeBigUint64Array;
    f32 => "Float32Array", kJSTypedArrayTypeFloat32Array;
    f64 => "Float64Array", kJSTypedArrayTypeFloat64Array;
}

unsafe extern "C" fn free_buffer(bytes: *mut core::ffi::c_void, context: *mut core::ffi::c_void) {
    let len = context as usize;
    // SAFETY: allocated by `Vec::with_capacity(len)` + `into_raw_parts` below.
    drop(unsafe { Vec::from_raw_parts(bytes.cast::<u8>(), len, len) });
}

fn raw_typed_array_type(value: &Value<'_>) -> ffi::JSTypedArrayType {
    let mut exception: ffi::JSValueRef = core::ptr::null();
    // SAFETY: live value.
    let ty = unsafe { ffi::JSValueGetTypedArrayType(value.ctx().raw(), value.as_raw(), &mut exception) };
    if !exception.is_null() {
        let _ = value.ctx().throw_raw(exception);
        let _ = value.ctx().catch();
        return ffi::kJSTypedArrayTypeNone;
    }
    ty
}

// ─── ArrayBuffer ─────────────────────────────────────────────────────────

#[repr(transparent)]
#[derive(Clone)]
pub struct ArrayBuffer<'js>(pub(crate) Object<'js>);

impl<'js> ArrayBuffer<'js> {
    /// Copy `src` into a new `ArrayBuffer`.
    pub fn new<T: Copy>(ctx: Ctx<'js>, src: impl AsRef<[T]>) -> Result<Self> {
        let src = src.as_ref();
        let byte_len = core::mem::size_of_val(src);
        // SAFETY: `T: Copy` — reinterpreting its bytes is sound.
        let bytes = unsafe { core::slice::from_raw_parts(src.as_ptr().cast::<u8>(), byte_len) };
        Self::new_copy(ctx, bytes)
    }

    pub fn new_copy(ctx: Ctx<'js>, bytes: &[u8]) -> Result<Self> {
        let mut owned = Vec::with_capacity(bytes.len().max(1));
        owned.extend_from_slice(bytes);
        let len = owned.len();
        let capacity_matches = owned.capacity() == len || len == 0;
        if !capacity_matches {
            owned.shrink_to_fit();
        }
        let mut owned = core::mem::ManuallyDrop::new(owned);
        let ptr = owned.as_mut_ptr();
        let mut exception: ffi::JSValueRef = core::ptr::null();
        // SAFETY: JSC takes ownership; `free_buffer` frees the Vec with the
        // same length/capacity.
        let raw = unsafe {
            ffi::JSObjectMakeArrayBufferWithBytesNoCopy(
                ctx.raw(),
                ptr.cast(),
                len,
                Some(free_buffer),
                len as *mut core::ffi::c_void,
                &mut exception,
            )
        };
        if !exception.is_null() {
            // SAFETY: JSC did not take the buffer.
            drop(unsafe { Vec::from_raw_parts(ptr, len, len) });
            return Err(ctx.throw_raw(exception));
        }
        // SAFETY: fresh object.
        Ok(ArrayBuffer(Object(unsafe { Value::from_raw(ctx, raw as ffi::JSValueRef) })))
    }

    pub fn from_object(object: Object<'js>) -> Option<Self> {
        (raw_typed_array_type(&object) == ffi::kJSTypedArrayTypeArrayBuffer).then(|| ArrayBuffer(object))
    }

    /// Byte length.
    pub fn len(&self) -> usize {
        let mut exception: ffi::JSValueRef = core::ptr::null();
        // SAFETY: live buffer.
        let len = unsafe { ffi::JSObjectGetArrayBufferByteLength(self.ctx().raw(), self.raw_object(), &mut exception) };
        if exception.is_null() { len } else { 0 }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The raw bytes; `None` when detached.
    pub fn as_bytes(&self) -> Option<&[u8]> {
        let mut exception: ffi::JSValueRef = core::ptr::null();
        // SAFETY: live buffer.
        let ptr = unsafe { ffi::JSObjectGetArrayBufferBytesPtr(self.ctx().raw(), self.raw_object(), &mut exception) };
        if !exception.is_null() || ptr.is_null() {
            return None;
        }
        // SAFETY: the buffer stays alive while `self` (a protection) does.
        Some(unsafe { core::slice::from_raw_parts(ptr.cast::<u8>(), self.len()) })
    }

    /// View as a slice of `T`.
    pub fn as_slice<T: TypedArrayItem>(&self) -> Result<&[T]> {
        let bytes = self.as_bytes().ok_or(AsSliceError::BufferUsed)?;
        if bytes.len() % core::mem::size_of::<T>() != 0 || bytes.as_ptr().align_offset(core::mem::align_of::<T>()) != 0 {
            return Err(AsSliceError::InvalidAlignment.into());
        }
        // SAFETY: checked size and alignment; `T` is a plain number type.
        Ok(unsafe { core::slice::from_raw_parts(bytes.as_ptr().cast::<T>(), bytes.len() / core::mem::size_of::<T>()) })
    }

    pub fn as_object(&self) -> &Object<'js> {
        &self.0
    }
    pub fn into_object(self) -> Object<'js> {
        self.0
    }
    pub fn as_value(&self) -> &Value<'js> {
        self.0.as_value()
    }
    pub fn into_value(self) -> Value<'js> {
        self.0.into_value()
    }
    pub fn ctx(&self) -> &Ctx<'js> {
        self.0.ctx()
    }
}

impl<'js> Deref for ArrayBuffer<'js> {
    type Target = Object<'js>;
    fn deref(&self) -> &Object<'js> {
        &self.0
    }
}

impl<'js, T: TypedArrayItem> AsRef<[T]> for ArrayBuffer<'js> {
    fn as_ref(&self) -> &[T] {
        self.as_slice().expect("ArrayBuffer cannot be viewed as this slice type")
    }
}

impl<'js> IntoJs<'js> for ArrayBuffer<'js> {
    fn into_js(self, _ctx: &Ctx<'js>) -> Result<Value<'js>> {
        Ok(self.into_value())
    }
}

impl<'js> FromJs<'js> for ArrayBuffer<'js> {
    fn from_js(_ctx: &Ctx<'js>, value: Value<'js>) -> Result<Self> {
        let type_name = value.type_name();
        value
            .into_object()
            .and_then(ArrayBuffer::from_object)
            .ok_or_else(|| Error::new_from_js(type_name, "ArrayBuffer"))
    }
}

impl fmt::Debug for ArrayBuffer<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ArrayBuffer({} bytes)", self.len())
    }
}

// ─── TypedArray ──────────────────────────────────────────────────────────

#[repr(transparent)]
pub struct TypedArray<'js, T>(pub(crate) Object<'js>, PhantomData<T>);

impl<'js, T> Clone for TypedArray<'js, T> {
    fn clone(&self) -> Self {
        TypedArray(self.0.clone(), PhantomData)
    }
}

impl<'js, T: TypedArrayItem> TypedArray<'js, T> {
    /// Copy `src` into a new typed array.
    pub fn new(ctx: Ctx<'js>, src: impl AsRef<[T]>) -> Result<Self> {
        let buffer = ArrayBuffer::new(ctx, src)?;
        Self::from_arraybuffer(buffer)
    }

    /// A typed array over the whole buffer.
    pub fn from_arraybuffer(buffer: ArrayBuffer<'js>) -> Result<Self> {
        let ctx = *buffer.ctx();
        let mut exception: ffi::JSValueRef = core::ptr::null();
        // SAFETY: valid context; live buffer.
        let raw = unsafe { ffi::JSObjectMakeTypedArrayWithArrayBuffer(ctx.raw(), T::ARRAY_TYPE, buffer.raw_object(), &mut exception) };
        if !exception.is_null() {
            return Err(ctx.throw_raw(exception));
        }
        // SAFETY: fresh object.
        Ok(TypedArray(Object(unsafe { Value::from_raw(ctx, raw as ffi::JSValueRef) }), PhantomData))
    }

    pub fn from_object(object: Object<'js>) -> Option<Self> {
        (raw_typed_array_type(&object) == T::ARRAY_TYPE).then(|| TypedArray(object, PhantomData))
    }

    /// Element count.
    pub fn len(&self) -> usize {
        let mut exception: ffi::JSValueRef = core::ptr::null();
        // SAFETY: live typed array.
        let len = unsafe { ffi::JSObjectGetTypedArrayLength(self.ctx().raw(), self.raw_object(), &mut exception) };
        if exception.is_null() { len } else { 0 }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The underlying `ArrayBuffer`.
    pub fn arraybuffer(&self) -> Result<ArrayBuffer<'js>> {
        let mut exception: ffi::JSValueRef = core::ptr::null();
        // SAFETY: live typed array.
        let raw = unsafe { ffi::JSObjectGetTypedArrayBuffer(self.ctx().raw(), self.raw_object(), &mut exception) };
        if !exception.is_null() {
            return Err(self.ctx().throw_raw(exception));
        }
        // SAFETY: object from the C API.
        Ok(ArrayBuffer(Object(unsafe { Value::from_raw(*self.ctx(), raw as ffi::JSValueRef) })))
    }

    /// The raw bytes of the view; `None` when detached.
    pub fn as_bytes(&self) -> Option<&[u8]> {
        let mut exception: ffi::JSValueRef = core::ptr::null();
        // SAFETY: live typed array.
        let ptr = unsafe { ffi::JSObjectGetTypedArrayBytesPtr(self.ctx().raw(), self.raw_object(), &mut exception) };
        if !exception.is_null() || ptr.is_null() {
            return None;
        }
        let mut exception: ffi::JSValueRef = core::ptr::null();
        // SAFETY: live typed array.
        let byte_len = unsafe { ffi::JSObjectGetTypedArrayByteLength(self.ctx().raw(), self.raw_object(), &mut exception) };
        // SAFETY: the buffer stays alive while `self` (a protection) does.
        Some(unsafe { core::slice::from_raw_parts(ptr.cast::<u8>(), byte_len) })
    }

    pub fn as_slice(&self) -> Result<&[T]> {
        let bytes = self.as_bytes().ok_or(AsSliceError::BufferUsed)?;
        if bytes.as_ptr().align_offset(core::mem::align_of::<T>()) != 0 {
            return Err(AsSliceError::InvalidAlignment.into());
        }
        // SAFETY: a typed array's bytes are a whole number of `T`s.
        Ok(unsafe { core::slice::from_raw_parts(bytes.as_ptr().cast::<T>(), bytes.len() / core::mem::size_of::<T>()) })
    }

    pub fn as_object(&self) -> &Object<'js> {
        &self.0
    }
    pub fn into_object(self) -> Object<'js> {
        self.0
    }
    pub fn as_value(&self) -> &Value<'js> {
        self.0.as_value()
    }
    pub fn into_value(self) -> Value<'js> {
        self.0.into_value()
    }
    pub fn ctx(&self) -> &Ctx<'js> {
        self.0.ctx()
    }
}

impl<'js, T> Deref for TypedArray<'js, T> {
    type Target = Object<'js>;
    fn deref(&self) -> &Object<'js> {
        &self.0
    }
}

impl<'js, T: TypedArrayItem> AsRef<[T]> for TypedArray<'js, T> {
    fn as_ref(&self) -> &[T] {
        self.as_slice().expect("TypedArray cannot be viewed as a slice")
    }
}

impl<'js, T: TypedArrayItem> IntoJs<'js> for TypedArray<'js, T> {
    fn into_js(self, _ctx: &Ctx<'js>) -> Result<Value<'js>> {
        Ok(self.into_value())
    }
}

impl<'js, T: TypedArrayItem> FromJs<'js> for TypedArray<'js, T> {
    fn from_js(_ctx: &Ctx<'js>, value: Value<'js>) -> Result<Self> {
        let type_name = value.type_name();
        value
            .into_object()
            .and_then(TypedArray::from_object)
            .ok_or_else(|| Error::new_from_js(type_name, T::CLASS_NAME))
    }
}

impl<'js, T: TypedArrayItem> fmt::Debug for TypedArray<'js, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}({})", T::CLASS_NAME, self.len())
    }
}

impl<'js> Object<'js> {
    pub fn is_typed_array<T: TypedArrayItem>(&self) -> bool {
        raw_typed_array_type(self) == T::ARRAY_TYPE
    }

    pub fn is_array_buffer(&self) -> bool {
        raw_typed_array_type(self) == ffi::kJSTypedArrayTypeArrayBuffer
    }

    pub fn as_typed_array<T: TypedArrayItem>(&self) -> Option<&TypedArray<'js, T>> {
        // SAFETY: `TypedArray` is `repr(transparent)` over `Object`.
        self.is_typed_array::<T>().then(|| unsafe { &*(self as *const Object<'js> as *const TypedArray<'js, T>) })
    }

    pub fn as_array_buffer(&self) -> Option<&ArrayBuffer<'js>> {
        // SAFETY: `ArrayBuffer` is `repr(transparent)` over `Object`.
        self.is_array_buffer().then(|| unsafe { &*(self as *const Object<'js> as *const ArrayBuffer<'js>) })
    }
}

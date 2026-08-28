//! Raw bindings: the subset of JavaScriptCore's public C API that `rbun` uses
//! (`<JavaScriptCore/JavaScript.h>`), plus Bun's embedding entry points
//! (`com/github/oven-sh/bun/dist/src/runtime/embed.rs`). All exported by
//! `libbun_embed.dylib`.

#![allow(non_snake_case, non_camel_case_types, non_upper_case_globals, dead_code)]

use core::ffi::{c_char, c_int, c_uint, c_void};

pub type JSContextRef = *const c_void;
pub type JSGlobalContextRef = *mut c_void;
pub type JSValueRef = *const c_void;
pub type JSObjectRef = *mut c_void;
pub type JSStringRef = *mut c_void;
pub type JSClassRef = *mut c_void;
pub type JSPropertyNameArrayRef = *mut c_void;
pub type JSChar = u16;

pub type JSType = c_uint;
pub const kJSTypeUndefined: JSType = 0;
pub const kJSTypeNull: JSType = 1;
pub const kJSTypeBoolean: JSType = 2;
pub const kJSTypeNumber: JSType = 3;
pub const kJSTypeString: JSType = 4;
pub const kJSTypeObject: JSType = 5;
pub const kJSTypeSymbol: JSType = 6;
pub const kJSTypeBigInt: JSType = 7;

pub type JSPropertyAttributes = c_uint;
pub const kJSPropertyAttributeNone: JSPropertyAttributes = 0;
pub const kJSPropertyAttributeReadOnly: JSPropertyAttributes = 1 << 1;
pub const kJSPropertyAttributeDontEnum: JSPropertyAttributes = 1 << 2;
pub const kJSPropertyAttributeDontDelete: JSPropertyAttributes = 1 << 3;

pub type JSClassAttributes = c_uint;
pub const kJSClassAttributeNone: JSClassAttributes = 0;
pub const kJSClassAttributeNoAutomaticPrototype: JSClassAttributes = 1 << 1;


pub type JSTypedArrayType = c_uint;
pub const kJSTypedArrayTypeInt8Array: JSTypedArrayType = 0;
pub const kJSTypedArrayTypeInt16Array: JSTypedArrayType = 1;
pub const kJSTypedArrayTypeInt32Array: JSTypedArrayType = 2;
pub const kJSTypedArrayTypeUint8Array: JSTypedArrayType = 3;
pub const kJSTypedArrayTypeUint8ClampedArray: JSTypedArrayType = 4;
pub const kJSTypedArrayTypeUint16Array: JSTypedArrayType = 5;
pub const kJSTypedArrayTypeUint32Array: JSTypedArrayType = 6;
pub const kJSTypedArrayTypeFloat32Array: JSTypedArrayType = 7;
pub const kJSTypedArrayTypeFloat64Array: JSTypedArrayType = 8;
pub const kJSTypedArrayTypeArrayBuffer: JSTypedArrayType = 9;
pub const kJSTypedArrayTypeNone: JSTypedArrayType = 10;
pub const kJSTypedArrayTypeBigInt64Array: JSTypedArrayType = 11;
pub const kJSTypedArrayTypeBigUint64Array: JSTypedArrayType = 12;

pub type JSTypedArrayBytesDeallocator = Option<unsafe extern "C" fn(bytes: *mut c_void, deallocator_context: *mut c_void)>;

/// Result counters produced by Bun's native `bun:test` runner.
#[repr(C)]
#[derive(Debug, Default, Clone, Copy)]
pub struct BunEmbedTestResult {
    pub pass: u32,
    pub fail: u32,
    pub skip: u32,
    pub todo: u32,
    pub expectations: u32,
    pub files: u32,
    pub unhandled_errors: u32,
}

pub type JSObjectInitializeCallback = Option<unsafe extern "C" fn(ctx: JSContextRef, object: JSObjectRef)>;
pub type JSObjectFinalizeCallback = Option<unsafe extern "C" fn(object: JSObjectRef)>;
pub type JSObjectCallAsFunctionCallback = Option<
    unsafe extern "C" fn(
        ctx: JSContextRef,
        function: JSObjectRef,
        this_object: JSObjectRef,
        argument_count: usize,
        arguments: *const JSValueRef,
        exception: *mut JSValueRef,
    ) -> JSValueRef,
>;
pub type JSObjectCallAsConstructorCallback = Option<
    unsafe extern "C" fn(
        ctx: JSContextRef,
        constructor: JSObjectRef,
        argument_count: usize,
        arguments: *const JSValueRef,
        exception: *mut JSValueRef,
    ) -> JSObjectRef,
>;

#[repr(C)]
pub struct JSStaticValue {
    pub name: *const c_char,
    pub get_property: *const c_void,
    pub set_property: *const c_void,
    pub attributes: JSPropertyAttributes,
}

#[repr(C)]
pub struct JSStaticFunction {
    pub name: *const c_char,
    pub call_as_function: JSObjectCallAsFunctionCallback,
    pub attributes: JSPropertyAttributes,
}

/// Field order matches `JSObjectRef.h` exactly.
#[repr(C)]
pub struct JSClassDefinition {
    pub version: c_int,
    pub attributes: JSClassAttributes,
    pub class_name: *const c_char,
    pub parent_class: JSClassRef,
    pub static_values: *const JSStaticValue,
    pub static_functions: *const JSStaticFunction,
    pub initialize: JSObjectInitializeCallback,
    pub finalize: JSObjectFinalizeCallback,
    pub has_property: *const c_void,
    pub get_property: *const c_void,
    pub set_property: *const c_void,
    pub delete_property: *const c_void,
    pub get_property_names: *const c_void,
    pub call_as_function: JSObjectCallAsFunctionCallback,
    pub call_as_constructor: JSObjectCallAsConstructorCallback,
    pub has_instance: *const c_void,
    pub convert_to_type: *const c_void,
}

impl JSClassDefinition {
    pub const EMPTY: JSClassDefinition = JSClassDefinition {
        version: 0,
        attributes: kJSClassAttributeNone,
        class_name: core::ptr::null(),
        parent_class: core::ptr::null_mut(),
        static_values: core::ptr::null(),
        static_functions: core::ptr::null(),
        initialize: None,
        finalize: None,
        has_property: core::ptr::null(),
        get_property: core::ptr::null(),
        set_property: core::ptr::null(),
        delete_property: core::ptr::null(),
        get_property_names: core::ptr::null(),
        call_as_function: None,
        call_as_constructor: None,
        has_instance: core::ptr::null(),
        convert_to_type: core::ptr::null(),
    };
}

unsafe extern "C" {
    // ─── JSBase / JSContextRef ───
    pub fn JSEvaluateScript(
        ctx: JSContextRef,
        script: JSStringRef,
        this_object: JSObjectRef,
        source_url: JSStringRef,
        starting_line_number: c_int,
        exception: *mut JSValueRef,
    ) -> JSValueRef;
    pub fn JSGarbageCollect(ctx: JSContextRef);
    pub fn JSContextGetGlobalObject(ctx: JSContextRef) -> JSObjectRef;

    // ─── JSStringRef ───
    pub fn JSStringCreateWithCharacters(chars: *const JSChar, num_chars: usize) -> JSStringRef;
    pub fn JSStringCreateWithUTF8CString(string: *const c_char) -> JSStringRef;
    pub fn JSStringRetain(string: JSStringRef) -> JSStringRef;
    pub fn JSStringRelease(string: JSStringRef);
    pub fn JSStringGetLength(string: JSStringRef) -> usize;
    pub fn JSStringGetCharactersPtr(string: JSStringRef) -> *const JSChar;
    pub fn JSStringGetMaximumUTF8CStringSize(string: JSStringRef) -> usize;
    pub fn JSStringGetUTF8CString(string: JSStringRef, buffer: *mut c_char, buffer_size: usize) -> usize;

    // ─── JSValueRef ───
    pub fn JSValueGetType(ctx: JSContextRef, value: JSValueRef) -> JSType;
    pub fn JSValueIsUndefined(ctx: JSContextRef, value: JSValueRef) -> bool;
    pub fn JSValueIsNull(ctx: JSContextRef, value: JSValueRef) -> bool;
    pub fn JSValueIsBoolean(ctx: JSContextRef, value: JSValueRef) -> bool;
    pub fn JSValueIsNumber(ctx: JSContextRef, value: JSValueRef) -> bool;
    pub fn JSValueIsString(ctx: JSContextRef, value: JSValueRef) -> bool;
    pub fn JSValueIsObject(ctx: JSContextRef, value: JSValueRef) -> bool;
    pub fn JSValueIsArray(ctx: JSContextRef, value: JSValueRef) -> bool;
    pub fn JSValueIsStrictEqual(ctx: JSContextRef, a: JSValueRef, b: JSValueRef) -> bool;
    pub fn JSValueIsInstanceOfConstructor(
        ctx: JSContextRef,
        value: JSValueRef,
        constructor: JSObjectRef,
        exception: *mut JSValueRef,
    ) -> bool;
    pub fn JSValueMakeUndefined(ctx: JSContextRef) -> JSValueRef;
    pub fn JSValueMakeNull(ctx: JSContextRef) -> JSValueRef;
    pub fn JSValueMakeBoolean(ctx: JSContextRef, boolean: bool) -> JSValueRef;
    pub fn JSValueMakeNumber(ctx: JSContextRef, number: f64) -> JSValueRef;
    pub fn JSValueMakeString(ctx: JSContextRef, string: JSStringRef) -> JSValueRef;
    pub fn JSValueMakeFromJSONString(ctx: JSContextRef, string: JSStringRef) -> JSValueRef;
    pub fn JSValueCreateJSONString(
        ctx: JSContextRef,
        value: JSValueRef,
        indent: c_uint,
        exception: *mut JSValueRef,
    ) -> JSStringRef;
    pub fn JSValueToBoolean(ctx: JSContextRef, value: JSValueRef) -> bool;
    pub fn JSValueToNumber(ctx: JSContextRef, value: JSValueRef, exception: *mut JSValueRef) -> f64;
    pub fn JSValueToStringCopy(ctx: JSContextRef, value: JSValueRef, exception: *mut JSValueRef) -> JSStringRef;
    pub fn JSValueToObject(ctx: JSContextRef, value: JSValueRef, exception: *mut JSValueRef) -> JSObjectRef;
    pub fn JSValueProtect(ctx: JSContextRef, value: JSValueRef);
    pub fn JSValueUnprotect(ctx: JSContextRef, value: JSValueRef);

    // ─── JSObjectRef ───
    pub fn JSClassCreate(definition: *const JSClassDefinition) -> JSClassRef;
    pub fn JSClassRetain(class: JSClassRef) -> JSClassRef;
    pub fn JSClassRelease(class: JSClassRef);
    pub fn JSObjectMake(ctx: JSContextRef, class: JSClassRef, data: *mut c_void) -> JSObjectRef;
    pub fn JSObjectMakeFunctionWithCallback(
        ctx: JSContextRef,
        name: JSStringRef,
        call_as_function: JSObjectCallAsFunctionCallback,
    ) -> JSObjectRef;
    pub fn JSObjectMakeConstructor(
        ctx: JSContextRef,
        class: JSClassRef,
        call_as_constructor: JSObjectCallAsConstructorCallback,
    ) -> JSObjectRef;
    pub fn JSObjectMakeArray(
        ctx: JSContextRef,
        argument_count: usize,
        arguments: *const JSValueRef,
        exception: *mut JSValueRef,
    ) -> JSObjectRef;
    pub fn JSObjectMakeError(
        ctx: JSContextRef,
        argument_count: usize,
        arguments: *const JSValueRef,
        exception: *mut JSValueRef,
    ) -> JSObjectRef;
    pub fn JSObjectMakeDeferredPromise(
        ctx: JSContextRef,
        resolve: *mut JSObjectRef,
        reject: *mut JSObjectRef,
        exception: *mut JSValueRef,
    ) -> JSObjectRef;
    pub fn JSObjectGetPrototype(ctx: JSContextRef, object: JSObjectRef) -> JSValueRef;
    pub fn JSObjectSetPrototype(ctx: JSContextRef, object: JSObjectRef, value: JSValueRef);
    pub fn JSObjectHasProperty(ctx: JSContextRef, object: JSObjectRef, property_name: JSStringRef) -> bool;
    pub fn JSObjectGetProperty(
        ctx: JSContextRef,
        object: JSObjectRef,
        property_name: JSStringRef,
        exception: *mut JSValueRef,
    ) -> JSValueRef;
    pub fn JSObjectSetProperty(
        ctx: JSContextRef,
        object: JSObjectRef,
        property_name: JSStringRef,
        value: JSValueRef,
        attributes: JSPropertyAttributes,
        exception: *mut JSValueRef,
    );
    pub fn JSObjectDeleteProperty(
        ctx: JSContextRef,
        object: JSObjectRef,
        property_name: JSStringRef,
        exception: *mut JSValueRef,
    ) -> bool;
    pub fn JSObjectGetPropertyAtIndex(
        ctx: JSContextRef,
        object: JSObjectRef,
        property_index: c_uint,
        exception: *mut JSValueRef,
    ) -> JSValueRef;
    pub fn JSObjectSetPropertyAtIndex(
        ctx: JSContextRef,
        object: JSObjectRef,
        property_index: c_uint,
        value: JSValueRef,
        exception: *mut JSValueRef,
    );
    pub fn JSObjectGetPrivate(object: JSObjectRef) -> *mut c_void;
    pub fn JSObjectSetPrivate(object: JSObjectRef, data: *mut c_void) -> bool;
    pub fn JSObjectIsFunction(ctx: JSContextRef, object: JSObjectRef) -> bool;
    pub fn JSObjectCallAsFunction(
        ctx: JSContextRef,
        object: JSObjectRef,
        this_object: JSObjectRef,
        argument_count: usize,
        arguments: *const JSValueRef,
        exception: *mut JSValueRef,
    ) -> JSValueRef;
    pub fn JSObjectIsConstructor(ctx: JSContextRef, object: JSObjectRef) -> bool;
    pub fn JSObjectCallAsConstructor(
        ctx: JSContextRef,
        object: JSObjectRef,
        argument_count: usize,
        arguments: *const JSValueRef,
        exception: *mut JSValueRef,
    ) -> JSObjectRef;
    pub fn JSObjectCopyPropertyNames(ctx: JSContextRef, object: JSObjectRef) -> JSPropertyNameArrayRef;
    pub fn JSPropertyNameArrayRelease(array: JSPropertyNameArrayRef);
    pub fn JSPropertyNameArrayGetCount(array: JSPropertyNameArrayRef) -> usize;
    pub fn JSPropertyNameArrayGetNameAtIndex(array: JSPropertyNameArrayRef, index: usize) -> JSStringRef;


    // ─── property keys ───
    pub fn JSObjectHasPropertyForKey(ctx: JSContextRef, object: JSObjectRef, key: JSValueRef, exception: *mut JSValueRef) -> bool;
    pub fn JSObjectGetPropertyForKey(ctx: JSContextRef, object: JSObjectRef, key: JSValueRef, exception: *mut JSValueRef) -> JSValueRef;
    pub fn JSObjectSetPropertyForKey(
        ctx: JSContextRef,
        object: JSObjectRef,
        key: JSValueRef,
        value: JSValueRef,
        attributes: JSPropertyAttributes,
        exception: *mut JSValueRef,
    );
    pub fn JSObjectDeletePropertyForKey(ctx: JSContextRef, object: JSObjectRef, key: JSValueRef, exception: *mut JSValueRef) -> bool;

    // ─── symbols / bigint / dates ───
    pub fn JSValueMakeSymbol(ctx: JSContextRef, description: JSStringRef) -> JSValueRef;
    pub fn JSValueIsSymbol(ctx: JSContextRef, value: JSValueRef) -> bool;
    pub fn JSValueIsBigInt(ctx: JSContextRef, value: JSValueRef) -> bool;
    pub fn JSValueIsDate(ctx: JSContextRef, value: JSValueRef) -> bool;
    pub fn JSBigIntCreateWithInt64(ctx: JSContextRef, integer: i64, exception: *mut JSValueRef) -> JSValueRef;
    pub fn JSBigIntCreateWithUInt64(ctx: JSContextRef, integer: u64, exception: *mut JSValueRef) -> JSValueRef;
    pub fn JSBigIntCreateWithDouble(ctx: JSContextRef, value: f64, exception: *mut JSValueRef) -> JSValueRef;
    pub fn JSValueToInt64(ctx: JSContextRef, value: JSValueRef, exception: *mut JSValueRef) -> i64;
    pub fn JSValueToUInt64(ctx: JSContextRef, value: JSValueRef, exception: *mut JSValueRef) -> u64;
    pub fn JSValueToInt32(ctx: JSContextRef, value: JSValueRef, exception: *mut JSValueRef) -> i32;
    pub fn JSObjectMakeDate(ctx: JSContextRef, argument_count: usize, arguments: *const JSValueRef, exception: *mut JSValueRef) -> JSObjectRef;

    // ─── typed arrays / array buffers ───
    pub fn JSValueGetTypedArrayType(ctx: JSContextRef, value: JSValueRef, exception: *mut JSValueRef) -> JSTypedArrayType;
    pub fn JSObjectMakeTypedArray(ctx: JSContextRef, array_type: JSTypedArrayType, length: usize, exception: *mut JSValueRef) -> JSObjectRef;
    pub fn JSObjectMakeTypedArrayWithArrayBuffer(
        ctx: JSContextRef,
        array_type: JSTypedArrayType,
        buffer: JSObjectRef,
        exception: *mut JSValueRef,
    ) -> JSObjectRef;
    pub fn JSObjectGetTypedArrayBytesPtr(ctx: JSContextRef, object: JSObjectRef, exception: *mut JSValueRef) -> *mut c_void;
    pub fn JSObjectGetTypedArrayLength(ctx: JSContextRef, object: JSObjectRef, exception: *mut JSValueRef) -> usize;
    pub fn JSObjectGetTypedArrayByteLength(ctx: JSContextRef, object: JSObjectRef, exception: *mut JSValueRef) -> usize;
    pub fn JSObjectGetTypedArrayByteOffset(ctx: JSContextRef, object: JSObjectRef, exception: *mut JSValueRef) -> usize;
    pub fn JSObjectGetTypedArrayBuffer(ctx: JSContextRef, object: JSObjectRef, exception: *mut JSValueRef) -> JSObjectRef;
    pub fn JSObjectMakeArrayBufferWithBytesNoCopy(
        ctx: JSContextRef,
        bytes: *mut c_void,
        byte_length: usize,
        deallocator: JSTypedArrayBytesDeallocator,
        deallocator_context: *mut c_void,
        exception: *mut JSValueRef,
    ) -> JSObjectRef;
    pub fn JSObjectGetArrayBufferBytesPtr(ctx: JSContextRef, object: JSObjectRef, exception: *mut JSValueRef) -> *mut c_void;
    pub fn JSObjectGetArrayBufferByteLength(ctx: JSContextRef, object: JSObjectRef, exception: *mut JSValueRef) -> usize;

    pub fn bun_embed_vm_delete_module_registry_entry(vm: *mut c_void, specifier_ptr: *const u8, specifier_len: usize) -> bool;

    // ─── Bun embedding (com/github/oven-sh/bun/dist/src/runtime/embed.rs) ───
    pub fn bun_embed_run_internal_process_mode();
    pub fn bun_embed_init(argc: c_int, argv: *const *const c_char, install_crash_handler: bool);
    pub fn bun_embed_vm_create(cwd_ptr: *const u8, cwd_len: usize) -> *mut c_void;
    pub fn bun_embed_test_vm_create(cwd_ptr: *const u8, cwd_len: usize) -> *mut c_void;
    pub fn bun_embed_test_run_file(
        vm: *mut c_void,
        path_ptr: *const u8,
        path_len: usize,
        out_result: *mut BunEmbedTestResult,
    ) -> c_int;
    pub fn bun_embed_vm_global_object(vm: *mut c_void) -> *mut c_void;
    pub fn bun_embed_vm_configure_entrypoint(
        vm: *mut c_void,
        main_ptr: *const u8,
        main_len: usize,
        argv_ptrs: *const *const u8,
        argv_lens: *const usize,
        argc: usize,
    ) -> bool;
    pub fn bun_embed_vm_run_eval(vm: *mut c_void, source_ptr: *const u8, source_len: usize) -> c_int;
    pub fn bun_embed_vm_tick(vm: *mut c_void);
    pub fn bun_embed_vm_drain_microtasks(vm: *mut c_void);
    pub fn bun_embed_vm_is_event_loop_alive(vm: *mut c_void) -> bool;
    pub fn bun_embed_vm_auto_tick_active(vm: *mut c_void);
    pub fn bun_embed_vm_run_until_idle(vm: *mut c_void);
    pub fn bun_embed_vm_finish_process(vm: *mut c_void) -> !;
    pub fn bun_embed_vm_wait_for_promise(vm: *mut c_void, promise: usize) -> c_int;
    pub fn bun_embed_promise_status(promise: usize) -> c_int;
    pub fn bun_embed_promise_result(vm: *mut c_void, promise: usize) -> usize;
    pub fn bun_embed_promise_set_handled(vm: *mut c_void, promise: usize);
    pub fn bun_embed_vm_garbage_collect(vm: *mut c_void) -> usize;
    pub fn bun_embed_vm_wakeup(vm: *mut c_void);
    pub fn bun_embed_last_error(out_len: *mut usize) -> *const u8;
}

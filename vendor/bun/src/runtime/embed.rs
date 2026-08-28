//! Embedding entry points for hosting Bun's JavaScript VM inside another
//! process (the Chord `rbun` experiment; see `crates/rbun` in Chord).
//!
//! This mirrors the process-level init done by `bin_entry::main` and the VM
//! boot done by `cli::run_command::Run::boot`, minus everything that is
//! CLI-specific (argument parsing, bunfig, `Global::exit` on failure, the
//! run-to-completion event loop), and exposes a very small C ABI on top of
//! it. Everything value-level is done by the host through JavaScriptCore's
//! public C API (`<JavaScriptCore/JavaScript.h>`): a `JSValueRef` is
//! bit-identical to [`bun_jsc::JSValue`] on 64-bit targets and a
//! `JSGlobalObject*` *is* a `JSGlobalContextRef` (see `APICast.h`), so no
//! extra bridging is required beyond what is exported here.
//!
//! Threading contract: `bun_embed_init` runs once per process; every other
//! function must be called from the single thread that created the VM, which
//! holds the JSC API lock for its whole lifetime (like the CLI's `Run::start`).

#![allow(clippy::missing_safety_doc)]

use core::cell::RefCell;
use core::ffi::{c_char, c_int, c_void};
use std::sync::Once;

use bun_core::{StackCheck, output};
use bun_jsc::js_promise::Status as PromiseStatus;
use bun_jsc::virtual_machine::{InitOptions, VirtualMachine};
use bun_jsc::{JSGlobalObject, JSValue};
use bun_options_types::schema::api;

static PROCESS_INIT: Once = Once::new();

thread_local! {
    static LAST_ERROR: RefCell<String> = const { RefCell::new(String::new()) };
}

fn set_last_error(message: String) {
    LAST_ERROR.with(|cell| *cell.borrow_mut() = message);
}

#[inline]
unsafe fn vm_mut<'a>(vm: *mut c_void) -> &'a mut VirtualMachine {
    debug_assert!(!vm.is_null());
    // SAFETY: `vm` was returned by `bun_embed_vm_create` on this thread and is
    // never freed (the embedded VM is process-lifetime, like the CLI's).
    unsafe { &mut *vm.cast::<VirtualMachine>() }
}

/// Process-wide initialisation. Idempotent; safe to call from any thread, but
/// the thread that calls it first should be the future JS thread.
///
/// * `argc`/`argv`: what `process.argv` / `Bun.argv` will report. The strings
///   must live for the whole process.
/// * `install_crash_handler`: install Bun's SIGSEGV/SIGBUS/… handlers and its
///   Rust panic hook (which prints a Bun-style crash report). Hosts that have
///   their own crash reporting should pass `false`.
///
/// # Safety
/// `argv` must point to `argc` NUL-terminated C strings that live for the
/// process lifetime.
#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn bun_embed_init(
    argc: c_int,
    argv: *const *const c_char,
    install_crash_handler: bool,
) {
    PROCESS_INIT.call_once(|| {
        // SAFETY: forwarded verbatim from the caller, who upholds the contract
        // documented above (same contract as the C runtime's `main`).
        unsafe { bun_core::init_argv(argc, argv) };
        bun_crash_handler::cli_state::set_main_thread_id(bun_threading::current_thread_id());
        bun_core::set_start_time(bun_core::time::nano_timestamp());

        if install_crash_handler {
            bun_crash_handler::init();
        }

        // Bun's socket/subprocess code assumes SIGPIPE is ignored (same as
        // `bin_entry::main`).
        // SAFETY: valid signal number and disposition.
        #[cfg(unix)]
        unsafe {
            libc::signal(libc::SIGPIPE, libc::SIG_IGN);
        }

        // stdout/stderr writers + `bun_initialize_process()` (fd 0-2 sanity,
        // tty state).
        output::stdio::init();
        StackCheck::configure_thread();
    });
}

/// Create the (per-thread) VM. Returns null on failure; see
/// [`bun_embed_last_error`].
///
/// `cwd` is the working directory used for module resolution and
/// `process.cwd()`; it must be an absolute path.
///
/// After this returns the calling thread holds the JSC API lock for good, so
/// every JavaScriptCore C API call from this thread is already locked.
#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn bun_embed_vm_create(
    cwd_ptr: *const u8,
    cwd_len: usize,
) -> *mut c_void {
    if cwd_ptr.is_null() || cwd_len == 0 {
        set_last_error("bun_embed_vm_create: cwd must be a non-empty absolute path".into());
        return core::ptr::null_mut();
    }
    // SAFETY: caller passes a valid (ptr, len) pair.
    let cwd: Box<[u8]> = unsafe { core::slice::from_raw_parts(cwd_ptr, cwd_len) }.into();

    // Per-thread pieces of `bin_entry::main` / `Run::boot`.
    StackCheck::configure_thread();
    bun_jsc::initialize(bun_jsc::InitializeOptions::default());
    bun_ast::initialize_store_or_reset();

    let mut transform_options = api::TransformOptions::default();
    transform_options.absolute_working_dir = Some(cwd.clone());

    let vm_ptr = match VirtualMachine::init(InitOptions {
        transform_options,
        is_main_thread: true,
        ..Default::default()
    }) {
        Ok(vm) => vm,
        Err(err) => {
            set_last_error(format!("VirtualMachine::init failed: {err:?}"));
            return core::ptr::null_mut();
        }
    };
    // SAFETY: `init` returns the unique freshly-boxed VM on this thread.
    let vm = unsafe { &mut *vm_ptr };

    // Same env/define wiring as `Run::boot`, without the CLI's `ctx`.
    let defines_ok = {
        let b = &mut vm.transpiler;
        b.options.env.behavior = api::DotEnvBehavior::LoadAllWithoutInlining;
        b.configure_defines().is_ok()
    };
    if !defines_ok {
        set_last_error("bun_embed_vm_create: configure_defines failed".into());
        return core::ptr::null_mut();
    }

    // SAFETY: `vm.log` is set in `VirtualMachine::init`; `env_loader()` is the
    // long-lived loader owned by the transpiler.
    bun_http::async_http::load_env(unsafe { vm.log.unwrap().as_mut() }, vm.env_loader());
    vm.load_extra_env_and_source_code_printer();
    vm.is_main_thread = true;
    bun_jsc::virtual_machine::IS_MAIN_THREAD_VM.set(true);

    // `Bun.main` / relative resolution anchor. The file does not need to
    // exist; the host imports through the module loader itself.
    let mut main = Vec::with_capacity(cwd.len() + 16);
    main.extend_from_slice(&cwd);
    if !cwd.ends_with(b"/") {
        main.push(b'/');
    }
    main.extend_from_slice(b"[rbun-host].js");
    let main: &'static [u8] = Box::leak(main.into_boxed_slice());
    vm.set_main(main);

    // Hold the JSC API lock for the lifetime of this thread, exactly like
    // `Run::start` does for the CLI. Every JSC C API call re-enters it
    // recursively, so the host never has to lock explicitly.
    core::mem::forget(vm.global().vm().get_api_lock());

    vm_ptr.cast()
}

/// The VM's global object, usable directly as a `JSGlobalContextRef`.
#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn bun_embed_vm_global_object(vm: *mut c_void) -> *mut c_void {
    // SAFETY: see `vm_mut`.
    let vm = unsafe { vm_mut(vm) };
    let global: *mut JSGlobalObject = vm.global().as_ptr();
    global.cast()
}

/// Run one non-blocking event-loop tick (tasks, immediates, microtasks).
#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn bun_embed_vm_tick(vm: *mut c_void) {
    // SAFETY: see `vm_mut`.
    unsafe { vm_mut(vm) }.tick();
}

/// Drain the microtask queue only.
#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn bun_embed_vm_drain_microtasks(vm: *mut c_void) {
    // SAFETY: see `vm_mut`.
    unsafe { vm_mut(vm) }.drain_microtasks();
}

/// Whether anything (timers, sockets, pending tasks, …) keeps the loop alive.
#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn bun_embed_vm_is_event_loop_alive(vm: *mut c_void) -> bool {
    // SAFETY: see `vm_mut`.
    unsafe { vm_mut(vm) }.is_event_loop_alive()
}

/// Block in the I/O loop until the next event while there is something to
/// wait for (`auto_tick_active`), then process it. Returns immediately when
/// the loop is idle.
#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn bun_embed_vm_auto_tick_active(vm: *mut c_void) {
    // SAFETY: see `vm_mut`.
    unsafe { vm_mut(vm) }.auto_tick_active();
}

/// Run the event loop until nothing keeps it alive — the core of the CLI's
/// `Run::start` loop.
#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn bun_embed_vm_run_until_idle(vm: *mut c_void) {
    // SAFETY: see `vm_mut`.
    let vm = unsafe { vm_mut(vm) };
    while vm.is_event_loop_alive() {
        vm.tick();
        vm.auto_tick_active();
    }
}

/// Drive the event loop until `promise` settles. Returns 0 when settled, 1
/// when execution was stopped (termination / forbidden), 2 when the value is
/// not a promise.
#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn bun_embed_vm_wait_for_promise(
    vm: *mut c_void,
    promise: usize,
) -> c_int {
    // SAFETY: see `vm_mut`.
    let vm = unsafe { vm_mut(vm) };
    let Some(promise) = JSValue::from_encoded(promise).as_any_promise() else {
        return 2;
    };
    match vm.wait_for_promise(promise) {
        Ok(()) => 0,
        Err(_stopped) => 1,
    }
}

/// -1 when not a promise, otherwise 0 pending / 1 fulfilled / 2 rejected.
#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn bun_embed_promise_status(promise: usize) -> c_int {
    let Some(promise) = JSValue::from_encoded(promise).as_any_promise() else {
        return -1;
    };
    match promise.status() {
        PromiseStatus::Pending => 0,
        PromiseStatus::Fulfilled => 1,
        PromiseStatus::Rejected => 2,
    }
}

/// The settled value (fulfillment value or rejection reason) of a promise.
/// Undefined for pending promises / non-promises.
#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn bun_embed_promise_result(vm: *mut c_void, promise: usize) -> usize {
    // SAFETY: see `vm_mut`.
    let vm = unsafe { vm_mut(vm) };
    let Some(promise) = JSValue::from_encoded(promise).as_any_promise() else {
        return JSValue::UNDEFINED.encoded();
    };
    if promise.status() == PromiseStatus::Pending {
        return JSValue::UNDEFINED.encoded();
    }
    promise.result(vm.jsc_vm()).encoded()
}

/// Mark a rejected promise as handled so it is not reported as an unhandled
/// rejection.
#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn bun_embed_promise_set_handled(vm: *mut c_void, promise: usize) {
    // SAFETY: see `vm_mut`.
    let vm = unsafe { vm_mut(vm) };
    if let Some(promise) = JSValue::from_encoded(promise).as_any_promise() {
        promise.set_handled(vm.jsc_vm());
    }
}

/// Wake the I/O loop from another thread so a blocked
/// `bun_embed_vm_auto_tick_active` returns. Thread-safe; the only embed
/// function that may be called off the VM thread.
#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn bun_embed_vm_wakeup(vm: *mut c_void) {
    // SAFETY: `vm` is the live process-lifetime VM; `EventLoop::wakeup` only
    // touches the uSockets loop's thread-safe wakeup handle.
    unsafe { &*vm.cast::<VirtualMachine>() }.event_loop_shared().wakeup();
}

/// Evict a module from the module loader registry so the next import of
/// `specifier` re-resolves and re-evaluates it (used when a host re-declares
/// a module under the same name). Returns whether an entry was removed.
#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn bun_embed_vm_delete_module_registry_entry(
    vm: *mut c_void,
    specifier_ptr: *const u8,
    specifier_len: usize,
) -> bool {
    // SAFETY: see `vm_mut`.
    let vm = unsafe { vm_mut(vm) };
    // SAFETY: caller passes a valid (ptr, len) pair.
    let specifier = unsafe { core::slice::from_raw_parts(specifier_ptr, specifier_len) };
    let name = bun_core::EncodedSlice::from_bytes(specifier);
    vm.global().delete_module_registry_entry(&name).is_ok()
}

/// Run a synchronous GC.
#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn bun_embed_vm_garbage_collect(vm: *mut c_void) -> usize {
    // SAFETY: see `vm_mut`.
    unsafe { vm_mut(vm) }.garbage_collect(true)
}

/// The last error message recorded on this thread (UTF-8, not NUL-terminated;
/// length via `out_len`). Valid until the next embed call on this thread.
#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn bun_embed_last_error(out_len: *mut usize) -> *const u8 {
    LAST_ERROR.with(|cell| {
        let message = cell.borrow();
        if !out_len.is_null() {
            // SAFETY: caller passes a valid out pointer.
            unsafe { *out_len = message.len() };
        }
        message.as_ptr()
    })
}

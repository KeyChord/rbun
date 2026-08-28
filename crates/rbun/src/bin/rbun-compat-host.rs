//! One-shot process host for the differential Bun compatibility suite.
//!
//! Each invocation creates a fresh embedded VM, imports one fixture, waits for
//! its top-level evaluation and detached event-loop work, then mirrors
//! `process.exitCode`. Keeping this host deliberately small makes the
//! comparison exercise rbun's production initialization and module paths.

use std::path::PathBuf;
use std::thread;

use rbun::{Context, Module, Object, Runtime, RuntimeOptions};

const JS_STACK_SIZE: usize = 16 * 1024 * 1024;

fn main() {
    rbun::run_internal_process_mode();

    let mut args = std::env::args_os();
    let program = args.next().unwrap_or_default();
    let Some(fixture) = args.next() else {
        eprintln!("usage: {} <fixture>", PathBuf::from(program).display());
        std::process::exit(2);
    };
    if args.next().is_some() {
        eprintln!("rbun-compat-host: expected exactly one fixture path");
        std::process::exit(2);
    }

    let fixture = match PathBuf::from(fixture).canonicalize() {
        Ok(path) => path,
        Err(error) => {
            eprintln!("rbun-compat-host: cannot resolve fixture: {error}");
            std::process::exit(2);
        }
    };
    let cwd = match std::env::current_dir().and_then(|path| path.canonicalize()) {
        Ok(path) => path,
        Err(error) => {
            eprintln!("rbun-compat-host: cannot resolve current directory: {error}");
            std::process::exit(2);
        }
    };

    let worker = thread::Builder::new()
        .name("rbun-compat-js".into())
        .stack_size(JS_STACK_SIZE)
        .spawn(move || run_fixture(fixture, cwd));

    let result = match worker {
        Ok(worker) => match worker.join() {
            Ok(result) => result,
            Err(_) => Err("the JavaScript thread panicked".to_string()),
        },
        Err(error) => Err(format!("cannot start the JavaScript thread: {error}")),
    };

    match result {
        Ok(code) => std::process::exit(code),
        Err(error) => {
            eprintln!("rbun-compat-host: {error}");
            std::process::exit(1);
        }
    }
}

fn run_fixture(fixture: PathBuf, cwd: PathBuf) -> Result<i32, String> {
    let runtime = Runtime::new_with(RuntimeOptions {
        cwd,
        argv: None,
        install_crash_handler: false,
    })
    .map_err(|error| error.to_string())?;
    let context = Context::full(&runtime).map_err(|error| error.to_string())?;
    let specifier = fixture.to_string_lossy().into_owned();

    context.with(|ctx| {
        let result = (|| -> rbun::Result<i32> {
            let _: Object = Module::import(&ctx, &specifier)?.finish()?;
            ctx.run_until_idle();
            let exit_code: i32 =
                ctx.eval("process.exitCode == null ? 0 : Math.trunc(Number(process.exitCode))")?;
            Ok(exit_code.clamp(0, 255))
        })();

        result.map_err(|error| rbun::format_error(&ctx, error))
    })
}

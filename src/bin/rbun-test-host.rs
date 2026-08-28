//! One-shot host for Bun's upstream runtime tests.
//!
//! `--rbun-test-file` enters the native embedded `bun:test` runner. Direct
//! script and `-e` forms enter the ordinary runtime so upstream tests that
//! spawn `bunExe()` continue to exercise rbun instead of the reference Bun
//! executable. Package-manager/build commands are intentionally unsupported.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::thread;

use rbun::{Context, Module, Object, Runtime, RuntimeOptions};

const JS_STACK_SIZE: usize = 16 * 1024 * 1024;

enum Invocation {
    Test(PathBuf),
    Eval(String, Vec<OsString>),
    Run(PathBuf, Vec<OsString>),
}

fn main() {
    rbun::run_internal_process_mode();

    let invocation = match parse_invocation() {
        Ok(invocation) => invocation,
        Err(message) => {
            eprintln!("rbun-test-host: {message}");
            std::process::exit(2);
        }
    };
    let cwd = match canonical_cwd() {
        Ok(cwd) => cwd,
        Err(error) => {
            eprintln!("rbun-test-host: {error}");
            std::process::exit(2);
        }
    };

    let worker = thread::Builder::new()
        .name("rbun-upstream-test-js".into())
        .stack_size(JS_STACK_SIZE)
        .spawn(move || run(invocation, cwd));
    let result = match worker {
        Ok(worker) => worker
            .join()
            .unwrap_or_else(|_| Err("the JavaScript thread panicked".into())),
        Err(error) => Err(format!("cannot start the JavaScript thread: {error}")),
    };
    match result {
        Ok(code) => std::process::exit(code),
        Err(error) => {
            eprintln!("rbun-test-host: {error}");
            std::process::exit(1);
        }
    }
}

fn parse_invocation() -> Result<Invocation, String> {
    let mut args = std::env::args_os();
    let _program = args.next();
    let first = args.next().ok_or_else(|| {
        "usage: rbun-test-host --rbun-test-file <file> | -e <code> | <script>".to_string()
    })?;
    if first == "--rbun-test-file" {
        let file = args
            .next()
            .ok_or_else(|| "--rbun-test-file requires a path".to_string())?;
        if args.next().is_some() {
            return Err("--rbun-test-file accepts exactly one path".into());
        }
        return Ok(Invocation::Test(canonical_file(file)?));
    }
    if first == "-e" || first == "--eval" {
        let code = args
            .next()
            .ok_or_else(|| "-e/--eval requires JavaScript source".to_string())?
            .to_string_lossy()
            .into_owned();
        return Ok(Invocation::Eval(code, args.collect()));
    }
    if first == "test"
        || first == "install"
        || first == "i"
        || first == "add"
        || first == "update"
        || first == "pm"
        || first == "build"
        || first == "x"
        || first == "create"
    {
        return Err(format!(
            "Bun CLI subcommand {:?} is outside the embedded runtime host",
            first
        ));
    }
    Ok(Invocation::Run(canonical_file(first)?, args.collect()))
}

fn canonical_file(path: impl AsRef<Path>) -> Result<PathBuf, String> {
    path.as_ref()
        .canonicalize()
        .map_err(|error| format!("cannot resolve {}: {error}", path.as_ref().display()))
}

fn canonical_cwd() -> Result<PathBuf, String> {
    std::env::current_dir()
        .and_then(|path| path.canonicalize())
        .map_err(|error| format!("cannot resolve current directory: {error}"))
}

fn options(cwd: PathBuf) -> RuntimeOptions {
    RuntimeOptions {
        cwd,
        argv: None,
        install_crash_handler: false,
    }
}

fn run(invocation: Invocation, cwd: PathBuf) -> Result<i32, String> {
    match invocation {
        Invocation::Test(file) => {
            let runtime =
                Runtime::new_test_with(options(cwd)).map_err(|error| error.to_string())?;
            let _context = Context::full(&runtime).map_err(|error| error.to_string())?;
            let result = runtime
                .run_test_file(file)
                .map_err(|error| error.to_string())?;
            if std::env::var_os("RBUN_TEST_RESULT_JSON").is_some() {
                eprintln!(
                    "RBUN_TEST_RESULT {}",
                    serde_json::json!({
                        "pass": result.pass,
                        "fail": result.fail,
                        "skip": result.skip,
                        "todo": result.todo,
                        "expectations": result.expectations,
                        "files": result.files,
                        "unhandledErrors": result.unhandled_errors,
                    })
                );
            }
            Ok(i32::from(!result.passed()))
        }
        Invocation::Eval(code, arguments) => {
            let entrypoint = cwd.join("[eval]");
            run_eval(cwd, &entrypoint, arguments, code)
        }
        Invocation::Run(file, arguments) => {
            let specifier = file.to_string_lossy().into_owned();
            run_normal(cwd, &file, arguments, |ctx| {
                let _: Object = Module::import(ctx, &specifier)?.finish()?;
                Ok(())
            })
        }
    }
}

fn run_eval(
    cwd: PathBuf,
    entrypoint: &Path,
    arguments: Vec<OsString>,
    code: String,
) -> Result<i32, String> {
    let runtime = Runtime::new_with(options(cwd)).map_err(|error| error.to_string())?;
    runtime
        .configure_entrypoint(entrypoint, arguments)
        .map_err(|error| error.to_string())?;
    let _context = Context::full(&runtime).map_err(|error| error.to_string())?;
    runtime
        .run_eval_source(code)
        .map_err(|error| error.to_string())?;
    runtime.finish_process()
}

fn run_normal(
    cwd: PathBuf,
    entrypoint: &Path,
    arguments: Vec<OsString>,
    action: impl for<'js> FnOnce(&rbun::Ctx<'js>) -> rbun::Result<()>,
) -> Result<i32, String> {
    let runtime = Runtime::new_with(options(cwd)).map_err(|error| error.to_string())?;
    runtime
        .configure_entrypoint(entrypoint, arguments)
        .map_err(|error| error.to_string())?;
    let context = Context::full(&runtime).map_err(|error| error.to_string())?;
    context.with(|ctx| {
        let result = (|| -> rbun::Result<()> {
            action(&ctx)?;
            Ok(())
        })();
        result.map_err(|error| rbun::format_error(&ctx, error))
    })?;
    runtime.finish_process()
}

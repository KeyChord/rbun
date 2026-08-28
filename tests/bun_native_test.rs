use std::path::PathBuf;
use std::process::Command;

#[test]
fn embedded_native_bun_test_runner() {
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let host = PathBuf::from(env!("CARGO_BIN_EXE_rbun-test-host"));
    let fixtures = repo.join("tests/fixtures/bun_test");

    let pass = Command::new(&host)
        .args(["--rbun-test-file", "pass.test.ts"])
        .current_dir(&fixtures)
        .env("NO_COLOR", "1")
        .env("RBUN_TEST_RESULT_JSON", "1")
        .output()
        .expect("run passing embedded bun:test fixture");
    assert!(
        pass.status.success(),
        "passing fixture failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&pass.stdout),
        String::from_utf8_lossy(&pass.stderr)
    );
    let pass_stderr = String::from_utf8_lossy(&pass.stderr);
    assert!(pass_stderr.contains("\"pass\":2"), "{pass_stderr}");
    assert!(pass_stderr.contains("\"fail\":0"), "{pass_stderr}");

    let fail = Command::new(&host)
        .args(["--rbun-test-file", "fail.test.ts"])
        .current_dir(&fixtures)
        .env("NO_COLOR", "1")
        .env("RBUN_TEST_RESULT_JSON", "1")
        .output()
        .expect("run failing embedded bun:test fixture");
    assert_eq!(fail.status.code(), Some(1));
    let fail_stderr = String::from_utf8_lossy(&fail.stderr);
    assert!(fail_stderr.contains("\"pass\":0"), "{fail_stderr}");
    assert!(fail_stderr.contains("\"fail\":1"), "{fail_stderr}");
    assert!(
        fail_stderr.contains("reports native assertion failures"),
        "{fail_stderr}"
    );
}

#[test]
fn one_shot_runtime_host_matches_bun_process_identity_and_lifecycle() {
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let host = PathBuf::from(env!("CARGO_BIN_EXE_rbun-test-host"));
    let bun = repo.join("vendor/bun/build/release/bun");
    let fixture = repo.join("tests/fixtures/runtime_host/identity.ts");

    let reference = Command::new(&bun)
        .arg(&fixture)
        .args(["alpha", "beta"])
        .env("NO_COLOR", "1")
        .output()
        .expect("run Bun identity fixture");
    let candidate = Command::new(&host)
        .arg(&fixture)
        .args(["alpha", "beta"])
        .env("NO_COLOR", "1")
        .output()
        .expect("run rbun identity fixture");
    assert_eq!(candidate.status.code(), reference.status.code());
    assert_eq!(candidate.stdout, reference.stdout);
    assert_eq!(candidate.stderr, reference.stderr);

    let eval = r#"
console.log(JSON.stringify({
  importMetaMain: import.meta.main,
  bunMainIsEval: Bun.main.endsWith("/[eval]"),
  arguments: process.argv.slice(1),
  execArgvFlag: process.execArgv[0],
}));
process.on("beforeExit", code => console.log(`beforeExit:${code}`));
process.on("exit", code => console.log(`exit:${code}`));
"#;
    let reference = Command::new(&bun)
        .args(["-e", eval, "alpha", "beta"])
        .env("NO_COLOR", "1")
        .output()
        .expect("run Bun eval identity fixture");
    let candidate = Command::new(&host)
        .args(["-e", eval, "alpha", "beta"])
        .env("NO_COLOR", "1")
        .output()
        .expect("run rbun eval identity fixture");
    assert_eq!(candidate.status.code(), reference.status.code());
    assert_eq!(candidate.stdout, reference.stdout);
    assert_eq!(candidate.stderr, reference.stderr);

    let rejected_eval = r#"
process.on("beforeExit", code => console.log(`beforeExit:${code}`));
process.on("exit", code => console.log(`exit:${code}`));
throw new Error("expected rejected eval");
"#;
    let reference = Command::new(&bun)
        .args(["-e", rejected_eval])
        .env("NO_COLOR", "1")
        .output()
        .expect("run rejected Bun eval fixture");
    let candidate = Command::new(&host)
        .args(["-e", rejected_eval])
        .env("NO_COLOR", "1")
        .output()
        .expect("run rejected rbun eval fixture");
    assert_eq!(reference.status.code(), Some(1));
    assert_eq!(candidate.status.code(), reference.status.code());
    assert_eq!(candidate.stdout, reference.stdout);
    assert!(!String::from_utf8_lossy(&candidate.stdout).contains("beforeExit"));
}

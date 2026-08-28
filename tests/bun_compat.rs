//! Differential compatibility tests: run identical fixtures under the
//! same-commit Bun executable and rbun, then compare every observable result.

use std::collections::BTreeSet;
use std::ffi::OsStr;
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Component, Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

const OBSERVATION_FIELDS: [&str; 4] = ["status", "stdout", "stderr", "tree"];
const DEFAULT_TIMEOUT_MS: u64 = 20_000;

#[derive(Debug)]
struct Case {
    name: String,
    source_dir: PathBuf,
    entry: PathBuf,
    timeout: Duration,
    expected_status: i32,
}

#[test]
fn bun_runtime_compatibility() {
    if let Err(error) = run_suite() {
        panic!("{error}");
    }
}

fn run_suite() -> Result<(), String> {
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let reference = std::env::var_os("RBUN_REFERENCE_BUN")
        .map(PathBuf::from)
        .unwrap_or_else(|| repo.join("vendor/bun/build/release/bun"));
    let candidate = std::env::var_os("RBUN_COMPAT_HOST")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_BIN_EXE_rbun-compat-host")));
    let driver = repo.join("compat/reference-driver.mjs");
    let filter = std::env::var("RBUN_COMPAT_FILTER").ok();

    require_file(&reference, "reference Bun executable")?;
    require_file(&candidate, "rbun compatibility host")?;
    require_file(&driver, "reference import driver")?;
    verify_reference_revision(&reference, &repo)?;

    let expected_path = repo.join("compat/expected-deviations.json");
    let expected_document: Value = serde_json::from_slice(
        &fs::read(&expected_path)
            .map_err(|error| format!("read {}: {error}", expected_path.display()))?,
    )
    .map_err(|error| format!("parse {}: {error}", expected_path.display()))?;
    if expected_document.get("$schema").and_then(Value::as_u64) != Some(1) {
        return Err(format!(
            "{} must declare $schema: 1",
            expected_path.display()
        ));
    }
    let expected_cases = expected_document
        .get("cases")
        .and_then(Value::as_object)
        .ok_or_else(|| format!("{} must contain a cases object", expected_path.display()))?;

    let mut cases = discover_cases(&repo.join("compat/fixtures"))?;
    if let Some(filter) = &filter {
        cases.retain(|case| case.name.contains(filter));
    }
    if cases.is_empty() {
        return Err(match filter {
            Some(filter) => format!("no compatibility cases matched RBUN_COMPAT_FILTER={filter:?}"),
            None => "no compatibility cases found".into(),
        });
    }

    println!("reference: {}", reference.display());
    println!("candidate: {}", candidate.display());
    println!("cases: {}", cases.len());

    let mut failures = Vec::new();
    let mut seen_expected = BTreeSet::new();
    for case in &cases {
        let (reference_observation, candidate_observation) =
            match run_case(case, &reference, &candidate, &driver) {
                Ok(observations) => observations,
                Err(error) => {
                    failures.push(format!("{}: {error}", case.name));
                    continue;
                }
            };

        let expected_status = format!("code:{}", case.expected_status);
        if reference_observation.get("status").and_then(Value::as_str)
            != Some(expected_status.as_str())
        {
            failures.push(format!(
                "{}: reference fixture did not finish with expected status {expected_status}\nreference:\n{}",
                case.name,
                pretty(&reference_observation)
            ));
            continue;
        }

        let expected = expected_cases.get(&case.name);
        if expected.is_some() {
            seen_expected.insert(case.name.clone());
        }
        match compare_case(
            case,
            &reference_observation,
            &candidate_observation,
            expected,
        ) {
            Ok(Comparison::Equal) => println!("PASS  {}", case.name),
            Ok(Comparison::Expected(reason)) => {
                println!("XDIFF {} — {reason}", case.name)
            }
            Err(error) => failures.push(error),
        }
    }

    if filter.is_none() {
        for name in expected_cases.keys() {
            if !seen_expected.contains(name) {
                failures.push(format!(
                    "expected-deviation entry {name:?} has no corresponding compatibility case"
                ));
            }
        }
    }

    if failures.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "{} compatibility failure(s):\n\n{}",
            failures.len(),
            failures.join("\n\n")
        ))
    }
}

enum Comparison {
    Equal,
    Expected(String),
}

fn compare_case(
    case: &Case,
    reference: &Value,
    candidate: &Value,
    expected: Option<&Value>,
) -> Result<Comparison, String> {
    let differences: Vec<&str> = OBSERVATION_FIELDS
        .iter()
        .copied()
        .filter(|field| reference.get(field) != candidate.get(field))
        .collect();

    if differences.is_empty() {
        return match expected {
            Some(_) => Err(format!(
                "{}: XPASS — reference and candidate now agree; remove its expected deviation",
                case.name
            )),
            None => Ok(Comparison::Equal),
        };
    }

    let Some(expected) = expected else {
        return Err(format!(
            "{}: unexpected difference in {}\nreference:\n{}\ncandidate:\n{}",
            case.name,
            differences.join(", "),
            pretty(reference),
            pretty(candidate)
        ));
    };
    let reason = expected
        .get("reason")
        .and_then(Value::as_str)
        .filter(|reason| !reason.trim().is_empty())
        .ok_or_else(|| format!("{}: expected deviation needs a non-empty reason", case.name))?;
    let fields = expected
        .get("fields")
        .and_then(Value::as_object)
        .ok_or_else(|| format!("{}: expected deviation needs a fields object", case.name))?;
    let actual_fields: BTreeSet<&str> = differences.iter().copied().collect();
    let expected_fields: BTreeSet<&str> = fields.keys().map(String::as_str).collect();
    if actual_fields != expected_fields {
        return Err(format!(
            "{}: expected fields {:?}, observed {:?}\nreference:\n{}\ncandidate:\n{}",
            case.name,
            expected_fields,
            actual_fields,
            pretty(reference),
            pretty(candidate)
        ));
    }

    for field in differences {
        let snapshots = fields
            .get(field)
            .and_then(Value::as_object)
            .ok_or_else(|| format!("{}: expected field {field:?} must be an object", case.name))?;
        let expected_reference = snapshots
            .get("reference")
            .ok_or_else(|| format!("{}: expected field {field:?} needs reference", case.name))?;
        let expected_candidate = snapshots
            .get("candidate")
            .ok_or_else(|| format!("{}: expected field {field:?} needs candidate", case.name))?;
        if reference.get(field) != Some(expected_reference)
            || candidate.get(field) != Some(expected_candidate)
        {
            return Err(format!(
                "{}: expected deviation in {field:?} changed\nexpected reference: {}\nactual reference: {}\nexpected candidate: {}\nactual candidate: {}",
                case.name,
                pretty(expected_reference),
                pretty(reference.get(field).unwrap()),
                pretty(expected_candidate),
                pretty(candidate.get(field).unwrap())
            ));
        }
    }

    Ok(Comparison::Expected(reason.to_string()))
}

fn run_case(
    case: &Case,
    reference: &Path,
    candidate: &Path,
    driver: &Path,
) -> Result<(Value, Value), String> {
    let reference_temp = tempfile::Builder::new()
        .prefix("rbun-compat-reference-")
        .tempdir()
        .map_err(|error| format!("create reference temp directory: {error}"))?;
    let candidate_temp = tempfile::Builder::new()
        .prefix("rbun-compat-candidate-")
        .tempdir()
        .map_err(|error| format!("create candidate temp directory: {error}"))?;
    let reference_root = reference_temp.path().join("case");
    let candidate_root = candidate_temp.path().join("case");
    copy_tree(&case.source_dir, &reference_root)?;
    copy_tree(&case.source_dir, &candidate_root)?;

    let reference_entry = reference_root.join(&case.entry);
    let candidate_entry = candidate_root.join(&case.entry);
    let reference_args = [driver.as_os_str(), reference_entry.as_os_str()];
    let candidate_args = [candidate_entry.as_os_str()];

    let reference_observation =
        observe_process(reference, &reference_args, &reference_root, case.timeout)?;
    let candidate_observation =
        observe_process(candidate, &candidate_args, &candidate_root, case.timeout)?;
    Ok((reference_observation, candidate_observation))
}

fn observe_process(
    executable: &Path,
    args: &[&OsStr],
    root: &Path,
    timeout: Duration,
) -> Result<Value, String> {
    let mut stdout_file =
        tempfile::tempfile().map_err(|error| format!("create stdout capture: {error}"))?;
    let mut stderr_file =
        tempfile::tempfile().map_err(|error| format!("create stderr capture: {error}"))?;
    let stdout_child = stdout_file
        .try_clone()
        .map_err(|error| format!("clone stdout capture: {error}"))?;
    let stderr_child = stderr_file
        .try_clone()
        .map_err(|error| format!("clone stderr capture: {error}"))?;

    let mut command = Command::new(executable);
    command
        .args(args)
        .current_dir(root)
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout_child))
        .stderr(Stdio::from(stderr_child))
        .env("RBUN_COMPAT_FIXED", "fixed-value")
        .env("NO_COLOR", "1")
        .env("FORCE_COLOR", "0")
        .env("TZ", "Etc/UTC")
        .env("LANG", "C")
        .env("LC_ALL", "C")
        .env("TERM", "dumb");
    let mut child = command
        .spawn()
        .map_err(|error| format!("spawn {}: {error}", executable.display()))?;
    let started = Instant::now();
    let (status, timed_out) = loop {
        match child.try_wait() {
            Ok(Some(status)) => break (status, false),
            Ok(None) if started.elapsed() < timeout => {
                thread::sleep(Duration::from_millis(10));
            }
            Ok(None) => {
                let _ = child.kill();
                let status = child
                    .wait()
                    .map_err(|error| format!("wait after timeout: {error}"))?;
                break (status, true);
            }
            Err(error) => return Err(format!("wait for {}: {error}", executable.display())),
        }
    };

    let stdout = read_capture(&mut stdout_file)?;
    let stderr = read_capture(&mut stderr_file)?;
    Ok(json!({
        "status": status_label(status, timed_out),
        "stdout": normalize_output(&stdout, root),
        "stderr": normalize_output(&stderr, root),
        "tree": observe_tree(root)?,
    }))
}

fn read_capture(file: &mut File) -> Result<Vec<u8>, String> {
    file.seek(SeekFrom::Start(0))
        .map_err(|error| format!("seek process capture: {error}"))?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)
        .map_err(|error| format!("read process capture: {error}"))?;
    Ok(bytes)
}

fn normalize_output(bytes: &[u8], root: &Path) -> Value {
    let mut normalized = bytes.to_vec();
    replace_bytes(&mut normalized, b"\r\n", b"\n");

    let mut spellings = vec![root.as_os_str().as_encoded_bytes().to_vec()];
    if let Ok(canonical) = root.canonicalize() {
        spellings.push(canonical.as_os_str().as_encoded_bytes().to_vec());
    }
    spellings.sort_by_key(|path| std::cmp::Reverse(path.len()));
    spellings.dedup();
    for path in spellings {
        replace_bytes(&mut normalized, &path, b"<CASE_ROOT>");
    }

    match String::from_utf8(normalized) {
        Ok(text) => Value::String(text),
        Err(error) => json!({
            "encoding": "hex",
            "data": hex(error.as_bytes()),
        }),
    }
}

fn replace_bytes(bytes: &mut Vec<u8>, needle: &[u8], replacement: &[u8]) {
    if needle.is_empty() {
        return;
    }
    let mut result = Vec::with_capacity(bytes.len());
    let mut offset = 0;
    while let Some(relative) = bytes[offset..]
        .windows(needle.len())
        .position(|window| window == needle)
    {
        let position = offset + relative;
        result.extend_from_slice(&bytes[offset..position]);
        result.extend_from_slice(replacement);
        offset = position + needle.len();
    }
    result.extend_from_slice(&bytes[offset..]);
    *bytes = result;
}

fn status_label(status: ExitStatus, timed_out: bool) -> String {
    if timed_out {
        return "timeout".into();
    }
    if let Some(code) = status.code() {
        return format!("code:{code}");
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if let Some(signal) = status.signal() {
            return format!("signal:{signal}");
        }
    }
    "unknown".into()
}

fn discover_cases(fixtures: &Path) -> Result<Vec<Case>, String> {
    let mut manifests = Vec::new();
    find_named_files(fixtures, "case.json", &mut manifests)?;
    let mut cases = Vec::with_capacity(manifests.len());
    for manifest_path in manifests {
        let source_dir = manifest_path
            .parent()
            .ok_or_else(|| format!("{} has no parent", manifest_path.display()))?
            .to_path_buf();
        let config: Value = serde_json::from_slice(
            &fs::read(&manifest_path)
                .map_err(|error| format!("read {}: {error}", manifest_path.display()))?,
        )
        .map_err(|error| format!("parse {}: {error}", manifest_path.display()))?;
        let entry = PathBuf::from(
            config
                .get("entry")
                .and_then(Value::as_str)
                .ok_or_else(|| format!("{} needs a string entry", manifest_path.display()))?,
        );
        if entry.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        }) {
            return Err(format!(
                "{} entry must stay inside its case directory",
                manifest_path.display()
            ));
        }
        if !source_dir.join(&entry).is_file() {
            return Err(format!(
                "{} entry {} does not exist",
                manifest_path.display(),
                entry.display()
            ));
        }
        let timeout_ms = match config.get("timeoutMs") {
            None => Some(DEFAULT_TIMEOUT_MS),
            Some(value) => value.as_u64().filter(|timeout| *timeout > 0),
        }
        .ok_or_else(|| {
            format!(
                "{} timeoutMs must be a positive integer",
                manifest_path.display()
            )
        })?;
        let expected_status = match config.get("expectedStatus") {
            None => Some(0),
            Some(value) => value
                .as_i64()
                .and_then(|status| (0..=255).contains(&status).then_some(status as i32)),
        }
        .ok_or_else(|| {
            format!(
                "{} expectedStatus must be an integer from 0 through 255",
                manifest_path.display()
            )
        })?;
        let relative = source_dir
            .strip_prefix(fixtures)
            .map_err(|error| format!("derive case name: {error}"))?;
        let name = relative
            .components()
            .map(|component| component.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/");
        cases.push(Case {
            name,
            source_dir,
            entry,
            timeout: Duration::from_millis(timeout_ms),
            expected_status,
        });
    }
    cases.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(cases)
}

fn find_named_files(dir: &Path, name: &str, found: &mut Vec<PathBuf>) -> Result<(), String> {
    let entries = fs::read_dir(dir).map_err(|error| format!("read {}: {error}", dir.display()))?;
    for entry in entries {
        let entry = entry.map_err(|error| format!("read entry in {}: {error}", dir.display()))?;
        let file_type = entry
            .file_type()
            .map_err(|error| format!("inspect {}: {error}", entry.path().display()))?;
        if file_type.is_dir() {
            find_named_files(&entry.path(), name, found)?;
        } else if file_type.is_file() && entry.file_name() == name {
            found.push(entry.path());
        }
    }
    Ok(())
}

fn copy_tree(source: &Path, destination: &Path) -> Result<(), String> {
    fs::create_dir_all(destination)
        .map_err(|error| format!("create {}: {error}", destination.display()))?;
    for entry in
        fs::read_dir(source).map_err(|error| format!("read {}: {error}", source.display()))?
    {
        let entry =
            entry.map_err(|error| format!("read entry in {}: {error}", source.display()))?;
        let from = entry.path();
        let to = destination.join(entry.file_name());
        let file_type = entry
            .file_type()
            .map_err(|error| format!("inspect {}: {error}", from.display()))?;
        if file_type.is_dir() {
            copy_tree(&from, &to)?;
        } else if file_type.is_file() {
            fs::copy(&from, &to)
                .map_err(|error| format!("copy {} to {}: {error}", from.display(), to.display()))?;
        } else if file_type.is_symlink() {
            copy_symlink(&from, &to)?;
        } else {
            return Err(format!("unsupported fixture file type: {}", from.display()));
        }
    }
    Ok(())
}

#[cfg(unix)]
fn copy_symlink(source: &Path, destination: &Path) -> Result<(), String> {
    use std::os::unix::fs::symlink;
    let target = fs::read_link(source)
        .map_err(|error| format!("read symlink {}: {error}", source.display()))?;
    symlink(&target, destination).map_err(|error| {
        format!(
            "copy symlink {} to {}: {error}",
            source.display(),
            destination.display()
        )
    })
}

#[cfg(not(unix))]
fn copy_symlink(source: &Path, _destination: &Path) -> Result<(), String> {
    Err(format!(
        "symlink fixtures are not supported on this platform: {}",
        source.display()
    ))
}

fn observe_tree(root: &Path) -> Result<Vec<String>, String> {
    let mut entries = Vec::new();
    observe_tree_inner(root, root, &mut entries)?;
    entries.sort();
    Ok(entries)
}

fn observe_tree_inner(root: &Path, dir: &Path, entries: &mut Vec<String>) -> Result<(), String> {
    for entry in fs::read_dir(dir).map_err(|error| format!("read {}: {error}", dir.display()))? {
        let entry = entry.map_err(|error| format!("read entry in {}: {error}", dir.display()))?;
        let path = entry.path();
        let relative = normalized_relative(root, &path)?;
        let metadata = fs::symlink_metadata(&path)
            .map_err(|error| format!("inspect {}: {error}", path.display()))?;
        if metadata.is_dir() {
            entries.push(format!("D:{relative}"));
            observe_tree_inner(root, &path, entries)?;
        } else if metadata.is_file() {
            let bytes =
                fs::read(&path).map_err(|error| format!("read {}: {error}", path.display()))?;
            entries.push(format!("F:{relative}:{}", hex(&bytes)));
        } else if metadata.file_type().is_symlink() {
            let target = fs::read_link(&path)
                .map_err(|error| format!("read symlink {}: {error}", path.display()))?;
            entries.push(format!("L:{relative}:{}", target.to_string_lossy()));
        }
    }
    Ok(())
}

fn normalized_relative(root: &Path, path: &Path) -> Result<String, String> {
    Ok(path
        .strip_prefix(root)
        .map_err(|error| format!("make {} relative: {error}", path.display()))?
        .components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/"))
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(DIGITS[(byte >> 4) as usize] as char);
        output.push(DIGITS[(byte & 0x0f) as usize] as char);
    }
    output
}

fn verify_reference_revision(reference: &Path, repo: &Path) -> Result<(), String> {
    let expected = fs::read_to_string(repo.join("vendor/bun/VENDORED_COMMIT"))
        .map_err(|error| format!("read VENDORED_COMMIT: {error}"))?;
    let expected = expected.trim();
    let output = Command::new(reference)
        .arg("--revision")
        .output()
        .map_err(|error| format!("run {} --revision: {error}", reference.display()))?;
    let revision = String::from_utf8_lossy(&output.stdout);
    let reported_sha = revision.trim().rsplit_once('+').map(|(_, sha)| sha);
    if !output.status.success()
        || !reported_sha.is_some_and(|sha| sha.len() >= 7 && expected.starts_with(sha))
    {
        return Err(format!(
            "reference Bun is not built from VENDORED_COMMIT {expected}: {:?}",
            revision.trim()
        ));
    }
    Ok(())
}

fn require_file(path: &Path, label: &str) -> Result<(), String> {
    if path.is_file() {
        Ok(())
    } else {
        Err(format!("{label} not found at {}", path.display()))
    }
}

fn pretty(value: &Value) -> String {
    serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string())
}

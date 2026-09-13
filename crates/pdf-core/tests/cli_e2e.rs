//! Process-level CLI contract tests — spawn the real `pdf-compressor-cli`
//! binary and pin the behaviors only a true process can prove: exit codes,
//! stdout/stderr stream discipline, stdin bounding, pipe survivability, and
//! the quick-mode directory walk.
//! CLI 进程级契约测试 — 起真实进程，钉死只有真进程才能验证的行为：
//! 退出码、stdout/stderr 流纪律、stdin 限量、管道存活性与 quick 目录遍历。
//!
//! These complement the unit-level tests in `src/bin/pdf-compressor-cli.rs`
//! (argument parsing, notification copy) and the engine integration tests in
//! `src/pdf/tests.rs` (compression semantics): here the contract is the
//! process boundary itself — exactly what scripts and file-manager
//! integrations depend on.
//!
//! Binary resolution: the spawn uses the plain program name (a literal — the
//! path is build-machine state, never user input) and points the child's
//! resolution at the just-built binary: its directory goes first in the
//! child's `PATH` (Unix resolves the program against the child's own
//! environment) resp. becomes the child's working directory (Windows'
//! `CreateProcess` search order includes the current directory). This keeps
//! the spawned executable pinned to the cargo-built one, not whatever may be
//! installed on the host.
//!
//! Isolation: every test points `XDG_CONFIG_HOME` at a scratch directory so
//! `quick` never picks up a real quick-profile from the developer's machine
//! (and never writes into it).

use std::{
    io::Write,
    path::{Path, PathBuf},
    process::{Child, Command, Output as ProcessOutput, Stdio},
};

use serde_json::Value;

/// The CLI binary directory for this test run (cargo sets this to the
/// just-built executable). The directory — not the full path — is what the
/// child needs to resolve the literal program name.
fn cli_binary_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_pdf-compressor-cli"))
        .parent()
        .expect("binary path has a parent")
        .to_path_buf()
}

fn cli() -> Command {
    let dir = tempfile::tempdir().expect("tempdir");
    let config_home = dir.path().join("config");
    std::fs::create_dir_all(&config_home).expect("create config home");
    // Leak the tempdir: the child may still read/write under it while the
    // test finishes, and the OS reaps temp files anyway.
    std::mem::forget(dir);

    let binary_dir = cli_binary_dir();
    let mut command = Command::new("pdf-compressor-cli");
    command.env("XDG_CONFIG_HOME", &config_home);
    #[cfg(unix)]
    {
        let parent_path = std::env::var_os("PATH").unwrap_or_default();
        let mut search = std::ffi::OsString::new();
        search.push(binary_dir.as_os_str());
        search.push(":");
        search.push(parent_path);
        command.env("PATH", search);
    }
    #[cfg(windows)]
    {
        command.current_dir(&binary_dir);
    }
    command
}

/// Fixture: a one-page text + JPEG-image PDF that reliably compresses.
fn write_fixture_pdf(dir: &Path, name: &str) -> PathBuf {
    let bytes = pdf_core::testutil::jpeg_page_pdf_bytes(1200, 900, 95);
    let path = dir.join(name);
    std::fs::write(&path, bytes).expect("write fixture");
    path
}

struct Output {
    status: std::process::ExitStatus,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

fn run(command: &mut Command) -> Output {
    let ProcessOutput {
        status,
        stdout,
        stderr,
    } = command.output().expect("spawn cli");
    Output {
        status,
        stdout,
        stderr,
    }
}

fn json(bytes: &[u8]) -> Value {
    serde_json::from_slice(bytes).expect("output must be JSON")
}

/// Parse every whitespace-separated JSON document in `bytes` — the pipeline
/// prints the summary first and may append a write-failure error afterwards,
/// so stderr can legitimately carry two documents.
fn json_stream(bytes: &[u8]) -> Vec<Value> {
    serde_json::Deserializer::from_slice(bytes)
        .into_iter::<Value>()
        .map(|result| result.expect("each stderr document must be JSON"))
        .collect()
}

fn scratch_dir(tag: &str) -> PathBuf {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join(tag);
    std::fs::create_dir_all(&path).expect("create scratch");
    std::mem::forget(dir);
    path
}

// ---------------------------------------------------------------------------
// analyze / compress (file mode)
// ---------------------------------------------------------------------------

#[test]
fn analyze_prints_json_summary_and_exits_zero() {
    let work = scratch_dir("analyze");
    let fixture = write_fixture_pdf(&work, "doc.pdf");
    let mut command = cli();
    let output = run(command.arg("analyze").arg(&fixture));

    assert!(output.status.success(), "analyze must exit 0");
    let summary = json(&output.stdout);
    assert!(summary.get("documentKind").is_some(), "summary shape");
    assert!(summary.get("pageCount").and_then(|v| v.as_u64()) == Some(1));
}

#[test]
fn compress_writes_output_and_prints_json_response() {
    let work = scratch_dir("compress-file");
    let fixture = write_fixture_pdf(&work, "doc.pdf");
    let mut command = cli();
    let output = run(command
        .arg("compress")
        .arg(&fixture)
        .arg("--preset")
        .arg("maximum"));

    assert!(output.status.success(), "compress must exit 0");
    let response = json(&output.stdout);
    let output_path = PathBuf::from(
        response
            .get("outputPath")
            .and_then(|value| value.as_str())
            .expect("outputPath in response"),
    );
    assert!(output_path.exists(), "the reported output file must exist");
    assert!(
        response.get("outputWasSmaller").and_then(|v| v.as_bool()) == Some(true),
        "a fresh JPEG-95 fixture must compress smaller"
    );
}

#[test]
fn missing_input_fails_with_stderr_json_and_nonzero_exit() {
    let mut command = cli();
    let output = run(command.arg("compress").arg("/definitely/not/there.pdf"));

    assert!(!output.status.success(), "missing input must exit nonzero");
    assert!(output.stdout.is_empty(), "stdout stays clean on failure");
    let error = json(&output.stderr);
    assert!(error.get("code").is_some(), "error payload carries a code");
}

#[test]
fn unknown_command_and_help_exit_codes() {
    let mut command = cli();
    let output = run(command.arg("frobnicate").arg("x.pdf"));
    assert!(!output.status.success());
    assert!(json(&output.stderr).get("code").is_some());

    for help in ["-h", "--help"] {
        let mut command = cli();
        let output = run(command.arg(help));
        assert!(output.status.success(), "{help} exits 0");
        assert!(String::from_utf8_lossy(&output.stdout).contains("USAGE"));
    }

    // No arguments at all also prints usage and succeeds.
    let output = run(&mut cli());
    assert!(output.status.success());
}

// ---------------------------------------------------------------------------
// Pipeline mode (compress - --stdout)
// ---------------------------------------------------------------------------

fn spawn_pipeline() -> Child {
    cli()
        .arg("compress")
        .arg("-")
        .arg("--stdout")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn pipeline")
}

fn feed_stdin(child: &mut Child, bytes: Vec<u8>) {
    let mut stdin = child.stdin.take().expect("stdin pipe");
    // Write in a worker thread: a fixture larger than the pipe capacity
    // would otherwise deadlock the test before the child starts reading.
    let writer = std::thread::spawn(move || {
        let _ = stdin.write_all(&bytes);
    });
    writer.join().expect("stdin writer");
}

#[test]
fn pipeline_keeps_stdout_pure_pdf_and_stderr_json() {
    let work = scratch_dir("pipe");
    let fixture = write_fixture_pdf(&work, "doc.pdf");
    let input_bytes = std::fs::read(&fixture).expect("read fixture");

    let mut child = spawn_pipeline();
    feed_stdin(&mut child, input_bytes);
    let output = child.wait_with_output().expect("collect output");

    assert!(output.status.success(), "pipeline must exit 0");
    assert!(
        output.stdout.starts_with(b"%PDF-"),
        "stdout must carry the PDF bytes"
    );
    let summary = json(&output.stderr);
    assert!(
        summary.get("outputPath").and_then(|v| v.as_str()) == Some("<stdout>"),
        "stderr carries the JSON summary with the pipe output path"
    );
    // Round-trip: the piped PDF must load as a document.
    assert!(
        lopdf::Document::load_mem(&output.stdout).is_ok(),
        "piped bytes must be a valid PDF"
    );
}

#[test]
fn pipeline_passthrough_keeps_bytes_identical_when_not_smaller() {
    let work = scratch_dir("pipe-passthrough");
    let fixture = write_fixture_pdf(&work, "doc.pdf");
    let input_bytes = std::fs::read(&fixture).expect("read fixture");

    // Compress once through the pipe, then feed the result back: when the
    // second run cannot beat its input, the contract is a byte-identical
    // passthrough (never an empty or corrupt pipe).
    let mut child = spawn_pipeline();
    feed_stdin(&mut child, input_bytes);
    let first = child.wait_with_output().expect("first run");
    assert!(first.status.success());

    let mut child = spawn_pipeline();
    feed_stdin(&mut child, first.stdout.clone());
    let second = child.wait_with_output().expect("second run");

    assert!(second.status.success());
    let summary = json(&second.stderr);
    if summary.get("outputWasSmaller").and_then(|v| v.as_bool()) == Some(false) {
        assert_eq!(
            second.stdout, first.stdout,
            "not-smaller runs pass the original bytes through unchanged"
        );
    } else {
        assert!(
            second.stdout.starts_with(b"%PDF-"),
            "smaller runs still emit a valid PDF"
        );
    }
}

#[cfg(unix)]
#[test]
fn pipeline_broken_stdout_pipe_fails_with_json_error() {
    let work = scratch_dir("pipe-epipe");
    let fixture = write_fixture_pdf(&work, "doc.pdf");
    let input_bytes = std::fs::read(&fixture).expect("read fixture");

    let mut child = spawn_pipeline();
    // Drop the read end: the child's stdout write hits EPIPE (Rust ignores
    // SIGPIPE by default, so the write returns an error instead of killing
    // the process) — the CLI must report it as a JSON error on stderr with a
    // nonzero exit, not die silently or print PDF bytes to a dead pipe.
    drop(child.stdout.take());
    feed_stdin(&mut child, input_bytes);
    let output = child.wait_with_output().expect("collect output");

    assert!(!output.status.success(), "a dead pipe is a real failure");
    let documents = json_stream(&output.stderr);
    assert!(
        documents.iter().any(|value| value.get("code").is_some()),
        "stderr must end with a JSON error, got {documents:?}"
    );
}

#[test]
fn pipeline_target_size_searches_in_memory() {
    let work = scratch_dir("pipe-target");
    let fixture = write_fixture_pdf(&work, "doc.pdf");
    let input_bytes = std::fs::read(&fixture).expect("read fixture");

    // 0.9.0: the bytes pipeline gained the target-size search — fully in
    // memory, no probe files. The summary reports which target it was fit
    // to, stdout carries a valid PDF no larger than the budget (or the
    // engine's best effort with a warning).
    let mut command = cli();
    let mut child = command
        .arg("compress")
        .arg("-")
        .arg("--stdout")
        .arg("--target-size")
        .arg("300KB")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn target-size pipeline");
    feed_stdin(&mut child, input_bytes);
    let output = child.wait_with_output().expect("collect output");

    assert!(output.status.success());
    let summary = json(&output.stderr);
    let codes: Vec<&str> = summary
        .get("notices")
        .and_then(|value| value.as_array())
        .map(|notices| {
            notices
                .iter()
                .filter_map(|notice| notice.get("code").and_then(|c| c.as_str()))
                .collect()
        })
        .unwrap_or_default();
    assert!(
        codes.contains(&"compress.note.targetSizeMet")
            || codes.contains(&"compress.warning.targetSizeMissed"),
        "the summary must state whether the target was met, got {codes:?}"
    );
    assert!(
        lopdf::Document::load_mem(&output.stdout).is_ok(),
        "piped bytes must be a valid PDF"
    );
}

/// The stdin pre-check reads at most `MAX_INPUT_BYTES + 1` and rejects an
/// oversized stream with `error.inputTooLarge` instead of buffering it all.
/// Ignored by default: the probe needs ~2 GiB of RAM and a few seconds of
/// /dev/zero throughput; run explicitly with
/// `cargo test --test cli_e2e -- --ignored` (release recommended).
#[cfg(unix)]
#[test]
#[ignore = "allocates ~2 GiB; run with --ignored on capable machines"]
fn pipeline_oversized_stdin_reports_input_too_large() {
    // /dev/zero gives an endless (fast, file-backed) stdin; the CLI must
    // stop at the ceiling and fail before any PDF parsing. The device is
    // handed over as a plain stdin file — no shell involved.
    let zero = std::fs::File::open("/dev/zero").expect("open /dev/zero");
    let mut command = cli();
    let output = run(command
        .arg("compress")
        .arg("-")
        .arg("--stdout")
        .stdin(Stdio::from(zero)));

    assert!(!output.status.success());
    let error = json(&output.stderr);
    assert_eq!(
        error.get("code").and_then(|value| value.as_str()),
        Some("error.inputTooLarge"),
        "oversized stdin must fail with the friendly ceiling error"
    );
}

// ---------------------------------------------------------------------------
// Flag discipline (mutual exclusion, missing values)
// ---------------------------------------------------------------------------

#[test]
fn flag_errors_exit_nonzero_with_stderr_json() {
    let work = scratch_dir("flags");
    let fixture = write_fixture_pdf(&work, "doc.pdf");
    let fixture_arg = fixture.to_string_lossy().into_owned();
    let work_arg = work.to_string_lossy().into_owned();
    let cases: Vec<Vec<String>> = vec![
        // `compress -` without --stdout misunderstands the pipeline mode.
        vec!["compress".into(), "-".into()],
        // --stdout with a file input: the input must be `-`.
        vec!["compress".into(), fixture_arg.clone(), "--stdout".into()],
        // --stdout and --output-dir fight over the destination.
        vec![
            "compress".into(),
            "-".into(),
            "--stdout".into(),
            "--output-dir".into(),
            work_arg.clone(),
        ],
        // Dangling value flag.
        vec!["compress".into(), fixture_arg.clone(), "--preset".into()],
        // A value that looks like the next flag is a missing value.
        vec![
            "compress".into(),
            fixture_arg.clone(),
            "--password".into(),
            "--grayscale".into(),
        ],
        // quick always writes next to the original.
        vec![
            "quick".into(),
            fixture_arg,
            "--output-dir".into(),
            work_arg,
            "--no-notify".into(),
        ],
    ];

    for case in cases {
        let mut command = cli();
        let output = run(command.args(&case));
        assert!(
            !output.status.success(),
            "must exit nonzero: {:?}",
            case.join(" ")
        );
        assert!(
            output.stdout.is_empty(),
            "no summary output for config errors: {:?}",
            case.join(" ")
        );
        assert!(
            json(&output.stderr).get("code").is_some(),
            "stderr must carry a JSON error: {:?}",
            case.join(" ")
        );
    }
}

// ---------------------------------------------------------------------------
// quick mode: directory recursion & dedup
// ---------------------------------------------------------------------------

#[test]
fn quick_walks_directories_recursively_for_pdfs() {
    let work = scratch_dir("quick-walk");
    write_fixture_pdf(&work, "top.pdf");
    let nested = work.join("nested");
    std::fs::create_dir_all(&nested).expect("create nested dir");
    write_fixture_pdf(&nested, "deep.pdf");
    std::fs::write(work.join("ignored.txt"), b"not a pdf").expect("write txt");

    let mut command = cli();
    let output = run(command.arg("quick").arg(&work).arg("--no-notify"));

    assert!(output.status.success(), "quick exits 0 on success");
    let summary = json(&output.stdout);
    let total = summary
        .get("filesCompressed")
        .and_then(|value| value.as_u64())
        .expect("filesCompressed")
        + summary
            .get("filesNotSmaller")
            .and_then(|value| value.as_u64())
            .expect("filesNotSmaller");
    assert_eq!(total, 2, "exactly the two PDFs, the .txt is skipped");
}

#[test]
fn quick_dedupes_overlapping_directory_and_file_arguments() {
    let work = scratch_dir("quick-dedupe");
    let fixture = write_fixture_pdf(&work, "a.pdf");

    // The same PDF via three spellings plus the containing directory: one
    // compression, one result entry.
    let mut command = cli();
    let output = run(command
        .arg("quick")
        .arg(&fixture)
        .arg(work.join("./a.pdf"))
        .arg(&work)
        .arg("--no-notify"));

    assert!(output.status.success());
    let summary = json(&output.stdout);
    let results = summary
        .get("results")
        .and_then(|value| value.as_array())
        .expect("results array");
    assert_eq!(results.len(), 1, "each PDF is compressed exactly once");
}

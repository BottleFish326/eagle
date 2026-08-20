use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use format_worker::{WorkerClient, WorkerRunError, WorkerSpec, digest_file_sha256};
use tempfile::tempdir;
use uuid::Uuid;

const FIXTURE_WORKER: &str = env!("CARGO_BIN_EXE_material-eagle-format-worker-fixture");

#[test]
fn executes_one_verified_worker_and_returns_a_bounded_png() {
    let workspace = workspace();
    let client = client(
        "fixture-ok",
        &workspace,
        Duration::from_secs(1),
        1024 * 1024,
    );
    let request = client
        .thumbnail_request(Uuid::now_v7(), &workspace.source, &workspace.root, 16)
        .expect("authorized request");

    let result = client
        .execute(&request, &workspace.root)
        .expect("worker success");

    assert_eq!(
        (result.properties.width, result.properties.height),
        (16, 16)
    );
    assert!(result.png.expect("PNG").starts_with(b"\x89PNG\r\n\x1a\n"));
}

#[test]
fn isolates_crash_timeout_output_flood_and_source_change() {
    for (provider, expected) in [
        ("fixture-crash", "crash"),
        ("fixture-timeout", "timeout"),
        ("fixture-output-flood", "overflow"),
        ("fixture-source-change", "source-change"),
    ] {
        let workspace = workspace();
        let timeout = if provider == "fixture-timeout" {
            Duration::from_millis(50)
        } else {
            Duration::from_secs(1)
        };
        let client = client(provider, &workspace, timeout, 1024);
        let request = client
            .thumbnail_request(Uuid::now_v7(), &workspace.source, &workspace.root, 16)
            .expect("authorized request");
        let error = client
            .execute(&request, &workspace.root)
            .expect_err(expected);
        match (expected, error) {
            ("crash", WorkerRunError::Crashed { diagnostic, .. }) => {
                assert!(diagnostic.contains("<source>"));
                assert!(!diagnostic.contains(&workspace.source.to_string_lossy().into_owned()));
            }
            ("timeout", WorkerRunError::TimedOut { .. })
            | ("overflow", WorkerRunError::OutputTooLarge)
            | ("source-change", WorkerRunError::SourceChanged) => {}
            (_, other) => panic!("unexpected {expected} error: {other:?}"),
        }
    }
}

#[test]
fn rejects_worker_substitution_and_source_escape_before_spawn() {
    let workspace = workspace();
    let mut spec = spec("fixture-ok", &workspace, Duration::from_secs(1), 1024);
    spec.expected_sha256 = "0".repeat(64);
    assert!(matches!(
        WorkerClient::open(spec),
        Err(WorkerRunError::ExecutableChanged)
    ));

    let client = client("fixture-ok", &workspace, Duration::from_secs(1), 1024);
    let outside = workspace.directory.path().join("outside.avif");
    fs::write(&outside, b"outside").expect("outside source");
    let nested_root = workspace.root.join("nested");
    fs::create_dir(&nested_root).expect("nested root");
    assert!(matches!(
        client.thumbnail_request(Uuid::now_v7(), &outside, &nested_root, 16),
        Err(WorkerRunError::SourceOutsideRoot)
    ));

    let request = client
        .thumbnail_request(Uuid::now_v7(), &workspace.source, &workspace.root, 16)
        .expect("valid request");
    assert!(matches!(
        client.execute(&request, &nested_root),
        Err(WorkerRunError::SourceOutsideRoot)
    ));
}

#[test]
fn rejects_response_limits_and_a_source_replaced_by_an_escaping_symlink() {
    let workspace = workspace();
    let client = client(
        "fixture-ok",
        &workspace,
        Duration::from_secs(1),
        1024 * 1024,
    );
    let mut request = client
        .thumbnail_request(Uuid::now_v7(), &workspace.source, &workspace.root, 16)
        .expect("valid request");
    request.limits.max_source_dimension = 8;
    assert!(matches!(
        client.execute(&request, &workspace.root),
        Err(WorkerRunError::ResourceLimitViolation)
    ));

    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;

        let request = client
            .thumbnail_request(Uuid::now_v7(), &workspace.source, &workspace.root, 16)
            .expect("valid request");
        let outside = workspace
            .directory
            .path()
            .join("outside-after-request.avif");
        fs::write(&outside, b"isolated source").expect("outside source");
        fs::remove_file(&workspace.source).expect("replace source");
        symlink(&outside, &workspace.source).expect("escaping source symlink");
        assert!(matches!(
            client.execute(&request, &workspace.root),
            Err(WorkerRunError::SourceOutsideRoot)
        ));
    }
}

struct Workspace {
    directory: tempfile::TempDir,
    root: PathBuf,
    source: PathBuf,
}

fn workspace() -> Workspace {
    let directory = tempdir().expect("tempdir");
    let root = directory.path().join("library");
    fs::create_dir(&root).expect("library root");
    let source = root.join("source.avif");
    fs::write(&source, b"isolated source").expect("source");
    Workspace {
        directory,
        root,
        source,
    }
}

fn client(
    provider: &str,
    workspace: &Workspace,
    timeout: Duration,
    max_png_bytes: u64,
) -> WorkerClient {
    WorkerClient::open(spec(provider, workspace, timeout, max_png_bytes)).expect("worker client")
}

fn spec(
    provider: &str,
    workspace: &Workspace,
    timeout: Duration,
    max_png_bytes: u64,
) -> WorkerSpec {
    let executable = Path::new(FIXTURE_WORKER)
        .canonicalize()
        .expect("fixture worker");
    let mut spec = WorkerSpec::new(
        executable.clone(),
        workspace
            .directory
            .path()
            .canonicalize()
            .expect("working dir"),
        digest_file_sha256(&executable).expect("worker digest"),
        provider,
        "fixture-v1",
    );
    spec.timeout = timeout;
    spec.max_png_bytes = max_png_bytes;
    spec
}

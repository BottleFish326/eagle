use std::fs::{self, File};
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use sha2::{Digest, Sha256};
use thiserror::Error;
use uuid::Uuid;

use crate::protocol::{
    DEFAULT_MAX_DECODE_BYTES, DEFAULT_MAX_OUTPUT_BYTES, DEFAULT_MAX_SOURCE_DIMENSION,
    DEFAULT_WORKER_TIMEOUT, HeifProperties, MAX_RESPONSE_JSON_BYTES, NativePath, ProtocolError,
    WorkerLimits, WorkerOperation, WorkerOutcome, WorkerRequest, read_response, write_request,
};

const MAX_STDERR_BYTES: usize = 4 * 1024;
const PROCESS_POLL_INTERVAL: Duration = Duration::from_millis(5);

#[derive(Debug, Clone)]
pub struct WorkerSpec {
    pub executable: PathBuf,
    pub working_directory: PathBuf,
    pub expected_sha256: String,
    pub provider_id: String,
    pub provider_version: String,
    pub timeout: Duration,
    pub max_response_json_bytes: usize,
    pub max_png_bytes: u64,
    pub max_stderr_bytes: usize,
}

impl WorkerSpec {
    #[must_use]
    pub fn new(
        executable: PathBuf,
        working_directory: PathBuf,
        expected_sha256: String,
        provider_id: impl Into<String>,
        provider_version: impl Into<String>,
    ) -> Self {
        Self {
            executable,
            working_directory,
            expected_sha256,
            provider_id: provider_id.into(),
            provider_version: provider_version.into(),
            timeout: DEFAULT_WORKER_TIMEOUT,
            max_response_json_bytes: MAX_RESPONSE_JSON_BYTES,
            max_png_bytes: DEFAULT_MAX_OUTPUT_BYTES,
            max_stderr_bytes: MAX_STDERR_BYTES,
        }
    }
}

#[derive(Debug)]
pub struct WorkerClient {
    spec: WorkerSpec,
    executable: PathBuf,
    working_directory: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkerSuccess {
    pub properties: HeifProperties,
    pub png: Option<Vec<u8>>,
}

#[derive(Debug, Error)]
pub enum WorkerRunError {
    #[error("worker configuration is invalid: {0}")]
    InvalidConfiguration(&'static str),
    #[error("worker source is outside its authorized root")]
    SourceOutsideRoot,
    #[error("worker source is not a readable regular file")]
    InvalidSource,
    #[error("worker executable integrity changed")]
    ExecutableChanged,
    #[error("worker process I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("worker protocol failed: {0}")]
    Protocol(#[from] ProtocolError),
    #[error("worker exceeded its {timeout_ms} ms hard timeout")]
    TimedOut {
        timeout_ms: u128,
        diagnostic: String,
    },
    #[error("worker exited unsuccessfully: {status}; {diagnostic}")]
    Crashed { status: String, diagnostic: String },
    #[error("worker output exceeded its configured bound")]
    OutputTooLarge,
    #[error("worker response identity does not match the request")]
    IdentityMismatch,
    #[error("worker response exceeds the limits of its request")]
    ResourceLimitViolation,
    #[error("source changed while the worker request was running")]
    SourceChanged,
    #[error("worker PNG integrity or dimensions are invalid")]
    InvalidPng,
    #[error("worker returned {code:?}: {message}")]
    Worker {
        code: crate::WorkerErrorCode,
        message: String,
    },
    #[error("worker thread panicked")]
    ThreadPanicked,
}

impl WorkerClient {
    /// Opens a client bound to one exact worker binary and provider version.
    ///
    /// # Errors
    ///
    /// Rejects symbolic links, relative paths, invalid bounds, and a binary digest mismatch.
    pub fn open(spec: WorkerSpec) -> Result<Self, WorkerRunError> {
        validate_spec(&spec)?;
        reject_symlink(&spec.executable)?;
        reject_symlink(&spec.working_directory)?;
        let executable = fs::canonicalize(&spec.executable)?;
        let working_directory = fs::canonicalize(&spec.working_directory)?;
        if !executable.is_absolute()
            || !executable.is_file()
            || !working_directory.is_absolute()
            || !working_directory.is_dir()
        {
            return Err(WorkerRunError::InvalidConfiguration("worker paths"));
        }
        if digest_file_sha256(&executable)? != spec.expected_sha256 {
            return Err(WorkerRunError::ExecutableChanged);
        }
        Ok(Self {
            spec,
            executable,
            working_directory,
        })
    }

    /// Builds a thumbnail request only after canonical root containment and source identity checks.
    ///
    /// # Errors
    ///
    /// Rejects an escaping symlink, non-file source, invalid edge, or inaccessible metadata.
    pub fn thumbnail_request(
        &self,
        request_id: Uuid,
        source: &Path,
        authorized_root: &Path,
        max_edge: u32,
    ) -> Result<WorkerRequest, WorkerRunError> {
        self.build_request(
            request_id,
            source,
            authorized_root,
            WorkerOperation::Thumbnail,
            Some(max_edge),
        )
    }

    /// Builds a metadata-only request with the same canonical authorization checks.
    ///
    /// # Errors
    ///
    /// Rejects an escaping symlink, non-file source, or inaccessible metadata.
    pub fn metadata_request(
        &self,
        request_id: Uuid,
        source: &Path,
        authorized_root: &Path,
    ) -> Result<WorkerRequest, WorkerRunError> {
        self.build_request(
            request_id,
            source,
            authorized_root,
            WorkerOperation::Metadata,
            None,
        )
    }

    /// Executes exactly one request in a fresh process and validates its bounded response.
    ///
    /// # Errors
    ///
    /// Returns a stable error for timeout, crash, output overflow, protocol mismatch, source drift,
    /// worker-reported failures, or an invalid PNG.
    pub fn execute(
        &self,
        request: &WorkerRequest,
        authorized_root: &Path,
    ) -> Result<WorkerSuccess, WorkerRunError> {
        request.validate()?;
        if request.provider_id != self.spec.provider_id
            || request.provider_version != self.spec.provider_version
        {
            return Err(WorkerRunError::IdentityMismatch);
        }
        self.verify_executable()?;
        let requested_source_path = request.source_path.to_path_buf()?;
        let source_path = fs::canonicalize(&requested_source_path)?;
        let authorized_root = fs::canonicalize(authorized_root)?;
        if !authorized_root.is_dir()
            || source_path != requested_source_path
            || !source_path.starts_with(&authorized_root)
        {
            return Err(WorkerRunError::SourceOutsideRoot);
        }
        if source_snapshot(&source_path)? != (request.source_size, request.source_modified_unix_ns)
        {
            return Err(WorkerRunError::SourceChanged);
        }

        let mut request_frame = Vec::new();
        write_request(&mut request_frame, request)?;
        let response_cap = response_capture_limit(&self.spec)?;
        let response = self.run_process(request_frame, response_cap, &source_path)?;
        self.verify_executable()?;
        if source_snapshot(&source_path)? != (request.source_size, request.source_modified_unix_ns)
        {
            return Err(WorkerRunError::SourceChanged);
        }
        self.interpret_response(request, response, &source_path)
    }

    fn run_process(
        &self,
        request_frame: Vec<u8>,
        response_cap: usize,
        source_path: &Path,
    ) -> Result<Vec<u8>, WorkerRunError> {
        let mut child = Command::new(&self.executable)
            .arg("--stdio-once")
            .current_dir(&self.working_directory)
            .env_clear()
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let stdin = child
            .stdin
            .take()
            .ok_or(WorkerRunError::InvalidConfiguration("worker stdin"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or(WorkerRunError::InvalidConfiguration("worker stdout"))?;
        let stderr = child
            .stderr
            .take()
            .ok_or(WorkerRunError::InvalidConfiguration("worker stderr"))?;

        let writer = thread::spawn(move || {
            let mut stdin = stdin;
            std::io::Write::write_all(&mut stdin, &request_frame)?;
            std::io::Write::flush(&mut stdin)
        });
        let response_reader = thread::spawn(move || read_capped(stdout, response_cap));
        let stderr_cap = self.spec.max_stderr_bytes;
        let stderr_reader = thread::spawn(move || read_capped(stderr, stderr_cap));

        let started = Instant::now();
        let (status, timed_out) = loop {
            if let Some(status) = child.try_wait()? {
                break (status, false);
            }
            if started.elapsed() >= self.spec.timeout {
                child.kill()?;
                break (child.wait()?, true);
            }
            thread::sleep(PROCESS_POLL_INTERVAL);
        };

        let write_result = writer.join().map_err(|_| WorkerRunError::ThreadPanicked)?;
        let response = response_reader
            .join()
            .map_err(|_| WorkerRunError::ThreadPanicked)??;
        let stderr = stderr_reader
            .join()
            .map_err(|_| WorkerRunError::ThreadPanicked)??;
        let diagnostic = redact_diagnostic(&stderr.bytes, source_path, self.spec.max_stderr_bytes);

        if timed_out {
            return Err(WorkerRunError::TimedOut {
                timeout_ms: self.spec.timeout.as_millis(),
                diagnostic,
            });
        }
        if !status.success() {
            return Err(WorkerRunError::Crashed {
                status: exit_status(status),
                diagnostic,
            });
        }
        write_result?;
        if response.overflowed {
            return Err(WorkerRunError::OutputTooLarge);
        }
        Ok(response.bytes)
    }

    fn interpret_response(
        &self,
        request: &WorkerRequest,
        response: Vec<u8>,
        source_path: &Path,
    ) -> Result<WorkerSuccess, WorkerRunError> {
        let (header, png) = read_response(
            Cursor::new(response),
            self.spec.max_response_json_bytes,
            self.spec.max_png_bytes,
        )?;
        if header.request_id != request.request_id
            || header.provider_id != request.provider_id
            || header.provider_version != request.provider_version
        {
            return Err(WorkerRunError::IdentityMismatch);
        }
        match header.outcome {
            WorkerOutcome::Ready {
                properties,
                payload,
            } => {
                if properties.width > request.limits.max_source_dimension
                    || properties.height > request.limits.max_source_dimension
                    || u64::try_from(png.len()).unwrap_or(u64::MAX)
                        > request.limits.max_output_bytes
                {
                    return Err(WorkerRunError::ResourceLimitViolation);
                }
                match (request.operation, payload.as_ref(), png.is_empty()) {
                    (WorkerOperation::Metadata, None, true) => {}
                    (WorkerOperation::Thumbnail, Some(payload), false) => {
                        validate_png(&png, payload)?;
                        if payload.width > request.limits.max_edge.unwrap_or_default()
                            || payload.height > request.limits.max_edge.unwrap_or_default()
                        {
                            return Err(WorkerRunError::InvalidPng);
                        }
                    }
                    _ => return Err(WorkerRunError::InvalidPng),
                }
                Ok(WorkerSuccess {
                    properties,
                    png: (!png.is_empty()).then_some(png),
                })
            }
            WorkerOutcome::Error { code, message } => Err(WorkerRunError::Worker {
                code,
                message: redact_message(message, source_path),
            }),
        }
    }

    fn build_request(
        &self,
        request_id: Uuid,
        source: &Path,
        authorized_root: &Path,
        operation: WorkerOperation,
        max_edge: Option<u32>,
    ) -> Result<WorkerRequest, WorkerRunError> {
        let root = fs::canonicalize(authorized_root)?;
        let source = fs::canonicalize(source)?;
        if !root.is_dir() || !source.starts_with(&root) {
            return Err(WorkerRunError::SourceOutsideRoot);
        }
        if !source.is_file() {
            return Err(WorkerRunError::InvalidSource);
        }
        let (source_size, source_modified_unix_ns) = source_snapshot(&source)?;
        let timeout_ms = u64::try_from(self.spec.timeout.as_millis())
            .map_err(|_| WorkerRunError::InvalidConfiguration("worker timeout"))?;
        let request = WorkerRequest {
            schema: crate::protocol::WORKER_PROTOCOL_SCHEMA,
            request_id,
            provider_id: self.spec.provider_id.clone(),
            provider_version: self.spec.provider_version.clone(),
            operation,
            source_path: NativePath::from_path(&source),
            source_size,
            source_modified_unix_ns,
            limits: WorkerLimits {
                max_edge,
                max_source_dimension: DEFAULT_MAX_SOURCE_DIMENSION,
                max_decode_bytes: DEFAULT_MAX_DECODE_BYTES,
                max_output_bytes: self.spec.max_png_bytes,
                timeout_ms,
            },
        };
        request.validate()?;
        Ok(request)
    }

    fn verify_executable(&self) -> Result<(), WorkerRunError> {
        if digest_file_sha256(&self.executable)? != self.spec.expected_sha256 {
            return Err(WorkerRunError::ExecutableChanged);
        }
        Ok(())
    }
}

#[derive(Debug)]
struct CappedOutput {
    bytes: Vec<u8>,
    overflowed: bool,
}

fn read_capped(mut reader: impl Read, maximum: usize) -> Result<CappedOutput, std::io::Error> {
    let mut output = Vec::with_capacity(maximum.min(64 * 1024));
    let mut overflowed = false;
    let mut buffer = [0_u8; 8 * 1024];
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        let available = maximum.saturating_sub(output.len());
        let retained = read.min(available);
        output.extend_from_slice(&buffer[..retained]);
        overflowed |= retained != read;
    }
    Ok(CappedOutput {
        bytes: output,
        overflowed,
    })
}

fn validate_spec(spec: &WorkerSpec) -> Result<(), WorkerRunError> {
    if !spec.executable.is_absolute()
        || !spec.working_directory.is_absolute()
        || spec.expected_sha256.len() != 64
        || !spec
            .expected_sha256
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        || spec.timeout.is_zero()
        || spec.timeout > DEFAULT_WORKER_TIMEOUT
        || spec.max_response_json_bytes == 0
        || spec.max_response_json_bytes > MAX_RESPONSE_JSON_BYTES
        || spec.max_png_bytes == 0
        || spec.max_png_bytes > DEFAULT_MAX_OUTPUT_BYTES
        || spec.max_stderr_bytes == 0
        || spec.max_stderr_bytes > MAX_STDERR_BYTES
    {
        return Err(WorkerRunError::InvalidConfiguration("worker spec"));
    }
    Ok(())
}

fn reject_symlink(path: &Path) -> Result<(), WorkerRunError> {
    if fs::symlink_metadata(path)?.file_type().is_symlink() {
        return Err(WorkerRunError::InvalidConfiguration("symbolic link"));
    }
    Ok(())
}

fn source_snapshot(path: &Path) -> Result<(u64, i128), WorkerRunError> {
    let metadata = fs::metadata(path)?;
    if !metadata.is_file() {
        return Err(WorkerRunError::InvalidSource);
    }
    Ok((metadata.len(), unix_nanoseconds(metadata.modified()?)))
}

fn unix_nanoseconds(time: SystemTime) -> i128 {
    match time.duration_since(UNIX_EPOCH) {
        Ok(duration) => i128::try_from(duration.as_nanos()).unwrap_or(i128::MAX),
        Err(error) => -i128::try_from(error.duration().as_nanos()).unwrap_or(i128::MAX),
    }
}

fn response_capture_limit(spec: &WorkerSpec) -> Result<usize, WorkerRunError> {
    4_usize
        .checked_add(spec.max_response_json_bytes)
        .and_then(|value| value.checked_add(usize::try_from(spec.max_png_bytes).ok()?))
        .ok_or(WorkerRunError::InvalidConfiguration("response bound"))
}

fn validate_png(png: &[u8], payload: &crate::PngPayload) -> Result<(), WorkerRunError> {
    if png.len() < 24
        || !png.starts_with(b"\x89PNG\r\n\x1a\n")
        || &png[12..16] != b"IHDR"
        || u32::from_be_bytes(
            png[16..20]
                .try_into()
                .map_err(|_| WorkerRunError::InvalidPng)?,
        ) != payload.width
        || u32::from_be_bytes(
            png[20..24]
                .try_into()
                .map_err(|_| WorkerRunError::InvalidPng)?,
        ) != payload.height
        || format!("{:x}", Sha256::digest(png)) != payload.sha256
    {
        return Err(WorkerRunError::InvalidPng);
    }
    Ok(())
}

fn redact_diagnostic(bytes: &[u8], source: &Path, maximum: usize) -> String {
    let mut diagnostic = String::from_utf8_lossy(bytes).into_owned();
    let source = source.to_string_lossy();
    if !source.is_empty() {
        diagnostic = diagnostic.replace(source.as_ref(), "<source>");
    }
    if diagnostic.len() > maximum {
        let mut boundary = maximum;
        while !diagnostic.is_char_boundary(boundary) {
            boundary -= 1;
        }
        diagnostic.truncate(boundary);
    }
    diagnostic.trim().to_owned()
}

fn redact_message(mut message: String, source: &Path) -> String {
    let source = source.to_string_lossy();
    if !source.is_empty() {
        message = message.replace(source.as_ref(), "<source>");
    }
    message
}

fn exit_status(status: ExitStatus) -> String {
    status.code().map_or_else(
        || "terminated-by-signal".into(),
        |code| format!("code-{code}"),
    )
}

/// Calculates the lower-case SHA-256 of a file without loading it all into memory.
///
/// # Errors
///
/// Returns an I/O error when the file cannot be opened or read.
pub fn digest_file_sha256(path: &Path) -> Result<String, std::io::Error> {
    let mut file = File::open(path)?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 8 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    Ok(format!("{:x}", digest.finalize()))
}

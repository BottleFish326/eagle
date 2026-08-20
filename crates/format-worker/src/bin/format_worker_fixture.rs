use std::io::Write;
use std::process::ExitCode;
use std::thread;
use std::time::Duration;

use format_worker::{
    HeifProperties, PngPayload, WorkerErrorCode, WorkerOutcome, WorkerResponseHeader, read_request,
    write_response,
};
use sha2::{Digest, Sha256};

const PNG: &[u8] = include_bytes!("../../../../fixtures/formats/references/svg/minimal.png");

fn main() -> ExitCode {
    if std::env::args().skip(1).collect::<Vec<_>>() != ["--stdio-once"] {
        return ExitCode::from(2);
    }
    let Ok(request) = read_request(std::io::stdin().lock()) else {
        return ExitCode::from(3);
    };
    match request.provider_id.as_str() {
        "fixture-crash" => {
            let path = request
                .source_path
                .to_path_buf()
                .expect("fixture native path");
            eprintln!("fixture crash while reading {}", path.display());
            ExitCode::from(23)
        }
        "fixture-timeout" => {
            thread::sleep(Duration::from_secs(2));
            respond_error(&request, WorkerErrorCode::TimedOut)
        }
        "fixture-output-flood" => {
            let bytes = vec![b'x'; 1024 * 1024];
            std::io::stdout()
                .lock()
                .write_all(&bytes)
                .expect("flood stdout");
            ExitCode::SUCCESS
        }
        "fixture-source-change" => {
            let path = request
                .source_path
                .to_path_buf()
                .expect("fixture native path");
            std::fs::write(path, b"changed by isolated test worker")
                .expect("change fixture source");
            respond_ready(&request)
        }
        "fixture-ok" => respond_ready(&request),
        _ => respond_error(&request, WorkerErrorCode::Internal),
    }
}

fn respond_ready(request: &format_worker::WorkerRequest) -> ExitCode {
    let payload =
        (request.operation == format_worker::WorkerOperation::Thumbnail).then(|| PngPayload {
            byte_length: u64::try_from(PNG.len()).unwrap(),
            sha256: format!("{:x}", Sha256::digest(PNG)),
            width: 16,
            height: 16,
        });
    let response = WorkerResponseHeader {
        schema: 1,
        request_id: request.request_id,
        provider_id: request.provider_id.clone(),
        provider_version: request.provider_version.clone(),
        outcome: WorkerOutcome::Ready {
            properties: HeifProperties {
                width: 16,
                height: 16,
                orientation: Some(1),
                color_space: Some("srgb".into()),
                has_alpha: Some(false),
                image_count: 1,
            },
            payload,
        },
    };
    let png = if request.operation == format_worker::WorkerOperation::Thumbnail {
        PNG
    } else {
        &[]
    };
    if write_response(std::io::stdout().lock(), &response, png).is_ok() {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(4)
    }
}

fn respond_error(request: &format_worker::WorkerRequest, code: WorkerErrorCode) -> ExitCode {
    let response = WorkerResponseHeader::error(request, code, "isolated fixture response");
    if write_response(std::io::stdout().lock(), &response, &[]).is_ok() {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(4)
    }
}

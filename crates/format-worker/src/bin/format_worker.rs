use std::process::ExitCode;

use format_worker::{WorkerErrorCode, WorkerResponseHeader, read_request, write_response};

fn main() -> ExitCode {
    if std::env::args().skip(1).collect::<Vec<_>>() != ["--stdio-once"] {
        eprintln!("format worker requires --stdio-once");
        return ExitCode::from(2);
    }
    let request = match read_request(std::io::stdin().lock()) {
        Ok(request) => request,
        Err(error) => {
            eprintln!("invalid bounded worker request: {error}");
            return ExitCode::from(3);
        }
    };
    let response = WorkerResponseHeader::error(
        &request,
        WorkerErrorCode::CodecUnavailable,
        "libheif decoder is not bundled in this worker build",
    );
    if let Err(error) = write_response(std::io::stdout().lock(), &response, &[]) {
        eprintln!("could not write bounded worker response: {error}");
        return ExitCode::from(4);
    }
    ExitCode::SUCCESS
}

use std::process::ExitCode;

use format_worker::{
    LIBHEIF_PROVIDER_ID, LIBHEIF_PROVIDER_VERSION, WorkerErrorCode, WorkerResponseHeader,
    read_request, write_response,
};

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
    let (response, png) = process_request(&request);
    if let Err(error) = write_response(std::io::stdout().lock(), &response, &png) {
        eprintln!("could not write bounded worker response: {error}");
        return ExitCode::from(4);
    }
    ExitCode::SUCCESS
}

fn process_request(request: &format_worker::WorkerRequest) -> (WorkerResponseHeader, Vec<u8>) {
    if request.provider_id != LIBHEIF_PROVIDER_ID
        || request.provider_version != LIBHEIF_PROVIDER_VERSION
    {
        return (
            WorkerResponseHeader::error(
                request,
                WorkerErrorCode::Internal,
                "worker provider identity is not supported",
            ),
            Vec::new(),
        );
    }

    #[cfg(feature = "libheif-backend")]
    {
        match format_worker::process_libheif_request(request) {
            Ok((properties, payload, png)) => (
                WorkerResponseHeader {
                    schema: request.schema,
                    request_id: request.request_id,
                    provider_id: request.provider_id.clone(),
                    provider_version: request.provider_version.clone(),
                    outcome: format_worker::WorkerOutcome::Ready {
                        properties,
                        payload,
                    },
                },
                png,
            ),
            Err(error) => (
                WorkerResponseHeader::error(request, error.code, error.message),
                Vec::new(),
            ),
        }
    }

    #[cfg(not(feature = "libheif-backend"))]
    (
        WorkerResponseHeader::error(
            request,
            WorkerErrorCode::CodecUnavailable,
            "libheif decoder is not bundled in this worker build",
        ),
        Vec::new(),
    )
}

mod client;
#[cfg(feature = "libheif-backend")]
mod heif_backend;
mod protocol;

pub use client::{WorkerClient, WorkerRunError, WorkerSpec, WorkerSuccess, digest_file_sha256};
#[cfg(feature = "libheif-backend")]
pub use heif_backend::{BackendFailure, process_libheif_request};
pub use protocol::{
    DEFAULT_MAX_DECODE_BYTES, DEFAULT_MAX_OUTPUT_BYTES, DEFAULT_MAX_SOURCE_DIMENSION,
    DEFAULT_WORKER_TIMEOUT, HeifProperties, NativePath, PngPayload, ProtocolError, WorkerErrorCode,
    WorkerLimits, WorkerOperation, WorkerOutcome, WorkerRequest, WorkerResponseHeader,
    read_request, read_response, write_request, write_response,
};

pub const LIBHEIF_PROVIDER_ID: &str = "bundled-libheif";
pub const LIBHEIF_PROVIDER_VERSION: &str = "libheif-1.23.1-r1";

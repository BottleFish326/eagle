#![cfg(feature = "libheif-backend")]

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use format_worker::{
    DEFAULT_MAX_DECODE_BYTES, DEFAULT_MAX_OUTPUT_BYTES, DEFAULT_MAX_SOURCE_DIMENSION,
    HeifProperties, LIBHEIF_PROVIDER_ID, LIBHEIF_PROVIDER_VERSION, NativePath, WorkerErrorCode,
    WorkerLimits, WorkerOperation, WorkerRequest, process_libheif_request,
};
use sha2::{Digest, Sha256};
use uuid::Uuid;

#[test]
fn fixed_libheif_reads_metadata_and_generates_bounded_pngs() {
    for (relative, reference, expected_dimensions, expected_image_count) in [
        (
            "fixtures/formats/sources/avif/libheif-example.avif",
            "fixtures/formats/references/avif/libheif-example-64.png",
            (800, 533),
            1,
        ),
        (
            "fixtures/formats/sources/heic/libheif-example.heic",
            "fixtures/formats/references/heic/libheif-example-64.png",
            (1_280, 854),
            2,
        ),
    ] {
        let path = workspace_root().join(relative);
        let metadata_request = request(&path, WorkerOperation::Metadata);
        let (properties, payload, png) =
            process_libheif_request(&metadata_request).expect("metadata");
        assert_properties(&properties, expected_dimensions, expected_image_count);
        assert!(payload.is_none());
        assert!(png.is_empty());

        let thumbnail_request = request(&path, WorkerOperation::Thumbnail);
        let (properties, payload, png) =
            process_libheif_request(&thumbnail_request).expect("thumbnail");
        assert_properties(&properties, expected_dimensions, expected_image_count);
        let payload = payload.expect("PNG payload");
        assert!(!png.is_empty());
        assert!(payload.width <= 64 && payload.height <= 64);
        assert_eq!(payload.byte_length, u64::try_from(png.len()).unwrap());
        assert_eq!(payload.sha256, format!("{:x}", Sha256::digest(&png)));
        assert!(png.starts_with(b"\x89PNG\r\n\x1a\n"));
        assert_eq!(
            png,
            fs::read(workspace_root().join(reference)).expect("reference PNG"),
            "{relative}",
        );
    }
}

#[test]
fn fixed_libheif_isolates_malformed_unknown_and_oversized_fixtures() {
    for relative in [
        "fixtures/formats/sources/avif/corrupted-bitstream.avif",
        "fixtures/formats/sources/avif/unknown-codec.avif",
    ] {
        let path = workspace_root().join(relative);
        let (properties, payload, png) =
            process_libheif_request(&request(&path, WorkerOperation::Metadata))
                .expect("container metadata remains available");
        assert_properties(&properties, (800, 533), 1);
        assert!(payload.is_none());
        assert!(png.is_empty());
    }

    for (relative, operation, expected) in [
        (
            "fixtures/formats/sources/avif/corrupted-bitstream.avif",
            WorkerOperation::Thumbnail,
            WorkerErrorCode::InvalidContent,
        ),
        (
            "fixtures/formats/sources/avif/truncated-ftyp.avif",
            WorkerOperation::Metadata,
            WorkerErrorCode::InvalidContent,
        ),
        (
            "fixtures/formats/sources/avif/unknown-codec.avif",
            WorkerOperation::Thumbnail,
            WorkerErrorCode::CodecUnavailable,
        ),
        (
            "fixtures/formats/sources/avif/oversized-ispe.avif",
            WorkerOperation::Metadata,
            WorkerErrorCode::ResourceLimited,
        ),
    ] {
        let path = workspace_root().join(relative);
        let digest = Sha256::digest(fs::read(&path).expect("fixture bytes"));
        let failure = process_libheif_request(&request(&path, operation))
            .expect_err("adversarial fixture must fail in isolation");
        assert_eq!(failure.code, expected, "{relative}");
        assert_eq!(
            Sha256::digest(fs::read(&path).expect("fixture bytes after")),
            digest,
            "{relative}",
        );
    }
}

#[test]
fn fixed_libheif_rejects_png_output_before_exceeding_the_request_budget() {
    let path = workspace_root().join("fixtures/formats/sources/avif/resource-limited-output.avif");
    let digest = Sha256::digest(fs::read(&path).expect("fixture bytes"));
    let mut constrained = request(&path, WorkerOperation::Thumbnail);
    constrained.limits.max_output_bytes = 64;

    let failure = process_libheif_request(&constrained)
        .expect_err("bounded output fixture must exceed the request budget");
    assert_eq!(failure.code, WorkerErrorCode::ResourceLimited);
    assert_eq!(
        Sha256::digest(fs::read(&path).expect("fixture bytes after")),
        digest,
    );
}

fn request(path: &Path, operation: WorkerOperation) -> WorkerRequest {
    let path = path.canonicalize().expect("fixture path");
    let metadata = fs::metadata(&path).expect("fixture metadata");
    WorkerRequest {
        schema: 1,
        request_id: Uuid::now_v7(),
        provider_id: LIBHEIF_PROVIDER_ID.into(),
        provider_version: LIBHEIF_PROVIDER_VERSION.into(),
        operation,
        source_path: NativePath::from_path(&path),
        source_size: metadata.len(),
        source_modified_unix_ns: unix_nanoseconds(metadata.modified().expect("modified")),
        limits: WorkerLimits {
            max_edge: (operation == WorkerOperation::Thumbnail).then_some(64),
            max_source_dimension: DEFAULT_MAX_SOURCE_DIMENSION,
            max_decode_bytes: DEFAULT_MAX_DECODE_BYTES,
            max_output_bytes: DEFAULT_MAX_OUTPUT_BYTES,
            timeout_ms: 10_000,
        },
    }
}

fn assert_properties(
    properties: &HeifProperties,
    dimensions: (u32, u32),
    expected_image_count: u32,
) {
    assert_eq!((properties.width, properties.height), dimensions);
    assert_eq!(properties.orientation, Some(1));
    assert_eq!(properties.image_count, expected_image_count);
    assert!(properties.has_alpha.is_some());
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root")
        .to_owned()
}

fn unix_nanoseconds(time: SystemTime) -> i128 {
    match time.duration_since(UNIX_EPOCH) {
        Ok(duration) => i128::try_from(duration.as_nanos()).unwrap_or(i128::MAX),
        Err(error) => -i128::try_from(error.duration().as_nanos()).unwrap_or(i128::MAX),
    }
}

use std::fs;
use std::path::Path;
use std::process::ExitCode;

use format_worker::{
    WorkerClient, WorkerErrorCode, WorkerOperation, WorkerRunError, digest_file_sha256,
    open_libheif_worker_bundle,
};
use uuid::Uuid;

fn main() -> ExitCode {
    let mut arguments = std::env::args_os().skip(1);
    let Some(directory) = arguments.next() else {
        usage();
        return ExitCode::from(2);
    };
    let fixture_root = arguments.next();
    if arguments.next().is_some() {
        usage();
        return ExitCode::from(2);
    }
    match open_libheif_worker_bundle(Path::new(&directory)) {
        Ok(worker) => {
            if let Some(fixture_root) = fixture_root
                && let Err(error) = probe_fixtures(&worker, Path::new(&fixture_root))
            {
                eprintln!("worker bundle probe failed: {error}");
                return ExitCode::FAILURE;
            }
            println!("{} {}", worker.provider_id(), worker.provider_version());
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("worker bundle rejected: {error}");
            ExitCode::FAILURE
        }
    }
}

fn usage() {
    eprintln!(
        "usage: material-eagle-verify-worker-bundle <bundle-directory> [format-fixture-root]"
    );
}

fn probe_fixtures(worker: &WorkerClient, root: &Path) -> Result<(), String> {
    for (relative, reference, expected) in [
        (
            "avif/libheif-example.avif",
            "avif/libheif-example-64.png",
            (800, 533, 1),
        ),
        (
            "heic/libheif-example.heic",
            "heic/libheif-example-64.png",
            (1280, 854, 2),
        ),
    ] {
        probe_ready(worker, root, relative, reference, expected)?;
    }
    for relative in ["avif/corrupted-bitstream.avif", "avif/unknown-codec.avif"] {
        let source = root.join(relative);
        let digest = digest_file_sha256(&source).map_err(|error| error.to_string())?;
        let metadata = worker
            .metadata_request(Uuid::now_v7(), &source, root)
            .and_then(|request| worker.execute(&request, root))
            .map_err(|error| error.to_string())?;
        if (
            metadata.properties.width,
            metadata.properties.height,
            metadata.properties.image_count,
        ) != (800, 533, 1)
            || metadata.png.is_some()
            || metadata.png_dimensions.is_some()
        {
            return Err(format!(
                "{relative} metadata does not match the fixed container"
            ));
        }
        if digest_file_sha256(&source).map_err(|error| error.to_string())? != digest {
            return Err(format!("worker changed adversarial fixture {relative}"));
        }
    }
    for (relative, operation, expected) in [
        (
            "avif/corrupted-bitstream.avif",
            WorkerOperation::Thumbnail,
            WorkerErrorCode::InvalidContent,
        ),
        (
            "avif/truncated-ftyp.avif",
            WorkerOperation::Metadata,
            WorkerErrorCode::InvalidContent,
        ),
        (
            "avif/unknown-codec.avif",
            WorkerOperation::Thumbnail,
            WorkerErrorCode::CodecUnavailable,
        ),
        (
            "avif/oversized-ispe.avif",
            WorkerOperation::Metadata,
            WorkerErrorCode::ResourceLimited,
        ),
    ] {
        probe_failure(worker, root, relative, operation, expected, None)?;
    }
    probe_failure(
        worker,
        root,
        "avif/resource-limited-output.avif",
        WorkerOperation::Thumbnail,
        WorkerErrorCode::ResourceLimited,
        Some(64),
    )?;
    Ok(())
}

fn probe_ready(
    worker: &WorkerClient,
    root: &Path,
    relative: &str,
    reference: &str,
    expected: (u32, u32, u32),
) -> Result<(), String> {
    let source = root.join(relative);
    let digest = digest_file_sha256(&source).map_err(|error| error.to_string())?;
    let metadata = worker
        .metadata_request(Uuid::now_v7(), &source, root)
        .and_then(|request| worker.execute(&request, root))
        .map_err(|error| error.to_string())?;
    if (
        metadata.properties.width,
        metadata.properties.height,
        metadata.properties.image_count,
    ) != expected
        || metadata.png.is_some()
        || metadata.png_dimensions.is_some()
    {
        return Err("metadata result does not match the fixed fixture".into());
    }
    let thumbnail = worker
        .thumbnail_request(Uuid::now_v7(), &source, root, 64)
        .and_then(|request| worker.execute(&request, root))
        .map_err(|error| error.to_string())?;
    let dimensions = thumbnail
        .png_dimensions
        .ok_or_else(|| "thumbnail has no dimensions".to_owned())?;
    let png = thumbnail
        .png
        .ok_or_else(|| "thumbnail has no PNG payload".to_owned())?;
    if dimensions.0 > 64 || dimensions.1 > 64 {
        return Err("thumbnail result exceeds the fixed edge".into());
    }
    let reference_root = root
        .parent()
        .ok_or_else(|| "format source root has no fixture parent".to_owned())?
        .join("references");
    let expected_png =
        fs::read(reference_root.join(reference)).map_err(|error| error.to_string())?;
    if png != expected_png {
        return Err(format!("{relative} PNG does not match the fixed reference"));
    }
    if digest_file_sha256(&source).map_err(|error| error.to_string())? != digest {
        return Err("worker changed a fixed source fixture".into());
    }
    Ok(())
}

fn probe_failure(
    worker: &WorkerClient,
    root: &Path,
    relative: &str,
    operation: WorkerOperation,
    expected: WorkerErrorCode,
    max_output_bytes: Option<u64>,
) -> Result<(), String> {
    let source = root.join(relative);
    let digest = digest_file_sha256(&source).map_err(|error| error.to_string())?;
    let mut request = match operation {
        WorkerOperation::Metadata => worker.metadata_request(Uuid::now_v7(), &source, root),
        WorkerOperation::Thumbnail => worker.thumbnail_request(Uuid::now_v7(), &source, root, 64),
    }
    .map_err(|error| error.to_string())?;
    if let Some(maximum) = max_output_bytes {
        request.limits.max_output_bytes = maximum;
    }
    match worker.execute(&request, root) {
        Err(WorkerRunError::Worker { code, .. }) if code == expected => {}
        Err(error) => {
            return Err(format!(
                "{relative} returned {error}; expected worker error {expected:?}"
            ));
        }
        Ok(_) => return Err(format!("{relative} unexpectedly succeeded")),
    }
    if digest_file_sha256(&source).map_err(|error| error.to_string())? != digest {
        return Err(format!("worker changed adversarial fixture {relative}"));
    }
    Ok(())
}

use std::path::Path;
use std::process::ExitCode;

use format_worker::{WorkerClient, digest_file_sha256, open_libheif_worker_bundle};
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
    for (relative, expected) in [
        ("avif/libheif-example.avif", (800, 533, 1)),
        ("heic/libheif-example.heic", (1280, 854, 2)),
    ] {
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
        if thumbnail.png.is_none() || dimensions.0 > 64 || dimensions.1 > 64 {
            return Err("thumbnail result exceeds the fixed edge".into());
        }
        if digest_file_sha256(&source).map_err(|error| error.to_string())? != digest {
            return Err("worker changed a fixed source fixture".into());
        }
    }
    Ok(())
}

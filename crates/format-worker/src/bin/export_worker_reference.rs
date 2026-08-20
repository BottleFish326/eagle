use std::ffi::OsString;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Component, Path};
use std::process::ExitCode;

use format_worker::{digest_file_sha256, open_libheif_worker_bundle};
use sha2::{Digest, Sha256};
use uuid::Uuid;

const REFERENCE_EDGE: u32 = 64;

fn main() -> ExitCode {
    match run(&std::env::args_os().skip(1).collect::<Vec<_>>()) {
        Ok(message) => {
            println!("{message}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("worker reference export failed: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run(arguments: &[OsString]) -> Result<String, String> {
    let [bundle, fixture_root, source_relative, output_relative] = arguments else {
        return Err(
            "usage: material-eagle-export-worker-reference <bundle-directory> <fixture-root> <source-relative-path> <output-relative-path>"
                .into(),
        );
    };
    let root = fs::canonicalize(fixture_root).map_err(|error| error.to_string())?;
    if !root.is_dir() {
        return Err("fixture root is not a directory".into());
    }
    let source_relative = Path::new(source_relative);
    let output_relative = Path::new(output_relative);
    validate_relative(source_relative)?;
    validate_relative(output_relative)?;
    let source = root.join(source_relative);
    let output = root.join(output_relative);

    let worker =
        open_libheif_worker_bundle(Path::new(bundle)).map_err(|error| error.to_string())?;
    let source_digest = digest_file_sha256(&source).map_err(|error| error.to_string())?;
    let success = worker
        .thumbnail_request(Uuid::now_v7(), &source, &root, REFERENCE_EDGE)
        .and_then(|request| worker.execute(&request, &root))
        .map_err(|error| error.to_string())?;
    if digest_file_sha256(&source).map_err(|error| error.to_string())? != source_digest {
        return Err("worker changed the source while exporting a reference".into());
    }
    let png = success
        .png
        .ok_or_else(|| "worker thumbnail did not contain PNG bytes".to_owned())?;
    let dimensions = success
        .png_dimensions
        .ok_or_else(|| "worker thumbnail did not contain dimensions".to_owned())?;
    if dimensions.0 > REFERENCE_EDGE || dimensions.1 > REFERENCE_EDGE {
        return Err("worker thumbnail exceeds the fixed reference edge".into());
    }

    let parent = output
        .parent()
        .ok_or_else(|| "output path has no parent".to_owned())?;
    let canonical_parent = fs::canonicalize(parent).map_err(|error| error.to_string())?;
    if !canonical_parent.starts_with(&root) {
        return Err("output path escapes the fixture root".into());
    }
    if let Ok(metadata) = fs::symlink_metadata(&output) {
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err("output is not a non-symbolic-link regular file".into());
        }
        let existing = fs::read(&output).map_err(|error| error.to_string())?;
        if existing != png {
            return Err("existing reference differs from worker output".into());
        }
    } else {
        let temporary = output.with_extension(format!("export-{}.tmp", Uuid::now_v7()));
        let result = (|| {
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temporary)?;
            file.write_all(&png)?;
            file.sync_all()?;
            drop(file);
            fs::rename(&temporary, &output)
        })();
        if let Err(error) = result {
            let _ = fs::remove_file(&temporary);
            return Err(error.to_string());
        }
    }

    let digest = Sha256::digest(&png);
    Ok(format!(
        "{digest:x} {}x{} {}",
        dimensions.0,
        dimensions.1,
        output_relative.display()
    ))
}

fn validate_relative(path: &Path) -> Result<(), String> {
    if path.as_os_str().is_empty()
        || path.is_absolute()
        || !path
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
    {
        return Err("fixture paths must contain only relative normal components".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::validate_relative;
    use std::path::Path;

    #[test]
    fn reference_export_paths_cannot_escape_the_fixture_root() {
        assert!(validate_relative(Path::new("sources/avif/example.avif")).is_ok());
        for path in ["", "../example.avif", "a/../b.png"] {
            assert!(validate_relative(Path::new(path)).is_err(), "{path}");
        }
        #[cfg(unix)]
        assert!(validate_relative(Path::new("/tmp/example.avif")).is_err());
        #[cfg(windows)]
        assert!(validate_relative(Path::new(r"C:\tmp\example.avif")).is_err());
    }
}

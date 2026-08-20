use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;

use asset_core::AssetRecord;
use asset_preview::{ThumbnailOutcome, ThumbnailService};

fn main() {
    let mut arguments = std::env::args_os().skip(1);
    let source = PathBuf::from(arguments.next().expect("source path argument"));
    let output = PathBuf::from(arguments.next().expect("output path argument"));
    assert!(
        arguments.next().is_none(),
        "expected source and output paths"
    );
    let source = source.canonicalize().expect("canonical source");
    let metadata = fs::metadata(&source).expect("source metadata");
    let cache = tempfile::tempdir().expect("temporary preview cache");
    let service = ThumbnailService::open(cache.path(), 1).expect("preview service");
    let record = AssetRecord::untagged(
        source.to_string_lossy().into_owned(),
        source,
        "image/png".into(),
        metadata.len(),
        0,
    );
    let ThumbnailOutcome::Ready { thumbnail } =
        service.request(&record, 16).expect("preview request")
    else {
        panic!("raster preview was not generated");
    };
    let bytes = service.read(&thumbnail.cache_key).expect("preview bytes");
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output)
        .expect("create new reference PNG without following an existing link");
    output.write_all(&bytes).expect("write reference PNG");
    output.sync_all().expect("sync reference PNG");
}

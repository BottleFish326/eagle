use std::collections::BTreeSet;
use std::fs::{self, File};
use std::io::BufWriter;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, ensure};
use asset_core::AssetRecord;
use asset_preview::{ThumbnailOutcome, ThumbnailService};
use clap::{Parser, Subcommand};
use image::{DynamicImage, ImageBuffer, ImageFormat, Rgba};
use metadata::{
    AssetSidecar, ExpectedVersion, digest_file, sidecar_path_for, write_sidecar_atomic,
};
use serde::{Deserialize, Serialize};

const MANIFEST: &str = "cache-fault-expected.json";

#[derive(Debug, Parser)]
#[command(about = "Crash-only harness for thumbnail cache lifecycle acceptance tests")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Seed { workspace: PathBuf },
    Clear { workspace: PathBuf },
    Recover { workspace: PathBuf },
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExpectedState {
    asset_digest: String,
    sidecar_digest: String,
}

fn main() -> Result<()> {
    match Cli::parse().command {
        Command::Seed { workspace } => seed(&workspace),
        Command::Clear { workspace } => clear(&workspace),
        Command::Recover { workspace } => recover(&workspace),
    }
}

fn seed(workspace: &Path) -> Result<()> {
    fs::create_dir_all(workspace)
        .with_context(|| format!("create workspace {}", workspace.display()))?;
    ensure!(
        fs::read_dir(workspace)?.next().is_none(),
        "seed workspace must be empty"
    );
    let asset = workspace.join("asset.png");
    write_image(&asset)?;
    let sidecar_path = sidecar_path_for(&asset);
    let mut sidecar = AssetSidecar::new();
    sidecar.tags = BTreeSet::from(["acceptance/cache-safety".into()]);
    sidecar.rating = 5;
    sidecar.favorite = true;
    sidecar.note = "P2-A10 user metadata must survive cache cleanup".into();
    write_sidecar_atomic(&sidecar_path, &sidecar, &ExpectedVersion::Missing)?;
    let expected = ExpectedState {
        asset_digest: digest_file(&asset)?,
        sidecar_digest: digest_file(&sidecar_path)?,
    };
    fs::write(
        workspace.join(MANIFEST),
        serde_json::to_vec_pretty(&expected)?,
    )?;
    let service = ThumbnailService::open(&workspace.join("cache"), 1)?;
    let outcome = service.request(&record(&asset, sidecar.id)?, 64)?;
    ensure!(matches!(outcome, ThumbnailOutcome::Ready { .. }));
    ensure!(service.cache_stats()?.entry_count == 1);
    println!("seeded cache=1 asset=true sidecar=true");
    Ok(())
}

fn clear(workspace: &Path) -> Result<()> {
    verify_user_files(workspace)?;
    let service = ThumbnailService::open(&workspace.join("cache"), 1)?;
    let report = service.clear()?;
    println!(
        "cleared files={} bytes={}",
        report.removed_files, report.removed_bytes
    );
    Ok(())
}

fn recover(workspace: &Path) -> Result<()> {
    verify_user_files(workspace)?;
    let asset = workspace.join("asset.png");
    let (sidecar, _) = metadata::read_sidecar(&sidecar_path_for(&asset))?;
    let service = ThumbnailService::open(&workspace.join("cache"), 1)?;
    ensure!(service.cache_stats()?.entry_count == 0);
    let outcome = service.request(&record(&asset, sidecar.id)?, 64)?;
    let ThumbnailOutcome::Ready { thumbnail } = outcome else {
        anyhow::bail!("thumbnail was not rebuilt")
    };
    ensure!(!thumbnail.cache_hit, "recovery must rebuild the thumbnail");
    ensure!(service.cache_stats()?.entry_count == 1);
    verify_user_files(workspace)?;
    println!(
        "recovered disposition={} cache=1 asset=true sidecar=true",
        service.startup_report().disposition
    );
    Ok(())
}

fn verify_user_files(workspace: &Path) -> Result<()> {
    let expected: ExpectedState = serde_json::from_slice(&fs::read(workspace.join(MANIFEST))?)?;
    let asset = workspace.join("asset.png");
    let sidecar = sidecar_path_for(&asset);
    ensure!(digest_file(&asset)? == expected.asset_digest);
    ensure!(digest_file(&sidecar)? == expected.sidecar_digest);
    Ok(())
}

fn record(path: &Path, id: uuid::Uuid) -> Result<AssetRecord> {
    let metadata = fs::metadata(path)?;
    let mut record = AssetRecord::untagged(
        path.to_string_lossy().into_owned(),
        path.to_path_buf(),
        "image/png".into(),
        metadata.len(),
        0,
    );
    record.id = Some(id);
    Ok(record)
}

fn write_image(path: &Path) -> Result<()> {
    let pixels = ImageBuffer::from_pixel(80, 40, Rgba([31_u8, 111, 235, 255]));
    DynamicImage::ImageRgba8(pixels)
        .write_to(BufWriter::new(File::create(path)?), ImageFormat::Png)?;
    Ok(())
}

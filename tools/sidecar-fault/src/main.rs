use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Parser;
use metadata::{AssetSidecar, ExpectedVersion, read_sidecar, write_sidecar_atomic};

#[derive(Debug, Parser)]
#[command(about = "Crash-only harness for sidecar atomic-write acceptance tests")]
struct Cli {
    sidecar: PathBuf,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let (mut sidecar, expected) = if cli.sidecar.is_file() {
        let (sidecar, digest) = read_sidecar(&cli.sidecar).context("read existing sidecar")?;
        (sidecar, ExpectedVersion::Digest(digest))
    } else {
        (AssetSidecar::new(), ExpectedVersion::Missing)
    };
    sidecar.note = "fault harness replacement".into();
    sidecar.touch();
    let receipt = write_sidecar_atomic(&cli.sidecar, &sidecar, &expected)?;
    println!("persisted {}", receipt.path.display());
    Ok(())
}

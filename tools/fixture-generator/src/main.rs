use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand, ValueEnum};
use metadata::{AssetSidecar, sidecar_path_for};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

const MARKER_NAME: &str = ".eagle-fixture-manifest.json";
const GENERATOR_ID: &str = "eagle-fixture-generator";
const PNG: &[u8] = &[
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x04, 0x00, 0x00, 0x00, 0xb5, 0x1c, 0x0c,
    0x02, 0x00, 0x00, 0x00, 0x0b, 0x49, 0x44, 0x41, 0x54, 0x78, 0xda, 0x63, 0x64, 0xf8, 0x0f, 0x00,
    0x01, 0x05, 0x01, 0x01, 0x27, 0x18, 0xe3, 0x66, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44,
    0xae, 0x42, 0x60, 0x82,
];

#[derive(Debug, Parser)]
#[command(about = "Generate protected deterministic phase-zero fixture libraries")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Generate {
        output: PathBuf,
        #[arg(long, value_enum, default_value_t = Scale::Small)]
        scale: Scale,
        #[arg(long)]
        count: Option<usize>,
    },
    Clean {
        output: PathBuf,
    },
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum Scale {
    Small,
    Medium,
    Large,
}

impl Scale {
    const fn count(self) -> usize {
        match self {
            Self::Small => 1_000,
            Self::Medium => 10_000,
            Self::Large => 100_000,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FixtureManifest {
    schema: u32,
    generator: String,
    count: usize,
    sidecar_count: usize,
    seed: u64,
}

fn main() -> Result<()> {
    match Cli::parse().command {
        Command::Generate {
            output,
            scale,
            count,
        } => generate(&output, count.unwrap_or_else(|| scale.count())),
        Command::Clean { output } => clean(&output),
    }
}

fn generate(output: &Path, count: usize) -> Result<()> {
    if count == 0 {
        bail!("count must be greater than zero");
    }
    prepare_output(output)?;
    let mut sidecar_count = 0;

    for index in 0..count {
        let directory = fixture_directory(output, index);
        fs::create_dir_all(&directory)
            .with_context(|| format!("create fixture directory {}", directory.display()))?;
        let file_name = if index % 997 == 0 {
            format!("中文 空格 🦅-{index:06}.png")
        } else {
            format!("asset-{index:06}.png")
        };
        let asset_path = directory.join(file_name);
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&asset_path)
            .with_context(|| format!("create fixture {}", asset_path.display()))?;
        if index % 5_003 != 0 {
            file.write_all(PNG)?;
            if index % 10_000 != 0 {
                file.write_all(format!("fixture:{index}").as_bytes())?;
            }
        }
        if index % 5 == 0 {
            let sidecar_path = sidecar_path_for(&asset_path);
            if index % 3_331 == 0 && index != 0 {
                fs::write(&sidecar_path, "schema: [broken\n")?;
            } else {
                let mut sidecar = AssetSidecar::with_id(deterministic_uuid_v7(index as u64));
                sidecar.tags.insert(if index % 2 == 0 {
                    "group/even".into()
                } else {
                    "group/odd".into()
                });
                if index % 11 == 0 {
                    sidecar.tags.insert("state/draft".into());
                }
                sidecar.favorite = index % 13 == 0;
                sidecar.updated_at = "2026-08-14T00:00:00Z".into();
                fs::write(&sidecar_path, serde_yaml_ng::to_string(&sidecar)?)?;
            }
            sidecar_count += 1;
        }
    }

    fs::write(output.join("unsupported.txt"), "not a material asset\n")?;
    let manifest = FixtureManifest {
        schema: 1,
        generator: GENERATOR_ID.into(),
        count,
        sidecar_count,
        seed: 0x00EA_61E0,
    };
    fs::write(
        output.join(MARKER_NAME),
        format!("{}\n", serde_json::to_string_pretty(&manifest)?),
    )?;
    println!("generated {count} assets in {}", output.display());
    println!("generated {sidecar_count} sidecars");
    Ok(())
}

fn clean(output: &Path) -> Result<()> {
    let marker = output.join(MARKER_NAME);
    let bytes = fs::read(&marker)
        .with_context(|| format!("refusing cleanup: missing marker {}", marker.display()))?;
    let manifest: FixtureManifest = serde_json::from_slice(&bytes).context("invalid marker")?;
    if manifest.generator != GENERATOR_ID {
        bail!("refusing cleanup: marker was not created by this generator");
    }
    let canonical = output
        .canonicalize()
        .context("canonicalize cleanup target")?;
    if canonical.parent().is_none() || canonical == Path::new("/") {
        bail!("refusing cleanup of a broad filesystem target");
    }
    fs::remove_dir_all(&canonical)
        .with_context(|| format!("remove generated fixture {}", canonical.display()))?;
    println!("removed generated fixture {}", canonical.display());
    Ok(())
}

fn prepare_output(output: &Path) -> Result<()> {
    if output.exists() {
        let mut entries = fs::read_dir(output)
            .with_context(|| format!("read output directory {}", output.display()))?;
        if entries.next().is_some() {
            bail!("output directory must be empty: {}", output.display());
        }
    } else {
        fs::create_dir_all(output)
            .with_context(|| format!("create output directory {}", output.display()))?;
    }
    Ok(())
}

fn fixture_directory(root: &Path, index: usize) -> PathBuf {
    if index % 7_919 == 0 {
        root.join("deep/a/b/c/d/e")
            .join(format!("group-{:03}", index / 1_000))
    } else {
        root.join(format!("group-{:03}", index / 1_000))
    }
}

fn deterministic_uuid_v7(value: u64) -> Uuid {
    let mut bytes = [0_u8; 16];
    bytes[..8].copy_from_slice(&value.to_be_bytes());
    bytes[8..].copy_from_slice(&value.rotate_left(17).to_be_bytes());
    bytes[6] = (bytes[6] & 0x0f) | 0x70;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Uuid::from_bytes(bytes)
}

#[cfg(test)]
mod tests {
    use super::deterministic_uuid_v7;

    #[test]
    fn deterministic_ids_are_unique_version_seven_uuids() {
        let first = deterministic_uuid_v7(1);
        let second = deterministic_uuid_v7(2);
        assert_ne!(first, second);
        assert_eq!(first.get_version_num(), 7);
        assert_eq!(second.get_version_num(), 7);
    }
}

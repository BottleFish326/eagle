use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result, ensure};
use asset_transactions::{MetadataTransactionStore, TransactionState, TransactionTarget};
use clap::{Parser, Subcommand};
use metadata::{MetadataPatch, read_sidecar, sidecar_path_for};
use uuid::Uuid;

#[derive(Debug, Parser)]
#[command(about = "Crash-only harness for batch metadata transaction acceptance tests")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Execute {
        workspace: PathBuf,
        #[arg(long, default_value_t = 1_000)]
        count: usize,
    },
    Recover {
        workspace: PathBuf,
        #[arg(long, default_value_t = 1_000)]
        count: usize,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Execute { workspace, count } => execute(&workspace, count),
        Command::Recover { workspace, count } => recover(&workspace, count),
    }
}

fn execute(workspace: &std::path::Path, count: usize) -> Result<()> {
    fs::create_dir_all(workspace)
        .with_context(|| format!("create workspace {}", workspace.display()))?;
    let targets = create_targets(workspace, count)?;
    let store = MetadataTransactionStore::open(workspace.join("transactions"))?;
    let result = store.execute(
        &targets,
        &MetadataPatch {
            add_tags: BTreeSet::from(["batch/process-recovered".into()]),
            ..MetadataPatch::default()
        },
    )?;
    println!(
        "completed {} {}",
        result.summary.id, result.summary.applied_count
    );
    Ok(())
}

fn recover(workspace: &std::path::Path, count: usize) -> Result<()> {
    let store = MetadataTransactionStore::open(workspace.join("transactions"))?;
    let transactions = store.list()?;
    ensure!(transactions.len() == 1, "expected one transaction journal");
    let discovered = &transactions[0];
    println!(
        "discovered {} {:?} applied={}",
        discovered.id, discovered.state, discovered.applied_count
    );
    let result = store.continue_transaction(discovered.id)?;
    ensure!(result.summary.state == TransactionState::Completed);
    ensure!(result.summary.applied_count == count);
    ensure!(result.failures.is_empty());
    for index in 0..count {
        let asset_path = workspace.join(format!("asset-{index:04}.png"));
        let (sidecar, _) = read_sidecar(&sidecar_path_for(&asset_path))?;
        ensure!(sidecar.tags.contains("batch/process-recovered"));
    }
    println!("recovered {} {}", result.summary.id, count);
    Ok(())
}

fn create_targets(workspace: &std::path::Path, count: usize) -> Result<Vec<TransactionTarget>> {
    let root_id = Uuid::now_v7();
    (0..count)
        .map(|index| {
            let asset_path = workspace.join(format!("asset-{index:04}.png"));
            fs::write(&asset_path, format!("asset {index}"))
                .with_context(|| format!("write {}", asset_path.display()))?;
            Ok(TransactionTarget {
                key: asset_path.to_string_lossy().into_owned(),
                root_id,
                asset_path,
                expected_sidecar_digest: None,
                expected_sidecar_size: None,
                expected_sidecar_modified_unix_ms: None,
            })
        })
        .collect()
}

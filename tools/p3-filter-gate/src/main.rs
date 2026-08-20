use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail, ensure};
use asset_filesystem::{ScanCancellation, ScanOptions, scan_root_incremental};
use asset_saved_filters::{
    CreateSavedFilter, SavedFilter, SavedFilterFileVersion, SavedFilterScope, SavedFilterSort,
    SavedFilterSortDirection, SavedFilterSortField, SavedFilterStore, SavedFilterStoreError,
    SavedFilterTagChoice, SavedFilterTagChoiceAction, execute_saved_filter,
};
use asset_tag_renames::{
    TagRenameCoordinator, TagRenameFilterOutcome, TagRenameRequest, TagRenameState,
};
use asset_transactions::{MetadataTransactionStore, TransactionTarget};
use clap::{Parser, Subcommand, ValueEnum};
use metadata::{MetadataPatch, digest_file, edit_asset_metadata, read_sidecar, sidecar_path_for};
use serde::Serialize;
use sha2::{Digest, Sha256};
use uuid::Uuid;

const ACTIVE_ROOT_ID: &str = "019b76d0-0000-7000-8000-000000000001";
const OFFLINE_ROOT_ID: &str = "019b76d0-0000-7000-8000-000000000002";

#[derive(Debug, Parser)]
#[command(about = "P3-A04/A05 saved-filter and Tag-rename acceptance harness")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    FilterSeed {
        workspace: PathBuf,
    },
    FilterMutateCurrent {
        workspace: PathBuf,
    },
    FilterVerify {
        workspace: PathBuf,
    },
    FilterAdversarial {
        workspace: PathBuf,
    },
    RenameExecute {
        workspace: PathBuf,
        #[arg(long, default_value_t = 64)]
        count: usize,
    },
    RenameRecover {
        workspace: PathBuf,
        #[arg(long, default_value_t = 64)]
        count: usize,
        #[arg(long, value_enum)]
        action: RecoveryAction,
        #[arg(long)]
        external_filter: bool,
        #[arg(long)]
        external_sidecar: bool,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum RecoveryAction {
    Continue,
    Retain,
    Restore,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct FilterSeedReceipt {
    schema: u32,
    active_root_id: Uuid,
    offline_root_id: Uuid,
    asset_count: usize,
    saved_filter_count: usize,
    cache_created: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
#[allow(clippy::struct_excessive_bools)]
struct FilterVerifyReceipt {
    schema: u32,
    accepted: bool,
    scanned_asset_count: usize,
    scan_problem_count: usize,
    all_enabled_match_count: usize,
    selected_roots_match_count: usize,
    selected_missing_root_count: usize,
    cache_absent: bool,
    result_snapshot_absent: bool,
    source_sha256_unchanged: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
#[allow(clippy::struct_excessive_bools)]
struct AdversarialReceipt {
    schema: u32,
    accepted: bool,
    valid_count: usize,
    unavailable_count: usize,
    invalid_count: usize,
    unknown_fields_preserved: bool,
    external_change_blocked: bool,
    external_bytes_preserved: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct RenameReceipt {
    schema: u32,
    operation_id: Uuid,
    state: TagRenameState,
    filter_outcome: TagRenameFilterOutcome,
    action: String,
    asset_count: usize,
    source_sha256_unchanged: bool,
    external_filter_preserved: bool,
    external_sidecar_preserved: bool,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::FilterSeed { workspace } => filter_seed(&workspace),
        Command::FilterMutateCurrent { workspace } => filter_mutate_current(&workspace),
        Command::FilterVerify { workspace } => filter_verify(&workspace),
        Command::FilterAdversarial { workspace } => filter_adversarial(&workspace),
        Command::RenameExecute { workspace, count } => rename_execute(&workspace, count),
        Command::RenameRecover {
            workspace,
            count,
            action,
            external_filter,
            external_sidecar,
        } => rename_recover(&workspace, count, action, external_filter, external_sidecar),
    }
}

fn filter_seed(workspace: &Path) -> Result<()> {
    create_new_workspace(workspace)?;
    let root = workspace.join("library");
    fs::create_dir(&root).context("create filter library")?;
    for (index, tags) in [
        BTreeSet::from(["project/red".to_owned()]),
        BTreeSet::from(["project/red".to_owned()]),
        BTreeSet::from(["project/blue".to_owned()]),
        BTreeSet::from(["project/blue".to_owned(), "project/red".to_owned()]),
    ]
    .into_iter()
    .enumerate()
    {
        create_svg_with_tags(&root, index, tags)?;
    }
    let store = SavedFilterStore::new(workspace.join("saved-filters.yml"));
    let first = store.create(
        &SavedFilterFileVersion::expected_absent(),
        filter_input("All red", "project/red", SavedFilterScope::AllEnabledRoots),
    )?;
    store.create(
        &first.file_version,
        filter_input(
            "Selected red or blue",
            "any:(project/red|project/blue)",
            SavedFilterScope::SelectedRoots {
                root_ids: vec![active_root_id()?, offline_root_id()?],
            },
        ),
    )?;
    let cache = workspace.join("derived-cache");
    fs::create_dir(&cache).context("create derived cache")?;
    fs::write(cache.join("snapshot.bin"), b"derived-only").context("write cache sentinel")?;
    write_json(&FilterSeedReceipt {
        schema: 1,
        active_root_id: active_root_id()?,
        offline_root_id: offline_root_id()?,
        asset_count: 4,
        saved_filter_count: 2,
        cache_created: true,
    })
}

fn filter_mutate_current(workspace: &Path) -> Result<()> {
    ensure!(workspace.is_dir(), "filter workspace is missing");
    create_svg_with_tags(
        &workspace.join("library"),
        4,
        BTreeSet::from(["project/red".to_owned()]),
    )?;
    write_json(&serde_json::json!({ "schema": 1, "assetCount": 5 }))
}

fn filter_verify(workspace: &Path) -> Result<()> {
    let root_id = active_root_id()?;
    let mut records = Vec::new();
    let mut problem_count = 0;
    scan_root_incremental(
        Some(root_id),
        &workspace.join("library"),
        &ScanOptions::default(),
        &ScanCancellation::new(),
        |batch| {
            records.extend(batch.assets);
            problem_count += batch.problems.len();
        },
    )?;
    let store = SavedFilterStore::new(workspace.join("saved-filters.yml"));
    let catalog = store.load(&BTreeSet::from([root_id]))?;
    let filters = catalog
        .valid_filters
        .iter()
        .chain(
            catalog
                .unavailable_filters
                .iter()
                .map(|entry| &entry.filter),
        )
        .collect::<Vec<_>>();
    let all = named_filter(&filters, "All red")?;
    let selected = named_filter(&filters, "Selected red or blue")?;
    let enabled = BTreeSet::from([root_id, offline_root_id()?]);
    let available = BTreeSet::from([root_id]);
    let all_execution = execute_saved_filter(all, &records, &enabled, &available)?;
    let selected_execution = execute_saved_filter(selected, &records, &enabled, &available)?;
    let yaml = fs::read_to_string(store.path()).context("read saved filter YAML")?;
    let result_snapshot_absent = [
        "assetKey",
        "relativePath",
        "resultKeys",
        "thumbnail",
        "indexSnapshot",
    ]
    .iter()
    .all(|forbidden| !yaml.contains(forbidden));
    let source_sha256_unchanged = (0..5).all(|index| {
        digest_file(&workspace.join("library").join(format!("asset-{index}.svg")))
            .is_ok_and(|actual| actual == digest_bytes(svg_content(index).as_bytes()))
    });
    let receipt = FilterVerifyReceipt {
        schema: 1,
        accepted: records.len() == 5
            && problem_count == 0
            && all_execution.matched_assets == 4
            && selected_execution.matched_assets == 5
            && selected_execution.missing_root_ids == vec![offline_root_id()?]
            && !workspace.join("derived-cache").exists()
            && result_snapshot_absent
            && source_sha256_unchanged,
        scanned_asset_count: records.len(),
        scan_problem_count: problem_count,
        all_enabled_match_count: all_execution.matched_assets,
        selected_roots_match_count: selected_execution.matched_assets,
        selected_missing_root_count: selected_execution.missing_root_ids.len(),
        cache_absent: !workspace.join("derived-cache").exists(),
        result_snapshot_absent,
        source_sha256_unchanged,
    };
    ensure!(receipt.accepted, "saved-filter restart/cache gate rejected");
    write_json(&receipt)
}

#[allow(clippy::too_many_lines)]
fn filter_adversarial(workspace: &Path) -> Result<()> {
    create_new_workspace(workspace)?;
    let path = workspace.join("saved-filters.yml");
    let valid_id = "019b76d0-1000-7000-8000-000000000001";
    let offline_id = "019b76d0-1000-7000-8000-000000000002";
    let invalid_query_id = "019b76d0-1000-7000-8000-000000000003";
    let duplicate_id = "019b76d0-1000-7000-8000-000000000004";
    let duplicate_name_a = "019b76d0-1000-7000-8000-000000000005";
    let duplicate_name_b = "019b76d0-1000-7000-8000-000000000006";
    let unknown_sort_id = "019b76d0-1000-7000-8000-000000000007";
    let entry = |id: &str, name: &str, query: &str, scope: &str, sort: &str| {
        format!(
            "  - id: {id}\n    name: {name}\n    query: '{query}'\n    scope: {scope}\n    sort: {sort}\n    createdAt: '2026-08-21T00:00:00.000Z'\n    updatedAt: '2026-08-21T00:00:00.000Z'\n"
        )
    };
    let all_scope = "{kind: all-enabled-roots}";
    let normal_sort = "{field: file-name, direction: ascending}";
    let offline_scope = format!(
        "{{kind: selected-roots, rootIds: [{}]}}",
        offline_root_id()?
    );
    let mut yaml = String::from("schema: 1\nfilters:\n");
    yaml.push_str(&entry(
        valid_id,
        "Valid",
        "project/red",
        all_scope,
        normal_sort,
    ));
    yaml.push_str("    futureEntry: keep\n");
    yaml.push_str(&entry(
        offline_id,
        "Offline",
        "project/blue",
        &offline_scope,
        normal_sort,
    ));
    yaml.push_str(&entry(
        invalid_query_id,
        "Invalid query",
        "any:(broken|)",
        all_scope,
        normal_sort,
    ));
    yaml.push_str(&entry(
        duplicate_id,
        "Duplicate ID one",
        "project/red",
        all_scope,
        normal_sort,
    ));
    yaml.push_str(&entry(
        duplicate_id,
        "Duplicate ID two",
        "project/blue",
        all_scope,
        normal_sort,
    ));
    yaml.push_str(&entry(
        duplicate_name_a,
        "Same name",
        "project/red",
        all_scope,
        normal_sort,
    ));
    yaml.push_str(&entry(
        duplicate_name_b,
        "Same name",
        "project/blue",
        all_scope,
        normal_sort,
    ));
    yaml.push_str(&entry(
        unknown_sort_id,
        "Unknown sort",
        "project/red",
        all_scope,
        "{field: future-order, direction: ascending}",
    ));
    yaml.push_str("futureTop:\n  keep: true\n");
    fs::write(&path, yaml).context("write adversarial filter catalog")?;
    let store = SavedFilterStore::new(path.clone());
    let initial = store.load(&BTreeSet::from([active_root_id()?]))?;
    let renamed = store.rename(
        &initial.file_version,
        Uuid::parse_str(valid_id)?,
        "Valid renamed".into(),
    )?;
    let after_rename = fs::read_to_string(&path).context("read preserved YAML")?;
    let unknown_fields_preserved =
        after_rename.contains("futureEntry: keep") && after_rename.contains("futureTop:");
    fs::write(&path, format!("{after_rename}externalTop: true\n"))
        .context("external filter edit")?;
    let before_stale_attempt = fs::read(&path).context("external bytes")?;
    let external_change_blocked = matches!(
        store.rename(
            &renamed.file_version,
            Uuid::parse_str(valid_id)?,
            "Must not overwrite".into(),
        ),
        Err(SavedFilterStoreError::ExternalChange { .. })
    );
    let external_bytes_preserved = fs::read(&path)? == before_stale_attempt;
    let receipt = AdversarialReceipt {
        schema: 1,
        accepted: initial.valid_filters.len() == 1
            && initial.unavailable_filters.len() == 1
            && initial.invalid_entries.len() == 6
            && unknown_fields_preserved
            && external_change_blocked
            && external_bytes_preserved,
        valid_count: initial.valid_filters.len(),
        unavailable_count: initial.unavailable_filters.len(),
        invalid_count: initial.invalid_entries.len(),
        unknown_fields_preserved,
        external_change_blocked,
        external_bytes_preserved,
    };
    ensure!(receipt.accepted, "adversarial saved-filter gate rejected");
    write_json(&receipt)
}

fn rename_execute(workspace: &Path, count: usize) -> Result<()> {
    ensure!(count > 0, "rename count must be positive");
    create_new_workspace(workspace)?;
    let root = workspace.join("library");
    fs::create_dir(&root).context("create rename library")?;
    let root_id = active_root_id()?;
    let mut targets = Vec::with_capacity(count);
    for index in 0..count {
        let tags = if index % 2 == 0 {
            BTreeSet::from(["new".to_owned(), "old".to_owned()])
        } else {
            BTreeSet::from(["old".to_owned()])
        };
        let edit = create_svg_with_tags(&root, index, tags)?;
        let asset_path = root.join(format!("asset-{index}.svg"));
        targets.push(TransactionTarget {
            key: format!("asset-{index}"),
            root_id,
            asset_path,
            expected_sidecar_digest: Some(edit.digest),
            expected_sidecar_size: Some(edit.size),
            expected_sidecar_modified_unix_ms: Some(edit.modified_unix_ms),
        });
    }
    let filters = SavedFilterStore::new(workspace.join("saved-filters.yml"));
    let created = filters.create(
        &SavedFilterFileVersion::expected_absent(),
        filter_input(
            "Rename exact nodes",
            "old -old any:(old|other) path:old older",
            SavedFilterScope::AllEnabledRoots,
        ),
    )?;
    let filter_id = created.filter.context("created filter missing")?.id;
    let transactions = MetadataTransactionStore::open(workspace.join("transactions"))?;
    let coordinator = TagRenameCoordinator::open(workspace.join("tag-renames-v1"))?;
    let completed = coordinator.start(
        &transactions,
        &filters,
        TagRenameRequest {
            old_tag: "old".into(),
            new_tag: "new".into(),
            catalog_revision: 84,
            targets,
            saved_filter_version: created.file_version,
            saved_filter_choices: vec![SavedFilterTagChoice {
                filter_id,
                action: SavedFilterTagChoiceAction::Update,
            }],
        },
    )?;
    ensure!(completed.state == TagRenameState::Completed);
    println!("completed {} {count}", completed.id);
    Ok(())
}

fn rename_recover(
    workspace: &Path,
    count: usize,
    action: RecoveryAction,
    external_filter: bool,
    external_sidecar: bool,
) -> Result<()> {
    let filters = SavedFilterStore::new(workspace.join("saved-filters.yml"));
    let transactions = MetadataTransactionStore::open(workspace.join("transactions"))?;
    let coordinator = TagRenameCoordinator::open(workspace.join("tag-renames-v1"))?;
    let discovered = coordinator.list(&transactions, &filters)?;
    ensure!(discovered.len() == 1, "expected one coordinator journal");
    let id = discovered[0].id;
    if external_filter {
        let source =
            fs::read_to_string(filters.path()).context("read filter before external edit")?;
        fs::write(filters.path(), format!("{source}externalMarker: true\n"))
            .context("write external filter marker")?;
    }
    if external_sidecar {
        let asset = workspace.join("library/asset-0.svg");
        let (_, digest) = read_sidecar(&sidecar_path_for(&asset))?;
        edit_asset_metadata(
            &asset,
            Some(&digest),
            &MetadataPatch {
                add_tags: BTreeSet::from(["external".into()]),
                ..MetadataPatch::default()
            },
        )?;
    }
    let result = match action {
        RecoveryAction::Continue => coordinator.continue_operation(&transactions, &filters, id)?,
        RecoveryAction::Retain => coordinator.retain_filters(&transactions, &filters, id)?,
        RecoveryAction::Restore => coordinator.restore(&transactions, &filters, id)?,
    };
    let conflict_expected = external_filter || external_sidecar;
    ensure!(
        result.state
            == if conflict_expected {
                TagRenameState::Conflict
            } else if action == RecoveryAction::Restore {
                TagRenameState::Restored
            } else {
                TagRenameState::Completed
            },
        "unexpected recovered state {:?}",
        result.state
    );
    if !conflict_expected {
        verify_recovered_files(workspace, count, action)?;
    }
    let external_filter_preserved = !external_filter
        || fs::read_to_string(filters.path())
            .context("read external filter")?
            .contains("externalMarker: true");
    let external_sidecar_preserved = !external_sidecar
        || read_sidecar(&workspace.join("library/asset-0.svg.asset.yml"))?
            .0
            .tags
            .contains("external");
    let source_sha256_unchanged = verify_source_hashes(workspace, count)?;
    ensure!(external_filter_preserved && external_sidecar_preserved && source_sha256_unchanged);
    write_json(&RenameReceipt {
        schema: 1,
        operation_id: result.id,
        state: result.state,
        filter_outcome: result.filter_outcome,
        action: format!("{action:?}").to_lowercase(),
        asset_count: count,
        source_sha256_unchanged,
        external_filter_preserved,
        external_sidecar_preserved,
    })
}

fn verify_recovered_files(workspace: &Path, count: usize, action: RecoveryAction) -> Result<()> {
    let restored = action == RecoveryAction::Restore;
    for index in 0..count {
        let asset = workspace.join("library").join(format!("asset-{index}.svg"));
        let (sidecar, _) = read_sidecar(&sidecar_path_for(&asset))?;
        if restored {
            ensure!(sidecar.tags.contains("old"));
            ensure!(sidecar.tags.contains("new") == (index % 2 == 0));
        } else {
            ensure!(!sidecar.tags.contains("old"));
            ensure!(sidecar.tags.contains("new"));
        }
    }
    let store = SavedFilterStore::new(workspace.join("saved-filters.yml"));
    let catalog = store.load(&BTreeSet::from([active_root_id()?]))?;
    let query = &catalog.valid_filters[0].query;
    if restored || action == RecoveryAction::Retain {
        ensure!(query == "old -old any:(old|other) path:old older");
    } else {
        ensure!(query == "new -tag:new any:(new|other) path:old older");
    }
    Ok(())
}

fn verify_source_hashes(workspace: &Path, count: usize) -> Result<bool> {
    for index in 0..count {
        let path = workspace.join("library").join(format!("asset-{index}.svg"));
        if digest_file(&path)? != digest_bytes(svg_content(index).as_bytes()) {
            return Ok(false);
        }
    }
    Ok(true)
}

fn create_svg_with_tags(
    root: &Path,
    index: usize,
    tags: BTreeSet<String>,
) -> Result<metadata::MetadataEdit> {
    let asset_path = root.join(format!("asset-{index}.svg"));
    ensure!(!asset_path.exists(), "asset already exists");
    fs::write(&asset_path, svg_content(index))
        .with_context(|| format!("write {}", asset_path.display()))?;
    edit_asset_metadata(
        &asset_path,
        None,
        &MetadataPatch {
            set_tags: Some(tags),
            ..MetadataPatch::default()
        },
    )
    .map_err(Into::into)
}

fn svg_content(index: usize) -> String {
    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{}\" height=\"8\"><rect width=\"100%\" height=\"100%\" fill=\"#{:06x}\"/></svg>\n",
        8 + (index % 8),
        index % 0xFF_FFFF
    )
}

fn filter_input(name: &str, query: &str, scope: SavedFilterScope) -> CreateSavedFilter {
    CreateSavedFilter {
        name: name.into(),
        query: query.into(),
        scope,
        sort: SavedFilterSort {
            field: SavedFilterSortField::FileName,
            direction: SavedFilterSortDirection::Ascending,
        },
    }
}

fn named_filter<'a>(filters: &[&'a SavedFilter], name: &str) -> Result<&'a SavedFilter> {
    filters
        .iter()
        .copied()
        .find(|filter| filter.name == name)
        .with_context(|| format!("saved filter is missing: {name}"))
}

fn create_new_workspace(workspace: &Path) -> Result<()> {
    if workspace.exists() {
        bail!("workspace already exists: {}", workspace.display());
    }
    fs::create_dir(workspace).with_context(|| format!("create workspace {}", workspace.display()))
}

fn active_root_id() -> Result<Uuid> {
    Uuid::parse_str(ACTIVE_ROOT_ID).map_err(Into::into)
}

fn offline_root_id() -> Result<Uuid> {
    Uuid::parse_str(OFFLINE_ROOT_ID).map_err(Into::into)
}

fn digest_bytes(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn write_json(value: &impl Serialize) -> Result<()> {
    serde_json::to_writer_pretty(std::io::stdout().lock(), value)?;
    println!();
    Ok(())
}

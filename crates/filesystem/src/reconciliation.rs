use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use asset_core::{AssetIssue, AssetRecord};
use globset::{GlobBuilder, GlobSet, GlobSetBuilder};
use metadata::{Fingerprint, digest_file, quick_fingerprint_file, read_sidecar, sidecar_path_for};
use serde::Serialize;
use thiserror::Error;
use uuid::Uuid;
use walkdir::WalkDir;

use crate::{FilesystemError, ScanOptions};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum OrphanSidecarState {
    Ready,
    MissingFingerprint,
    Invalid,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OrphanSidecar {
    pub sidecar_id: Option<Uuid>,
    pub sidecar_path: PathBuf,
    pub expected_asset_path: PathBuf,
    pub state: OrphanSidecarState,
    pub message: Option<String>,
    pub candidate_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MissingAsset {
    pub sidecar_id: Option<Uuid>,
    pub expected_asset_path: PathBuf,
    pub sidecar_path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RelinkCandidate {
    pub candidate_id: Uuid,
    pub root_id: Uuid,
    pub sidecar_id: Uuid,
    pub sidecar_path: PathBuf,
    pub sidecar_digest: String,
    pub asset_key: String,
    pub asset_path: PathBuf,
    pub size: u64,
    pub quick_fingerprint: Option<String>,
    pub sha256: String,
    pub ambiguous: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReconciliationReport {
    pub root_id: Uuid,
    pub orphan_sidecars: Vec<OrphanSidecar>,
    pub missing_assets: Vec<MissingAsset>,
    pub pending_moves: Vec<RelinkCandidate>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RelinkReceipt {
    pub candidate_id: Uuid,
    pub sidecar_id: Uuid,
    pub from: PathBuf,
    pub to: PathBuf,
}

#[derive(Debug, Error)]
pub enum ReconciliationError {
    #[error("relink candidate is outside its configured library root")]
    OutsideRoot,
    #[error("relink candidate is stale: {0}")]
    Stale(String),
    #[error("relink destination already exists: {0}")]
    DestinationExists(PathBuf),
    #[error("relink file operation failed at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

/// Inspects one authorized root for Sidecars whose adjacent asset is missing and
/// produces size/quick/SHA-256-confirmed relink candidates without modifying files.
///
/// # Errors
///
/// Returns [`FilesystemError`] when the root or ignore rules are invalid.
pub fn inspect_reconciliation(
    root_id: Uuid,
    root: &Path,
    options: &ScanOptions,
    assets: &[AssetRecord],
) -> Result<ReconciliationReport, FilesystemError> {
    if !root.is_dir() {
        return Err(FilesystemError::InvalidRoot(root.to_path_buf()));
    }
    let root = root
        .canonicalize()
        .map_err(|source| FilesystemError::Canonicalize {
            path: root.to_path_buf(),
            source,
        })?;
    let ignore = compile_ignore_rules(&options.ignore)?;
    let candidates_by_size = unlinked_assets_by_size(assets);
    let mut orphan_sidecars = Vec::new();
    let mut missing_assets = Vec::new();
    let mut pending_moves = Vec::new();

    let mut walker = WalkDir::new(&root).follow_links(false);
    if !options.recursive {
        walker = walker.max_depth(1);
    }
    for entry in walker.into_iter().filter_entry(|entry| {
        should_visit_entry(entry.path(), entry.depth(), &root, options, &ignore)
    }) {
        let Ok(entry) = entry else { continue };
        if !entry.file_type().is_file() || !is_asset_sidecar(entry.path()) {
            continue;
        }
        let sidecar_path = entry.into_path();
        let expected_asset_path = asset_path_for_sidecar(&sidecar_path);
        let mismatched_adjacent_asset = assets.iter().any(|asset| {
            asset.path == expected_asset_path
                && asset
                    .issues
                    .iter()
                    .any(|issue| matches!(issue, AssetIssue::MismatchedSidecar(_)))
        });
        if expected_asset_path.is_file() && !mismatched_adjacent_asset {
            continue;
        }

        let mut diagnostic = OrphanSidecar {
            sidecar_id: None,
            sidecar_path: sidecar_path.clone(),
            expected_asset_path: expected_asset_path.clone(),
            state: OrphanSidecarState::Invalid,
            message: None,
            candidate_count: 0,
        };
        match read_sidecar(&sidecar_path) {
            Ok((sidecar, sidecar_digest)) => {
                diagnostic.sidecar_id = Some(sidecar.id);
                if let Some(fingerprint) = sidecar.fingerprint {
                    let mut candidates = confirmed_candidates(
                        root_id,
                        sidecar.id,
                        &sidecar_path,
                        &sidecar_digest,
                        &fingerprint,
                        &candidates_by_size,
                    );
                    let ambiguous = candidates.len() > 1;
                    for candidate in &mut candidates {
                        candidate.ambiguous = ambiguous;
                    }
                    diagnostic.state = OrphanSidecarState::Ready;
                    diagnostic.candidate_count = candidates.len();
                    pending_moves.extend(candidates);
                } else {
                    diagnostic.state = OrphanSidecarState::MissingFingerprint;
                    diagnostic.message =
                        Some("Sidecar 没有素材指纹；应用不会仅凭文件名或大小猜测关联".into());
                }
            }
            Err(error) => diagnostic.message = Some(error.to_string()),
        }
        missing_assets.push(MissingAsset {
            sidecar_id: diagnostic.sidecar_id,
            expected_asset_path,
            sidecar_path: sidecar_path.clone(),
        });
        orphan_sidecars.push(diagnostic);
    }

    orphan_sidecars.sort_by(|left, right| left.sidecar_path.cmp(&right.sidecar_path));
    missing_assets.sort_by(|left, right| left.expected_asset_path.cmp(&right.expected_asset_path));
    pending_moves.sort_by(|left, right| {
        left.sidecar_path
            .cmp(&right.sidecar_path)
            .then_with(|| left.asset_path.cmp(&right.asset_path))
    });
    Ok(ReconciliationReport {
        root_id,
        orphan_sidecars,
        missing_assets,
        pending_moves,
    })
}

/// Applies one previously displayed candidate after revalidating all paths and hashes.
/// The operation explicitly creates a no-overwrite hard link and then removes the
/// orphan Sidecar source.
///
/// # Errors
///
/// Returns [`ReconciliationError`] when the plan is stale, unsafe, or cannot be renamed.
pub fn apply_relink(
    root: &Path,
    candidate: &RelinkCandidate,
) -> Result<RelinkReceipt, ReconciliationError> {
    let root = root
        .canonicalize()
        .map_err(|source| ReconciliationError::Io {
            path: root.to_path_buf(),
            source,
        })?;
    let sidecar_path =
        candidate
            .sidecar_path
            .canonicalize()
            .map_err(|source| ReconciliationError::Io {
                path: candidate.sidecar_path.clone(),
                source,
            })?;
    let asset_path =
        candidate
            .asset_path
            .canonicalize()
            .map_err(|source| ReconciliationError::Io {
                path: candidate.asset_path.clone(),
                source,
            })?;
    if !sidecar_path.starts_with(&root) || !asset_path.starts_with(&root) {
        return Err(ReconciliationError::OutsideRoot);
    }
    let destination = sidecar_path_for(&asset_path);
    match fs::symlink_metadata(&destination) {
        Ok(_) => return Err(ReconciliationError::DestinationExists(destination)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(source) => {
            return Err(ReconciliationError::Io {
                path: destination,
                source,
            });
        }
    }
    let (sidecar, digest) = read_sidecar(&sidecar_path)
        .map_err(|error| ReconciliationError::Stale(error.to_string()))?;
    if sidecar.id != candidate.sidecar_id || digest != candidate.sidecar_digest {
        return Err(ReconciliationError::Stale(
            "Sidecar 内容或稳定 ID 已变化".into(),
        ));
    }
    let Some(fingerprint) = sidecar.fingerprint else {
        return Err(ReconciliationError::Stale("Sidecar 指纹已移除".into()));
    };
    if !fingerprint_matches(&asset_path, &fingerprint)
        || fingerprint.value != candidate.sha256
        || fingerprint.size != candidate.size
    {
        return Err(ReconciliationError::Stale("候选素材内容已变化".into()));
    }
    fs::hard_link(&sidecar_path, &destination).map_err(|source| ReconciliationError::Io {
        path: destination.clone(),
        source,
    })?;
    fs::remove_file(&sidecar_path).map_err(|source| ReconciliationError::Io {
        path: sidecar_path.clone(),
        source,
    })?;
    Ok(RelinkReceipt {
        candidate_id: candidate.candidate_id,
        sidecar_id: candidate.sidecar_id,
        from: sidecar_path,
        to: destination,
    })
}

fn confirmed_candidates(
    root_id: Uuid,
    sidecar_id: Uuid,
    sidecar_path: &Path,
    sidecar_digest: &str,
    fingerprint: &Fingerprint,
    candidates_by_size: &BTreeMap<u64, Vec<&AssetRecord>>,
) -> Vec<RelinkCandidate> {
    candidates_by_size
        .get(&fingerprint.size)
        .into_iter()
        .flatten()
        .filter(|asset| fingerprint_matches(&asset.path, fingerprint))
        .map(|asset| RelinkCandidate {
            candidate_id: Uuid::now_v7(),
            root_id,
            sidecar_id,
            sidecar_path: sidecar_path.to_path_buf(),
            sidecar_digest: sidecar_digest.to_owned(),
            asset_key: asset.key.clone(),
            asset_path: asset.path.clone(),
            size: fingerprint.size,
            quick_fingerprint: fingerprint.quick_value.clone(),
            sha256: fingerprint.value.clone(),
            ambiguous: false,
        })
        .collect()
}

fn fingerprint_matches(path: &Path, fingerprint: &Fingerprint) -> bool {
    if fs::metadata(path).map(|value| value.len()).ok() != Some(fingerprint.size) {
        return false;
    }
    if let Some(expected) = &fingerprint.quick_value {
        if quick_fingerprint_file(path).as_ref().ok() != Some(expected) {
            return false;
        }
    }
    digest_file(path).as_ref().ok() == Some(&fingerprint.value)
}

fn unlinked_assets_by_size(assets: &[AssetRecord]) -> BTreeMap<u64, Vec<&AssetRecord>> {
    let mut result = BTreeMap::<u64, Vec<&AssetRecord>>::new();
    for asset in assets
        .iter()
        .filter(|asset| asset.sidecar_state.is_none() && asset.path.is_file())
    {
        if let Some(size) = asset.size {
            result.entry(size).or_default().push(asset);
        }
    }
    result
}

fn asset_path_for_sidecar(sidecar_path: &Path) -> PathBuf {
    let name = sidecar_path
        .file_name()
        .map_or_else(String::new, |name| name.to_string_lossy().into_owned());
    sidecar_path.with_file_name(name.trim_end_matches(".asset.yml"))
}

fn is_asset_sidecar(path: &Path) -> bool {
    path.file_name()
        .is_some_and(|name| name.to_string_lossy().ends_with(".asset.yml"))
}

fn compile_ignore_rules(rules: &[String]) -> Result<GlobSet, FilesystemError> {
    let mut builder = GlobSetBuilder::new();
    for rule in rules {
        let glob = GlobBuilder::new(rule)
            .literal_separator(true)
            .build()
            .map_err(|error| FilesystemError::InvalidIgnoreRule {
                rule: rule.clone(),
                message: error.to_string(),
            })?;
        builder.add(glob);
    }
    builder
        .build()
        .map_err(|error| FilesystemError::InvalidIgnoreRule {
            rule: rules.join(", "),
            message: error.to_string(),
        })
}

fn should_visit_entry(
    path: &Path,
    depth: usize,
    root: &Path,
    options: &ScanOptions,
    ignore: &GlobSet,
) -> bool {
    if depth == 0 {
        return true;
    }
    if options.ignore_hidden
        && path
            .file_name()
            .is_some_and(|name| name.to_string_lossy().starts_with('.'))
    {
        return false;
    }
    !ignore.is_match(path.strip_prefix(root).unwrap_or(path))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::fs;

    use asset_core::AssetRecord;
    use metadata::{MetadataPatch, edit_asset_metadata, sidecar_path_for};
    use tempfile::tempdir;
    use uuid::Uuid;

    use super::{apply_relink, inspect_reconciliation};
    use crate::{ScanOptions, scan_root};

    const PNG: &[u8] = &[
        0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x04, 0x00, 0x00, 0x00, 0xb5,
        0x1c, 0x0c, 0x02, 0x00, 0x00, 0x00, 0x0b, 0x49, 0x44, 0x41, 0x54, 0x78, 0xda, 0x63, 0x64,
        0xf8, 0x0f, 0x00, 0x01, 0x05, 0x01, 0x01, 0x27, 0x18, 0xe3, 0x66, 0x00, 0x00, 0x00, 0x00,
        0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
    ];

    #[test]
    fn moved_asset_is_a_confirmed_candidate_but_no_file_moves_during_inspection() {
        let directory = tempdir().expect("tempdir");
        let original = directory.path().join("original.png");
        fs::write(&original, PNG).expect("write png");
        edit_asset_metadata(
            &original,
            None,
            &MetadataPatch {
                add_tags: BTreeSet::from(["brand/logo".into()]),
                ..MetadataPatch::default()
            },
        )
        .expect("create sidecar");
        let orphan = sidecar_path_for(&original);
        let moved = directory.path().join("moved.png");
        fs::rename(&original, &moved).expect("move only asset");

        let assets = scan_root(directory.path(), &ScanOptions::default())
            .expect("scan")
            .assets;
        let report = inspect_reconciliation(
            Uuid::now_v7(),
            directory.path(),
            &ScanOptions::default(),
            &assets,
        )
        .expect("inspect");

        assert_eq!(report.orphan_sidecars.len(), 1);
        assert_eq!(report.missing_assets.len(), 1);
        assert_eq!(report.pending_moves.len(), 1);
        assert!(orphan.is_file());
        assert!(!sidecar_path_for(&moved).exists());
    }

    #[test]
    fn identical_assets_stay_ambiguous_until_the_user_selects_one() {
        let directory = tempdir().expect("tempdir");
        let original = directory.path().join("original.png");
        fs::write(&original, PNG).expect("write png");
        edit_asset_metadata(
            &original,
            None,
            &MetadataPatch {
                favorite: Some(true),
                ..MetadataPatch::default()
            },
        )
        .expect("create sidecar");
        let first = directory.path().join("first.png");
        let second = directory.path().join("second.png");
        fs::rename(&original, &first).expect("move asset");
        fs::copy(&first, &second).expect("duplicate asset");
        let assets = scan_root(directory.path(), &ScanOptions::default())
            .expect("scan")
            .assets;
        let report = inspect_reconciliation(
            Uuid::now_v7(),
            directory.path(),
            &ScanOptions::default(),
            &assets,
        )
        .expect("inspect");

        assert_eq!(report.pending_moves.len(), 2);
        assert!(report.pending_moves.iter().all(|item| item.ambiguous));
    }

    #[test]
    fn moved_sidecar_finds_the_asset_that_remained_in_place() {
        let directory = tempdir().expect("tempdir");
        let asset = directory.path().join("original.png");
        fs::write(&asset, PNG).expect("write png");
        edit_asset_metadata(
            &asset,
            None,
            &MetadataPatch {
                rating: Some(4),
                ..MetadataPatch::default()
            },
        )
        .expect("create sidecar");
        let misplaced_directory = directory.path().join("misplaced");
        fs::create_dir(&misplaced_directory).expect("misplaced directory");
        let misplaced = misplaced_directory.join("original.png.asset.yml");
        fs::rename(sidecar_path_for(&asset), &misplaced).expect("move only sidecar");

        let assets = scan_root(directory.path(), &ScanOptions::default())
            .expect("scan")
            .assets;
        let report = inspect_reconciliation(
            Uuid::now_v7(),
            directory.path(),
            &ScanOptions::default(),
            &assets,
        )
        .expect("inspect");

        assert_eq!(
            report.orphan_sidecars[0].sidecar_path,
            misplaced.canonicalize().expect("misplaced path")
        );
        assert_eq!(report.pending_moves.len(), 1);
        assert_eq!(
            report.pending_moves[0].asset_path,
            asset.canonicalize().expect("asset path")
        );
        assert!(misplaced.is_file());
    }

    #[test]
    fn sidecar_moved_next_to_a_different_same_named_asset_is_not_merged() {
        let directory = tempdir().expect("tempdir");
        let source_directory = directory.path().join("source");
        let target_directory = directory.path().join("target");
        fs::create_dir(&source_directory).expect("source directory");
        fs::create_dir(&target_directory).expect("target directory");
        let source = source_directory.join("logo.png");
        let target = target_directory.join("logo.png");
        fs::write(&source, PNG).expect("source png");
        let mut different_png = PNG.to_vec();
        different_png.extend_from_slice(b"different");
        fs::write(&target, different_png).expect("target png");
        edit_asset_metadata(
            &source,
            None,
            &MetadataPatch {
                add_tags: BTreeSet::from(["identity/source".into()]),
                ..MetadataPatch::default()
            },
        )
        .expect("create sidecar");
        let misplaced = sidecar_path_for(&target);
        fs::rename(sidecar_path_for(&source), &misplaced).expect("misplace sidecar");

        let assets = scan_root(directory.path(), &ScanOptions::default())
            .expect("scan")
            .assets;
        let target_record = assets
            .iter()
            .find(|asset| asset.path == target.canonicalize().expect("target path"))
            .expect("target record");
        assert!(target_record.id.is_none());
        assert!(target_record.tags.is_empty());
        assert!(
            target_record
                .issues
                .iter()
                .any(|issue| matches!(issue, asset_core::AssetIssue::MismatchedSidecar(_)))
        );

        let report = inspect_reconciliation(
            Uuid::now_v7(),
            directory.path(),
            &ScanOptions::default(),
            &assets,
        )
        .expect("inspect");
        assert_eq!(report.orphan_sidecars.len(), 1);
        assert_eq!(report.pending_moves.len(), 1);
        assert_eq!(
            report.pending_moves[0].asset_path,
            source.canonicalize().expect("source path")
        );
    }

    #[test]
    fn confirmed_relink_revalidates_and_moves_only_the_sidecar_without_overwrite() {
        let directory = tempdir().expect("tempdir");
        let original = directory.path().join("original.png");
        fs::write(&original, PNG).expect("write png");
        edit_asset_metadata(
            &original,
            None,
            &MetadataPatch {
                note: Some("identity".into()),
                ..MetadataPatch::default()
            },
        )
        .expect("create sidecar");
        let orphan = sidecar_path_for(&original);
        let moved = directory.path().join("moved.png");
        fs::rename(&original, &moved).expect("move asset");
        let assets: Vec<AssetRecord> = scan_root(directory.path(), &ScanOptions::default())
            .expect("scan")
            .assets;
        let root_id = Uuid::now_v7();
        let report =
            inspect_reconciliation(root_id, directory.path(), &ScanOptions::default(), &assets)
                .expect("inspect");
        let canonical_orphan = orphan.canonicalize().expect("orphan path");
        let canonical_moved = moved.canonicalize().expect("moved path");
        let receipt = apply_relink(directory.path(), &report.pending_moves[0]).expect("confirm");

        assert_eq!(receipt.from, canonical_orphan);
        assert_eq!(receipt.to, sidecar_path_for(&canonical_moved));
        assert!(!receipt.from.exists());
        assert!(receipt.to.is_file());
        assert_eq!(fs::read(&moved).expect("asset"), PNG);
    }

    #[test]
    fn relink_refuses_a_destination_created_after_the_candidate_was_displayed() {
        let directory = tempdir().expect("tempdir");
        let original = directory.path().join("original.png");
        fs::write(&original, PNG).expect("write png");
        edit_asset_metadata(
            &original,
            None,
            &MetadataPatch {
                favorite: Some(true),
                ..MetadataPatch::default()
            },
        )
        .expect("create sidecar");
        let orphan = sidecar_path_for(&original);
        let moved = directory.path().join("moved.png");
        fs::rename(&original, &moved).expect("move asset");
        let assets = scan_root(directory.path(), &ScanOptions::default())
            .expect("scan")
            .assets;
        let report = inspect_reconciliation(
            Uuid::now_v7(),
            directory.path(),
            &ScanOptions::default(),
            &assets,
        )
        .expect("inspect");
        let destination = sidecar_path_for(&moved);
        fs::write(&destination, b"external sidecar").expect("external destination");

        let error = apply_relink(directory.path(), &report.pending_moves[0])
            .expect_err("destination must not be overwritten");

        assert!(matches!(
            error,
            super::ReconciliationError::DestinationExists(_)
        ));
        assert!(orphan.is_file());
        assert_eq!(
            fs::read(&destination).expect("destination"),
            b"external sidecar"
        );
    }
}

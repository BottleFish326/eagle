use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::{
    AssetSidecar, ExpectedVersion, SidecarError, digest_file, fingerprint_asset, read_sidecar,
    serialize_sidecar, sidecar_path_for, write_sidecar_atomic,
};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MetadataPatch {
    pub set_tags: Option<BTreeSet<String>>,
    #[serde(default)]
    pub add_tags: BTreeSet<String>,
    #[serde(default)]
    pub remove_tags: BTreeSet<String>,
    pub rating: Option<u8>,
    pub favorite: Option<bool>,
    pub note: Option<String>,
    pub aliases: Option<BTreeSet<String>>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MetadataEdit {
    pub sidecar_path: PathBuf,
    pub sidecar: AssetSidecar,
    pub digest: String,
    pub created: bool,
    pub changed: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PreparedMetadataEdit {
    pub sidecar_path: PathBuf,
    pub sidecar: AssetSidecar,
    pub expected: ExpectedVersion,
    pub planned_content: String,
    pub created: bool,
    pub changed: bool,
}

/// Applies user metadata to an adjacent sidecar with optimistic concurrency control.
///
/// `expected_digest` must be `None` when the caller observed no sidecar, or the exact
/// digest returned by the last scan/read. Unknown sidecar fields are preserved.
///
/// # Errors
///
/// Returns [`SidecarError`] for invalid input, stale versions, malformed existing
/// sidecars, or any atomic persistence failure.
pub fn edit_asset_metadata(
    asset_path: &Path,
    expected_digest: Option<&str>,
    patch: &MetadataPatch,
) -> Result<MetadataEdit, SidecarError> {
    commit_prepared_metadata_edit(prepare_asset_metadata_edit(
        asset_path,
        expected_digest,
        patch,
    )?)
}

/// Prepares deterministic Sidecar content without writing it to disk.
///
/// # Errors
///
/// Returns [`SidecarError`] for invalid inputs, stale versions, or unreadable assets.
pub fn prepare_asset_metadata_edit(
    asset_path: &Path,
    expected_digest: Option<&str>,
    patch: &MetadataPatch,
) -> Result<PreparedMetadataEdit, SidecarError> {
    validate_patch(patch)?;
    if !asset_path.is_file() {
        return Err(SidecarError::InvalidAsset(asset_path.to_path_buf()));
    }
    let sidecar_path = sidecar_path_for(asset_path);
    let (mut sidecar, expected, created) = if let Some(expected_digest) = expected_digest {
        if !sidecar_path.is_file() {
            return Err(SidecarError::Conflict { path: sidecar_path });
        }
        let actual_digest = digest_file(&sidecar_path)?;
        if actual_digest != expected_digest {
            return Err(SidecarError::Conflict { path: sidecar_path });
        }
        let (sidecar, _) = read_sidecar(&sidecar_path)?;
        (
            sidecar,
            ExpectedVersion::Digest(expected_digest.to_owned()),
            false,
        )
    } else {
        (AssetSidecar::new(), ExpectedVersion::Missing, true)
    };

    let before = sidecar.clone();
    apply_patch(&mut sidecar, patch);
    sidecar.fingerprint = Some(fingerprint_asset(asset_path)?);
    let changed = created || sidecar != before;
    if !changed {
        return Ok(PreparedMetadataEdit {
            sidecar_path,
            sidecar,
            expected,
            planned_content: String::new(),
            created,
            changed,
        });
    }

    sidecar.touch();
    let planned_content = serialize_sidecar(&sidecar)?;
    Ok(PreparedMetadataEdit {
        sidecar_path,
        sidecar,
        expected,
        planned_content,
        created,
        changed,
    })
}

/// Commits a previously prepared edit using the prepared optimistic version.
///
/// # Errors
///
/// Returns [`SidecarError`] when the Sidecar changed or cannot be atomically written.
pub fn commit_prepared_metadata_edit(
    prepared: PreparedMetadataEdit,
) -> Result<MetadataEdit, SidecarError> {
    if !prepared.changed {
        let ExpectedVersion::Digest(digest) = prepared.expected else {
            return Err(SidecarError::Conflict {
                path: prepared.sidecar_path,
            });
        };
        return Ok(MetadataEdit {
            sidecar_path: prepared.sidecar_path,
            sidecar: prepared.sidecar,
            digest,
            created: prepared.created,
            changed: false,
        });
    }
    let receipt = write_sidecar_atomic(
        &prepared.sidecar_path,
        &prepared.sidecar,
        &prepared.expected,
    )?;
    Ok(MetadataEdit {
        sidecar_path: prepared.sidecar_path,
        sidecar: prepared.sidecar,
        digest: receipt.digest,
        created: prepared.created,
        changed: true,
    })
}

fn validate_patch(patch: &MetadataPatch) -> Result<(), SidecarError> {
    if patch == &MetadataPatch::default() {
        return Err(SidecarError::EmptyEdit);
    }
    if patch.set_tags.is_some() && (!patch.add_tags.is_empty() || !patch.remove_tags.is_empty()) {
        return Err(SidecarError::AmbiguousTagEdit);
    }
    if let Some(tag) = patch.add_tags.intersection(&patch.remove_tags).next() {
        return Err(SidecarError::ConflictingTagEdit(tag.clone()));
    }
    if let Some(rating) = patch.rating.filter(|rating| *rating > 5) {
        return Err(SidecarError::InvalidRating(rating));
    }
    validate_values(
        patch
            .set_tags
            .iter()
            .flat_map(|tags| tags.iter())
            .chain(patch.add_tags.iter())
            .chain(patch.remove_tags.iter()),
        128,
        SidecarError::EmptyTag,
        SidecarError::TagTooLong,
    )?;
    if let Some(aliases) = &patch.aliases {
        validate_values(
            aliases.iter(),
            256,
            SidecarError::EmptyAlias,
            SidecarError::AliasTooLong,
        )?;
    }
    if patch
        .note
        .as_ref()
        .is_some_and(|note| note.chars().count() > 10_000)
    {
        return Err(SidecarError::NoteTooLong);
    }
    Ok(())
}

fn validate_values<'a>(
    values: impl Iterator<Item = &'a String>,
    maximum: usize,
    empty_error: SidecarError,
    long_error: SidecarError,
) -> Result<(), SidecarError> {
    for value in values {
        if value.trim().is_empty() {
            return Err(empty_error);
        }
        if value.chars().count() > maximum {
            return Err(long_error);
        }
    }
    Ok(())
}

fn apply_patch(sidecar: &mut AssetSidecar, patch: &MetadataPatch) {
    if let Some(tags) = &patch.set_tags {
        sidecar.tags.clone_from(tags);
    } else {
        sidecar.tags.extend(patch.add_tags.iter().cloned());
        sidecar.tags.retain(|tag| !patch.remove_tags.contains(tag));
    }
    if let Some(rating) = patch.rating {
        sidecar.rating = rating;
    }
    if let Some(favorite) = patch.favorite {
        sidecar.favorite = favorite;
    }
    if let Some(note) = &patch.note {
        sidecar.note.clone_from(note);
    }
    if let Some(aliases) = &patch.aliases {
        sidecar.aliases.clone_from(aliases);
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};
    use std::fs;

    use serde_yaml_ng::Value;
    use tempfile::tempdir;

    use super::{MetadataPatch, edit_asset_metadata};
    use crate::{SidecarError, digest_file, read_sidecar};

    #[test]
    fn first_edit_creates_a_stable_sidecar_without_changing_the_asset() {
        let directory = tempdir().expect("tempdir");
        let asset = directory.path().join("logo.png");
        fs::write(&asset, b"original asset bytes").expect("write asset");
        let asset_digest = digest_file(&asset).expect("asset digest");
        let edit = edit_asset_metadata(
            &asset,
            None,
            &MetadataPatch {
                add_tags: BTreeSet::from(["ui/icon".into()]),
                ..MetadataPatch::default()
            },
        )
        .expect("first edit");

        assert!(edit.created);
        assert!(edit.changed);
        assert_eq!(edit.sidecar.id.get_version_num(), 7);
        assert!(edit.sidecar.tags.contains("ui/icon"));
        let fingerprint = edit.sidecar.fingerprint.expect("asset fingerprint");
        assert_eq!(fingerprint.algorithm, "sha256");
        assert_eq!(fingerprint.value, asset_digest);
        assert_eq!(fingerprint.size, 20);
        assert_eq!(
            fingerprint.quick_algorithm.as_deref(),
            Some("sha256-sample-64k-v1")
        );
        assert_eq!(fingerprint.quick_value.as_deref().map(str::len), Some(64));
        assert_eq!(digest_file(&asset).expect("asset digest"), asset_digest);
    }

    #[test]
    fn updates_all_user_fields_and_preserves_unknown_fields() {
        let directory = tempdir().expect("tempdir");
        let asset = directory.path().join("logo.png");
        fs::write(&asset, b"asset").expect("write asset");
        let first = edit_asset_metadata(
            &asset,
            None,
            &MetadataPatch {
                add_tags: BTreeSet::from(["old".into()]),
                ..MetadataPatch::default()
            },
        )
        .expect("create sidecar");
        let mut sidecar = first.sidecar;
        sidecar.extra = BTreeMap::from([("future".into(), Value::String("kept".into()))]);
        fs::write(
            &first.sidecar_path,
            serde_yaml_ng::to_string(&sidecar).expect("serialize external sidecar"),
        )
        .expect("write external field");
        let (_, digest) = read_sidecar(&first.sidecar_path).expect("read external sidecar");

        let edit = edit_asset_metadata(
            &asset,
            Some(&digest),
            &MetadataPatch {
                add_tags: BTreeSet::from(["new".into()]),
                remove_tags: BTreeSet::from(["old".into()]),
                rating: Some(4),
                favorite: Some(true),
                note: Some("candidate".into()),
                aliases: Some(BTreeSet::from(["main-logo".into()])),
                ..MetadataPatch::default()
            },
        )
        .expect("update sidecar");

        assert_eq!(edit.sidecar.id, sidecar.id);
        assert_eq!(edit.sidecar.tags, BTreeSet::from(["new".into()]));
        assert_eq!(edit.sidecar.rating, 4);
        assert!(edit.sidecar.favorite);
        assert_eq!(edit.sidecar.note, "candidate");
        assert_eq!(edit.sidecar.aliases, BTreeSet::from(["main-logo".into()]));
        assert_eq!(edit.sidecar.extra["future"], Value::String("kept".into()));
    }

    #[test]
    fn rejects_stale_or_ambiguous_edits_without_overwriting_the_sidecar() {
        let directory = tempdir().expect("tempdir");
        let asset = directory.path().join("logo.png");
        fs::write(&asset, b"asset").expect("write asset");
        let first = edit_asset_metadata(
            &asset,
            None,
            &MetadataPatch {
                favorite: Some(true),
                ..MetadataPatch::default()
            },
        )
        .expect("create sidecar");
        fs::write(&first.sidecar_path, "external: change\n").expect("external edit");
        let external = fs::read(&first.sidecar_path).expect("read external sidecar");

        let stale = edit_asset_metadata(
            &asset,
            Some(&first.digest),
            &MetadataPatch {
                rating: Some(3),
                ..MetadataPatch::default()
            },
        )
        .expect_err("reject stale digest");
        assert!(matches!(
            stale,
            SidecarError::Parse { .. } | SidecarError::Conflict { .. }
        ));
        assert_eq!(
            fs::read(&first.sidecar_path).expect("read sidecar"),
            external
        );

        let ambiguous = edit_asset_metadata(
            &asset,
            None,
            &MetadataPatch {
                set_tags: Some(BTreeSet::new()),
                add_tags: BTreeSet::from(["new".into()]),
                ..MetadataPatch::default()
            },
        )
        .expect_err("reject ambiguous patch");
        assert!(matches!(ambiguous, SidecarError::AmbiguousTagEdit));
        assert_eq!(
            fs::read(&first.sidecar_path).expect("read sidecar"),
            external
        );
    }
}

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{AssetSidecar, MetadataPatch};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserMetadata {
    pub tags: BTreeSet<String>,
    pub rating: u8,
    pub favorite: bool,
    pub note: String,
    pub aliases: BTreeSet<String>,
}

impl From<&AssetSidecar> for UserMetadata {
    fn from(sidecar: &AssetSidecar) -> Self {
        Self {
            tags: sidecar.tags.clone(),
            rating: sidecar.rating,
            favorite: sidecar.favorite,
            note: sidecar.note.clone(),
            aliases: sidecar.aliases.clone(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ConflictField {
    Tags,
    Rating,
    Favorite,
    Note,
    Aliases,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConflictAnalysis {
    pub base: UserMetadata,
    pub current: UserMetadata,
    pub proposed: UserMetadata,
    pub externally_changed_fields: BTreeSet<ConflictField>,
    pub conflicting_fields: BTreeSet<ConflictField>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TagConflictResolution {
    Merge,
    KeepExternal,
    UseMine,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FieldConflictResolution {
    KeepExternal,
    UseMine,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MetadataConflictResolution {
    pub tags: Option<TagConflictResolution>,
    #[serde(default)]
    pub fields: BTreeMap<ConflictField, FieldConflictResolution>,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ConflictResolutionError {
    #[error("missing explicit resolution for {0:?}")]
    MissingResolution(ConflictField),
}

#[must_use]
pub fn analyze_metadata_conflict(
    base: UserMetadata,
    current: UserMetadata,
    patch: &MetadataPatch,
) -> ConflictAnalysis {
    let proposed = apply_patch_to_metadata(&base, patch);
    let externally_changed_fields = changed_fields(&base, &current);
    let mut conflicting_fields = BTreeSet::new();
    if patch_touches_tags(patch) && base.tags != current.tags && proposed.tags != current.tags {
        conflicting_fields.insert(ConflictField::Tags);
    }
    insert_scalar_conflict(
        &mut conflicting_fields,
        ConflictField::Rating,
        patch.rating.is_some(),
        base.rating != current.rating,
        proposed.rating != current.rating,
    );
    insert_scalar_conflict(
        &mut conflicting_fields,
        ConflictField::Favorite,
        patch.favorite.is_some(),
        base.favorite != current.favorite,
        proposed.favorite != current.favorite,
    );
    insert_scalar_conflict(
        &mut conflicting_fields,
        ConflictField::Note,
        patch.note.is_some(),
        base.note != current.note,
        proposed.note != current.note,
    );
    insert_scalar_conflict(
        &mut conflicting_fields,
        ConflictField::Aliases,
        patch.aliases.is_some(),
        base.aliases != current.aliases,
        proposed.aliases != current.aliases,
    );
    ConflictAnalysis {
        base,
        current,
        proposed,
        externally_changed_fields,
        conflicting_fields,
    }
}

/// Resolves a conflict onto the current external version without touching unknown fields.
///
/// # Errors
///
/// Returns [`ConflictResolutionError`] until every overlapping field has an explicit choice.
pub fn resolve_metadata_conflict(
    current_sidecar: &AssetSidecar,
    patch: &MetadataPatch,
    analysis: &ConflictAnalysis,
    resolution: &MetadataConflictResolution,
) -> Result<AssetSidecar, ConflictResolutionError> {
    let mut resolved = current_sidecar.clone();
    if patch_touches_tags(patch) {
        if analysis.conflicting_fields.contains(&ConflictField::Tags) {
            resolved.tags =
                match resolution
                    .tags
                    .ok_or(ConflictResolutionError::MissingResolution(
                        ConflictField::Tags,
                    ))? {
                    TagConflictResolution::Merge => {
                        merge_tags(&analysis.base.tags, &analysis.current.tags, patch)
                    }
                    TagConflictResolution::KeepExternal => analysis.current.tags.clone(),
                    TagConflictResolution::UseMine => analysis.proposed.tags.clone(),
                };
        } else {
            resolved.tags = apply_tag_patch(&resolved.tags, patch);
        }
    }
    resolve_scalar(
        &mut resolved.rating,
        patch.rating,
        ConflictField::Rating,
        analysis,
        resolution,
    )?;
    resolve_scalar(
        &mut resolved.favorite,
        patch.favorite,
        ConflictField::Favorite,
        analysis,
        resolution,
    )?;
    resolve_scalar(
        &mut resolved.note,
        patch.note.clone(),
        ConflictField::Note,
        analysis,
        resolution,
    )?;
    resolve_scalar(
        &mut resolved.aliases,
        patch.aliases.clone(),
        ConflictField::Aliases,
        analysis,
        resolution,
    )?;
    Ok(resolved)
}

fn apply_patch_to_metadata(base: &UserMetadata, patch: &MetadataPatch) -> UserMetadata {
    UserMetadata {
        tags: apply_tag_patch(&base.tags, patch),
        rating: patch.rating.unwrap_or(base.rating),
        favorite: patch.favorite.unwrap_or(base.favorite),
        note: patch.note.clone().unwrap_or_else(|| base.note.clone()),
        aliases: patch
            .aliases
            .clone()
            .unwrap_or_else(|| base.aliases.clone()),
    }
}

fn apply_tag_patch(tags: &BTreeSet<String>, patch: &MetadataPatch) -> BTreeSet<String> {
    let mut result = patch.set_tags.clone().unwrap_or_else(|| tags.clone());
    result.extend(patch.add_tags.iter().cloned());
    for tag in &patch.remove_tags {
        result.remove(tag);
    }
    result
}

fn merge_tags(
    base: &BTreeSet<String>,
    current: &BTreeSet<String>,
    patch: &MetadataPatch,
) -> BTreeSet<String> {
    if patch.set_tags.is_none() {
        return apply_tag_patch(current, patch);
    }
    let proposed = apply_tag_patch(base, patch);
    let mut result = current.clone();
    for removed in base.difference(&proposed) {
        result.remove(removed);
    }
    result.extend(proposed.difference(base).cloned());
    result
}

fn changed_fields(base: &UserMetadata, current: &UserMetadata) -> BTreeSet<ConflictField> {
    let mut fields = BTreeSet::new();
    if base.tags != current.tags {
        fields.insert(ConflictField::Tags);
    }
    if base.rating != current.rating {
        fields.insert(ConflictField::Rating);
    }
    if base.favorite != current.favorite {
        fields.insert(ConflictField::Favorite);
    }
    if base.note != current.note {
        fields.insert(ConflictField::Note);
    }
    if base.aliases != current.aliases {
        fields.insert(ConflictField::Aliases);
    }
    fields
}

fn patch_touches_tags(patch: &MetadataPatch) -> bool {
    patch.set_tags.is_some() || !patch.add_tags.is_empty() || !patch.remove_tags.is_empty()
}

fn insert_scalar_conflict(
    fields: &mut BTreeSet<ConflictField>,
    field: ConflictField,
    touched: bool,
    externally_changed: bool,
    proposed_differs: bool,
) {
    if touched && externally_changed && proposed_differs {
        fields.insert(field);
    }
}

fn resolve_scalar<T: Clone>(
    current: &mut T,
    proposed: Option<T>,
    field: ConflictField,
    analysis: &ConflictAnalysis,
    resolution: &MetadataConflictResolution,
) -> Result<(), ConflictResolutionError> {
    let Some(proposed) = proposed else {
        return Ok(());
    };
    if analysis.conflicting_fields.contains(&field) {
        match resolution
            .fields
            .get(&field)
            .ok_or(ConflictResolutionError::MissingResolution(field))?
        {
            FieldConflictResolution::KeepExternal => {}
            FieldConflictResolution::UseMine => *current = proposed,
        }
    } else {
        *current = proposed;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        ConflictField, FieldConflictResolution, MetadataConflictResolution, TagConflictResolution,
        UserMetadata, analyze_metadata_conflict, resolve_metadata_conflict,
    };
    use crate::{AssetSidecar, MetadataPatch};
    use std::collections::{BTreeMap, BTreeSet};

    #[test]
    fn explicitly_merges_tag_deltas_and_requires_a_note_choice() {
        let mut base_sidecar = AssetSidecar::new();
        base_sidecar.tags = BTreeSet::from(["base".into(), "remove-me".into()]);
        base_sidecar.note = "base note".into();
        let mut current_sidecar = base_sidecar.clone();
        current_sidecar.tags.insert("external".into());
        current_sidecar.note = "external note".into();
        let patch = MetadataPatch {
            add_tags: BTreeSet::from(["mine".into()]),
            remove_tags: BTreeSet::from(["remove-me".into()]),
            note: Some("my note".into()),
            ..MetadataPatch::default()
        };
        let analysis = analyze_metadata_conflict(
            UserMetadata::from(&base_sidecar),
            UserMetadata::from(&current_sidecar),
            &patch,
        );
        assert_eq!(
            analysis.conflicting_fields,
            BTreeSet::from([ConflictField::Tags, ConflictField::Note])
        );
        let missing = resolve_metadata_conflict(
            &current_sidecar,
            &patch,
            &analysis,
            &MetadataConflictResolution {
                tags: Some(TagConflictResolution::Merge),
                ..MetadataConflictResolution::default()
            },
        );
        assert!(missing.is_err());

        let resolved = resolve_metadata_conflict(
            &current_sidecar,
            &patch,
            &analysis,
            &MetadataConflictResolution {
                tags: Some(TagConflictResolution::Merge),
                fields: BTreeMap::from([(
                    ConflictField::Note,
                    FieldConflictResolution::KeepExternal,
                )]),
            },
        )
        .expect("resolve");
        assert_eq!(
            resolved.tags,
            BTreeSet::from(["base".into(), "external".into(), "mine".into()])
        );
        assert_eq!(resolved.note, "external note");
    }
}

use std::collections::BTreeSet;

use asset_core::{AssetIssue, AssetRecord, SidecarState};
use asset_index::{AssetIndex, AssetQuery, QueryParseError, parse_query};
use metadata::{MetadataPatch, SidecarError, edit_asset_metadata};
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetEditTarget {
    pub key: String,
    pub expected_sidecar_digest: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchMetadataEdit {
    pub targets: Vec<AssetEditTarget>,
    pub patch: MetadataPatch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum EditFailureKind {
    NotFound,
    Conflict,
    InvalidInput,
    WriteFailed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MetadataEditFailure {
    pub key: String,
    pub kind: EditFailureKind,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchMetadataEditResult {
    pub updated: Vec<AssetRecord>,
    pub failures: Vec<MetadataEditFailure>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueryAssetsInput {
    pub expression: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QueryAssetsResult {
    pub expression: String,
    pub query: AssetQuery,
    pub keys: Vec<String>,
    pub total_assets: usize,
}

#[derive(Debug, Error)]
pub enum CatalogError {
    #[error("metadata edit requires at least one target")]
    EmptyTargets,
}

#[derive(Debug, Default)]
pub struct AssetCatalog {
    index: AssetIndex,
}

impl AssetCatalog {
    pub fn ingest(&mut self, records: impl IntoIterator<Item = AssetRecord>) {
        for record in records {
            self.index.upsert(record);
        }
    }

    #[must_use]
    pub fn get(&self, key: &str) -> Option<&AssetRecord> {
        self.index.get(key)
    }

    #[must_use]
    pub fn query(&self, query: &AssetQuery) -> BTreeSet<String> {
        self.index.query(query)
    }

    /// Parses and executes a user-facing filter expression against the in-memory index.
    ///
    /// # Errors
    ///
    /// Returns a structured [`QueryParseError`] instead of treating malformed syntax
    /// as an empty result.
    pub fn query_assets(
        &self,
        input: &QueryAssetsInput,
    ) -> Result<QueryAssetsResult, QueryParseError> {
        let query = parse_query(&input.expression)?;
        let keys = self.index.query(&query).into_iter().collect();
        Ok(QueryAssetsResult {
            expression: input.expression.clone(),
            query,
            keys,
            total_assets: self.index.len(),
        })
    }

    /// Applies the same metadata patch to one or more assets and updates index postings
    /// immediately after each successful atomic sidecar write.
    ///
    /// Failures are isolated per target; successful earlier targets are not rolled back.
    ///
    /// # Errors
    ///
    /// Returns [`CatalogError`] only when no edit target is supplied.
    pub fn edit_metadata(
        &mut self,
        request: &BatchMetadataEdit,
    ) -> Result<BatchMetadataEditResult, CatalogError> {
        if request.targets.is_empty() {
            return Err(CatalogError::EmptyTargets);
        }
        let mut updated = Vec::new();
        let mut failures = Vec::new();
        for target in &request.targets {
            match self.edit_one(target, &request.patch) {
                Ok(record) => updated.push(record),
                Err(error) => failures.push(MetadataEditFailure {
                    key: target.key.clone(),
                    kind: error.kind(),
                    message: error.to_string(),
                }),
            }
        }
        Ok(BatchMetadataEditResult { updated, failures })
    }

    fn edit_one(
        &mut self,
        target: &AssetEditTarget,
        patch: &MetadataPatch,
    ) -> Result<AssetRecord, EditOneError> {
        let mut record = self
            .index
            .get(&target.key)
            .cloned()
            .ok_or_else(|| EditOneError::NotFound(target.key.clone()))?;
        let catalog_digest = record
            .sidecar_state
            .as_ref()
            .map(|state| state.digest.as_str());
        if catalog_digest != target.expected_sidecar_digest.as_deref() {
            return Err(EditOneError::CatalogConflict(target.key.clone()));
        }
        let edit = edit_asset_metadata(
            &record.path,
            target.expected_sidecar_digest.as_deref(),
            patch,
        )?;
        record.id = Some(edit.sidecar.id);
        record.sidecar_path = Some(edit.sidecar_path);
        record.sidecar_state = Some(SidecarState {
            schema: edit.sidecar.schema,
            digest: edit.digest,
            updated_at: edit.sidecar.updated_at.clone(),
        });
        record.tags = edit.sidecar.tags;
        record.rating = edit.sidecar.rating;
        record.favorite = edit.sidecar.favorite;
        record.note = edit.sidecar.note;
        record.aliases = edit.sidecar.aliases;
        record
            .issues
            .retain(|issue| !matches!(issue, AssetIssue::InvalidSidecar(_)));
        self.index.upsert(record.clone());
        Ok(record)
    }
}

#[derive(Debug, Error)]
enum EditOneError {
    #[error("asset is not present in the in-memory catalog: {0}")]
    NotFound(String),
    #[error("asset metadata version does not match the in-memory catalog: {0}")]
    CatalogConflict(String),
    #[error(transparent)]
    Sidecar(#[from] SidecarError),
}

impl EditOneError {
    const fn kind(&self) -> EditFailureKind {
        match self {
            Self::NotFound(_) => EditFailureKind::NotFound,
            Self::CatalogConflict(_) | Self::Sidecar(SidecarError::Conflict { .. }) => {
                EditFailureKind::Conflict
            }
            Self::Sidecar(
                SidecarError::InvalidRating(_)
                | SidecarError::EmptyTag
                | SidecarError::TagTooLong
                | SidecarError::EmptyAlias
                | SidecarError::AliasTooLong
                | SidecarError::NoteTooLong
                | SidecarError::EmptyEdit
                | SidecarError::AmbiguousTagEdit
                | SidecarError::ConflictingTagEdit(_),
            ) => EditFailureKind::InvalidInput,
            Self::Sidecar(_) => EditFailureKind::WriteFailed,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::fs;
    use std::path::PathBuf;

    use asset_core::{AssetKind, AssetRecord};
    use asset_index::{AssetQuery, QueryParseErrorKind};
    use metadata::{MetadataPatch, digest_file};
    use tempfile::tempdir;

    use super::{
        AssetCatalog, AssetEditTarget, BatchMetadataEdit, EditFailureKind, QueryAssetsInput,
    };

    #[test]
    fn parsed_query_returns_deterministic_keys_and_visible_errors() {
        let mut first = record("first", PathBuf::from("/assets/first.PNG"));
        first.tags = BTreeSet::from(["ui/icon".into(), "color/blue".into()]);
        first.favorite = true;
        let mut second = record("second", PathBuf::from("/assets/second.jpg"));
        second.tags = BTreeSet::from(["ui/photo".into(), "color/red".into()]);
        let mut draft = record("draft", PathBuf::from("/assets/draft.mp4"));
        draft.mime = "video/mp4".into();
        draft.kind = AssetKind::Video;
        draft.tags = BTreeSet::from(["ui/icon".into(), "color/blue".into(), "draft".into()]);
        draft.favorite = true;
        let mut catalog = AssetCatalog::default();
        catalog.ingest([first, second, draft]);

        let result = catalog
            .query_assets(&QueryAssetsInput {
                expression:
                    "ui/* any:(color/blue|color/red) -draft type:image ext:png|jpg favorite:true"
                        .into(),
            })
            .expect("query assets");

        assert_eq!(result.keys, vec!["first"]);
        assert_eq!(result.total_assets, 3);
        assert_eq!(
            result.query.extensions,
            BTreeSet::from(["jpg".into(), "png".into()])
        );

        let all = catalog
            .query_assets(&QueryAssetsInput {
                expression: String::new(),
            })
            .expect("empty query");
        assert_eq!(all.keys, vec!["draft", "first", "second"]);

        let error = catalog
            .query_assets(&QueryAssetsInput {
                expression: "kind:image".into(),
            })
            .expect_err("unknown filters are visible");
        assert_eq!(error.kind, QueryParseErrorKind::UnknownFilter);
        assert_eq!(error.offset, 0);
    }

    #[test]
    fn batch_edit_creates_sidecars_and_updates_tag_index_immediately() {
        let directory = tempdir().expect("tempdir");
        let assets = (0..20)
            .map(|index| {
                let key = format!("asset-{index:02}");
                let path = directory.path().join(format!("{key}.png"));
                fs::write(&path, format!("asset body {index}")).expect("write asset");
                let digest = digest_file(&path).expect("asset digest");
                (key, path, digest)
            })
            .collect::<Vec<_>>();
        let mut catalog = AssetCatalog::default();
        catalog.ingest(
            assets
                .iter()
                .map(|(key, path, _)| record(key, path.clone())),
        );

        let result = catalog
            .edit_metadata(&BatchMetadataEdit {
                targets: assets.iter().map(|(key, _, _)| target(key, None)).collect(),
                patch: MetadataPatch {
                    set_tags: Some(BTreeSet::from(["project/eagle".into(), "ui/icon".into()])),
                    ..MetadataPatch::default()
                },
            })
            .expect("batch edit");

        assert_eq!(result.updated.len(), 20);
        assert!(result.failures.is_empty());
        assert_eq!(
            result
                .updated
                .iter()
                .filter_map(|record| record.id)
                .collect::<BTreeSet<_>>()
                .len(),
            20
        );
        let all_keys = assets
            .iter()
            .map(|(key, _, _)| key.clone())
            .collect::<BTreeSet<_>>();
        assert_eq!(
            catalog.query(&AssetQuery {
                all_tags: BTreeSet::from(["project/eagle".into()]),
                ..AssetQuery::default()
            }),
            all_keys
        );

        let first = &result.updated[0];
        let first_digest = first
            .sidecar_state
            .as_ref()
            .expect("sidecar state")
            .digest
            .clone();
        let removal = catalog
            .edit_metadata(&BatchMetadataEdit {
                targets: vec![target(&first.key, Some(first_digest))],
                patch: MetadataPatch {
                    remove_tags: BTreeSet::from(["project/eagle".into()]),
                    ..MetadataPatch::default()
                },
            })
            .expect("remove tag");
        assert_eq!(removal.updated.len(), 1);
        assert_eq!(
            catalog
                .query(&AssetQuery {
                    all_tags: BTreeSet::from(["project/eagle".into()]),
                    ..AssetQuery::default()
                })
                .len(),
            19
        );
        assert_eq!(
            catalog.query(&AssetQuery {
                all_tags: BTreeSet::from(["ui/icon".into()]),
                ..AssetQuery::default()
            }),
            assets.iter().map(|(key, _, _)| key.clone()).collect()
        );
        for (_, path, digest) in &assets {
            assert_eq!(digest_file(path).expect("asset digest after edit"), *digest);
        }
    }

    #[test]
    fn stale_disk_edit_is_isolated_and_does_not_change_index() {
        let directory = tempdir().expect("tempdir");
        let asset = directory.path().join("asset.png");
        fs::write(&asset, b"asset").expect("write asset");
        let mut catalog = AssetCatalog::default();
        catalog.ingest([record("asset", asset)]);
        let created = catalog
            .edit_metadata(&BatchMetadataEdit {
                targets: vec![target("asset", None)],
                patch: MetadataPatch {
                    add_tags: BTreeSet::from(["old".into()]),
                    ..MetadataPatch::default()
                },
            })
            .expect("create sidecar");
        let record = &created.updated[0];
        let expected = record
            .sidecar_state
            .as_ref()
            .expect("sidecar state")
            .digest
            .clone();
        let sidecar_path = record.sidecar_path.as_ref().expect("sidecar path");
        let mut external = fs::read_to_string(sidecar_path).expect("read sidecar");
        external.push_str("externalField: true\n");
        fs::write(sidecar_path, external).expect("external write");

        let result = catalog
            .edit_metadata(&BatchMetadataEdit {
                targets: vec![target("asset", Some(expected))],
                patch: MetadataPatch {
                    add_tags: BTreeSet::from(["new".into()]),
                    ..MetadataPatch::default()
                },
            })
            .expect("batch result");

        assert!(result.updated.is_empty());
        assert_eq!(result.failures[0].kind, EditFailureKind::Conflict);
        assert_eq!(
            catalog.query(&AssetQuery {
                all_tags: BTreeSet::from(["old".into()]),
                ..AssetQuery::default()
            }),
            BTreeSet::from(["asset".into()])
        );
        assert!(
            catalog
                .query(&AssetQuery {
                    all_tags: BTreeSet::from(["new".into()]),
                    ..AssetQuery::default()
                })
                .is_empty()
        );
    }

    fn record(key: &str, path: PathBuf) -> AssetRecord {
        AssetRecord::untagged(key.into(), path, "image/png".into(), 5, 0)
    }

    fn target(key: &str, expected_sidecar_digest: Option<String>) -> AssetEditTarget {
        AssetEditTarget {
            key: key.into(),
            expected_sidecar_digest,
        }
    }
}

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::Mutex;

use asset_core::AssetRecord;
use metadata::{
    AssetSidecar, ConflictAnalysis, ConflictField, MetadataConflictResolution, MetadataPatch,
    SidecarFileVersion, UserMetadata, analyze_metadata_conflict, read_sidecar_versioned,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

const MAX_PENDING_CONFLICTS: usize = 256;

#[derive(Debug, Clone)]
pub(crate) struct PendingMetadataConflict {
    pub id: Uuid,
    pub key: String,
    pub root_id: Uuid,
    pub asset_path: PathBuf,
    pub sidecar_path: PathBuf,
    pub current_sidecar: AssetSidecar,
    pub current_version: SidecarFileVersion,
    pub patch: MetadataPatch,
    pub analysis: ConflictAnalysis,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MetadataConflictView {
    pub id: Uuid,
    pub key: String,
    pub file_name: String,
    pub source: &'static str,
    pub sidecar_modified_unix_ms: i64,
    pub identity_changed: bool,
    pub base: UserMetadata,
    pub current: UserMetadata,
    pub proposed: UserMetadata,
    pub externally_changed_fields: BTreeSet<ConflictField>,
    pub conflicting_fields: BTreeSet<ConflictField>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ResolveMetadataConflictInput {
    pub conflict_id: Uuid,
    pub resolution: MetadataConflictResolution,
}

#[derive(Debug, Default)]
pub(crate) struct MetadataConflictStore {
    pending: Mutex<BTreeMap<Uuid, PendingMetadataConflict>>,
}

impl MetadataConflictStore {
    pub(crate) fn capture(
        &self,
        record: &AssetRecord,
        patch: &MetadataPatch,
    ) -> Result<MetadataConflictView, String> {
        let root_id = record
            .root_id
            .ok_or_else(|| "conflicting asset has no library root".to_owned())?;
        let sidecar_path = metadata::sidecar_path_for(&record.path);
        let (current_sidecar, current_version) =
            read_sidecar_versioned(&sidecar_path).map_err(|error| error.to_string())?;
        let base = user_metadata_from_record(record);
        let current = UserMetadata::from(&current_sidecar);
        let analysis = analyze_metadata_conflict(base, current, patch);
        let id = Uuid::now_v7();
        let pending = PendingMetadataConflict {
            id,
            key: record.key.clone(),
            root_id,
            asset_path: record.path.clone(),
            sidecar_path,
            current_sidecar,
            current_version,
            patch: patch.clone(),
            analysis,
        };
        let view = pending.view(record.id);
        let mut entries = self
            .pending
            .lock()
            .map_err(|_| "metadata conflict store lock is poisoned".to_owned())?;
        entries.retain(|_, conflict| conflict.key != record.key);
        while entries.len() >= MAX_PENDING_CONFLICTS {
            let Some(oldest) = entries.keys().next().copied() else {
                break;
            };
            entries.remove(&oldest);
        }
        entries.insert(id, pending);
        Ok(view)
    }

    pub(crate) fn get(&self, id: Uuid) -> Result<PendingMetadataConflict, String> {
        self.pending
            .lock()
            .map_err(|_| "metadata conflict store lock is poisoned".to_owned())?
            .get(&id)
            .cloned()
            .ok_or_else(|| format!("metadata conflict was not found: {id}"))
    }

    pub(crate) fn remove(&self, id: Uuid) -> Result<(), String> {
        self.pending
            .lock()
            .map_err(|_| "metadata conflict store lock is poisoned".to_owned())?
            .remove(&id)
            .ok_or_else(|| format!("metadata conflict was not found: {id}"))?;
        Ok(())
    }

    pub(crate) fn invalidate_keys<'a>(
        &self,
        keys: impl IntoIterator<Item = &'a str>,
    ) -> Result<(), String> {
        let keys = keys.into_iter().collect::<BTreeSet<_>>();
        self.pending
            .lock()
            .map_err(|_| "metadata conflict store lock is poisoned".to_owned())?
            .retain(|_, conflict| !keys.contains(conflict.key.as_str()));
        Ok(())
    }
}

impl PendingMetadataConflict {
    fn view(&self, base_id: Option<Uuid>) -> MetadataConflictView {
        MetadataConflictView {
            id: self.id,
            key: self.key.clone(),
            file_name: self
                .asset_path
                .file_name()
                .map_or_else(String::new, |name| name.to_string_lossy().into_owned()),
            source: "external-edit",
            sidecar_modified_unix_ms: self.current_version.modified_unix_ms,
            identity_changed: base_id.is_some_and(|id| id != self.current_sidecar.id),
            base: self.analysis.base.clone(),
            current: self.analysis.current.clone(),
            proposed: self.analysis.proposed.clone(),
            externally_changed_fields: self.analysis.externally_changed_fields.clone(),
            conflicting_fields: self.analysis.conflicting_fields.clone(),
        }
    }
}

fn user_metadata_from_record(record: &AssetRecord) -> UserMetadata {
    UserMetadata {
        tags: record.tags.clone(),
        rating: record.rating,
        favorite: record.favorite,
        note: record.note.clone(),
        aliases: record.aliases.clone(),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::fs;

    use asset_core::AssetRecord;
    use metadata::{
        AssetSidecar, ConflictField, ExpectedVersion, FieldConflictResolution, MetadataPatch,
        SidecarFileVersion, TagConflictResolution, sidecar_path_for, write_sidecar_atomic,
    };
    use serde_json::json;
    use tempfile::tempdir;
    use uuid::Uuid;

    use super::{MetadataConflictStore, ResolveMetadataConflictInput};

    #[test]
    fn resolution_input_uses_the_frontend_wire_shape() {
        let input: ResolveMetadataConflictInput = serde_json::from_value(json!({
            "conflictId": Uuid::now_v7(),
            "resolution": {
                "tags": "merge",
                "fields": { "note": "use-mine" }
            }
        }))
        .expect("deserialize resolution");

        assert_eq!(input.resolution.tags, Some(TagConflictResolution::Merge));
        assert_eq!(
            input.resolution.fields.get(&ConflictField::Note),
            Some(&FieldConflictResolution::UseMine)
        );
    }

    #[test]
    fn captures_an_opaque_conflict_plan_without_modifying_the_sidecar() {
        let directory = tempdir().expect("tempdir");
        let asset_path = directory.path().join("logo.png");
        fs::write(&asset_path, b"asset").expect("asset");
        let sidecar_path = sidecar_path_for(&asset_path);
        let mut base = AssetSidecar::new();
        base.tags.insert("base".into());
        base.note = "base note".into();
        let receipt = write_sidecar_atomic(&sidecar_path, &base, &ExpectedVersion::Missing)
            .expect("base sidecar");

        let root_id = Uuid::now_v7();
        let key = asset_path.to_string_lossy().into_owned();
        let mut record = AssetRecord::untagged(key.clone(), asset_path, "image/png".into(), 5, 0);
        record.root_id = Some(root_id);
        record.id = Some(base.id);
        record.tags = base.tags.clone();
        record.note.clone_from(&base.note);

        let mut current = base;
        current.tags.insert("external".into());
        current.note = "external note".into();
        current.touch();
        write_sidecar_atomic(
            &sidecar_path,
            &current,
            &ExpectedVersion::Snapshot(SidecarFileVersion {
                digest: receipt.digest,
                size: receipt.size,
                modified_unix_ms: receipt.modified_unix_ms,
            }),
        )
        .expect("external edit");
        let before_capture = fs::read(&sidecar_path).expect("read external sidecar");

        let store = MetadataConflictStore::default();
        let view = store
            .capture(
                &record,
                &MetadataPatch {
                    add_tags: BTreeSet::from(["mine".into()]),
                    note: Some("my note".into()),
                    ..MetadataPatch::default()
                },
            )
            .expect("capture");

        assert_eq!(view.key, key);
        assert_eq!(view.source, "external-edit");
        assert_eq!(
            view.conflicting_fields,
            BTreeSet::from([ConflictField::Tags, ConflictField::Note])
        );
        assert_eq!(store.get(view.id).expect("opaque plan").root_id, root_id);
        let replacement = store
            .capture(
                &record,
                &MetadataPatch {
                    favorite: Some(true),
                    ..MetadataPatch::default()
                },
            )
            .expect("replace plan for the same asset");
        assert!(store.get(view.id).is_err());
        assert_eq!(
            store
                .get(replacement.id)
                .expect("replacement opaque plan")
                .key,
            key
        );
        store
            .invalidate_keys([key.as_str()])
            .expect("invalidate after a successful edit");
        assert!(store.get(replacement.id).is_err());
        assert_eq!(
            fs::read(sidecar_path).expect("sidecar remains"),
            before_capture
        );
    }
}

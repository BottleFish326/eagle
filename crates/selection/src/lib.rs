use std::collections::{BTreeMap, BTreeSet};
use std::sync::Mutex;

use asset_catalog::AssetCatalog;
use asset_index::{AssetSort, QueryParseErrorKind, parse_query};
use chrono::{DateTime, Duration, SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

pub const MAX_SELECTION_SNAPSHOTS: usize = 32;
pub const MAX_SELECTION_ITEMS: usize = 100_000;
pub const MAX_TOTAL_SELECTION_ITEMS: usize = 200_000;
pub const MAX_EXPLICIT_SELECTION_ITEMS: usize = 4_096;
pub const SELECTION_TTL_MINUTES: i64 = 15;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuerySelectionInput {
    pub expected_catalog_revision: u64,
    pub expression: String,
    pub scope_root_ids: BTreeSet<Uuid>,
    pub sort: AssetSort,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RangeSelectionInput {
    #[serde(flatten)]
    pub query: QuerySelectionInput,
    pub anchor_key: String,
    pub target_key: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExplicitSelectionInput {
    pub expected_catalog_revision: u64,
    pub keys: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SelectionSnapshotSummary {
    pub id: Uuid,
    pub catalog_revision: u64,
    pub item_count: usize,
    pub created_at: String,
    pub expires_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectionSnapshotItem {
    pub key: String,
    pub stable_id: Option<Uuid>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedSelectionSnapshot {
    pub summary: SelectionSnapshotSummary,
    pub ordered_items: Vec<SelectionSnapshotItem>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SelectionSessionStats {
    pub snapshot_count: usize,
    pub total_item_count: usize,
    pub maximum_snapshot_count: usize,
    pub maximum_item_count: usize,
    pub maximum_total_item_count: usize,
}

#[derive(Debug, Error)]
pub enum SelectionError {
    #[error("selection session state is unavailable")]
    StateUnavailable,
    #[error("catalog changed before the selection snapshot was created")]
    CatalogChanged { actual_revision: u64 },
    #[error("selection query is invalid at byte offset {offset}")]
    InvalidQuery {
        kind: QueryParseErrorKind,
        offset: usize,
    },
    #[error("selection root scope must not be empty")]
    EmptyRootScope,
    #[error("selection snapshot would be empty")]
    EmptySelection,
    #[error("selection snapshot exceeds {MAX_SELECTION_ITEMS} items")]
    TooManyItems,
    #[error("explicit selection exceeds {MAX_EXPLICIT_SELECTION_ITEMS} submitted keys")]
    TooManyExplicitItems,
    #[error("selection session budget is exhausted")]
    SessionBudgetExceeded,
    #[error("selection range anchor is not in the current ordered result")]
    AnchorMissing,
    #[error("selection range target is not in the current ordered result")]
    TargetMissing,
    #[error("explicit selection asset is not in the current catalog")]
    AssetMissing,
    #[error("selection snapshot was not found")]
    SnapshotNotFound,
    #[error("selection snapshot expired")]
    SnapshotExpired,
}

#[derive(Debug, Default)]
struct SelectionSessions {
    snapshots: BTreeMap<Uuid, StoredSnapshot>,
    total_items: usize,
}

#[derive(Debug, Clone)]
struct StoredSnapshot {
    summary: SelectionSnapshotSummary,
    ordered_items: Vec<SelectionSnapshotItem>,
    expires_unix_ms: i64,
}

#[derive(Debug, Default)]
pub struct SelectionSessionStore {
    sessions: Mutex<SelectionSessions>,
}

impl SelectionSessionStore {
    /// Materializes the exact backend query/sort result at one catalog revision.
    ///
    /// # Errors
    ///
    /// Rejects stale revisions, invalid queries, empty/oversized results, and
    /// exhausted bounded session capacity.
    pub fn create_query_snapshot(
        &self,
        catalog: &AssetCatalog,
        input: &QuerySelectionInput,
    ) -> Result<SelectionSnapshotSummary, SelectionError> {
        let items = query_items(catalog, input)?;
        self.store_items(items, catalog.revision(), Utc::now())
    }

    /// Materializes an inclusive anchor-to-target slice from the current
    /// backend-owned ordered query result.
    ///
    /// # Errors
    ///
    /// Rejects stale revisions, missing endpoints, invalid queries, and session
    /// capacity violations.
    pub fn create_range_snapshot(
        &self,
        catalog: &AssetCatalog,
        input: &RangeSelectionInput,
    ) -> Result<SelectionSnapshotSummary, SelectionError> {
        let items = query_items(catalog, &input.query)?;
        let anchor = items
            .iter()
            .position(|item| item.key == input.anchor_key)
            .ok_or(SelectionError::AnchorMissing)?;
        let target = items
            .iter()
            .position(|item| item.key == input.target_key)
            .ok_or(SelectionError::TargetMissing)?;
        let (start, end) = if anchor <= target {
            (anchor, target)
        } else {
            (target, anchor)
        };
        self.store_items(items[start..=end].to_vec(), catalog.revision(), Utc::now())
    }

    /// Captures a bounded explicitly selected key sequence without accepting
    /// paths. Duplicate keys are removed while first occurrence order is kept.
    ///
    /// # Errors
    ///
    /// Rejects stale revisions, missing assets, empty/oversized input, and
    /// exhausted session capacity.
    pub fn create_explicit_snapshot(
        &self,
        catalog: &AssetCatalog,
        input: &ExplicitSelectionInput,
    ) -> Result<SelectionSnapshotSummary, SelectionError> {
        ensure_revision(catalog, input.expected_catalog_revision)?;
        if input.keys.len() > MAX_EXPLICIT_SELECTION_ITEMS {
            return Err(SelectionError::TooManyExplicitItems);
        }
        let mut seen = BTreeSet::new();
        let mut items = Vec::with_capacity(input.keys.len());
        for key in &input.keys {
            if !seen.insert(key.clone()) {
                continue;
            }
            let record = catalog.get(key).ok_or(SelectionError::AssetMissing)?;
            items.push(SelectionSnapshotItem {
                key: key.clone(),
                stable_id: record.id,
            });
        }
        self.store_items(items, catalog.revision(), Utc::now())
    }

    /// Resolves a still-live opaque snapshot without re-running its query.
    ///
    /// # Errors
    ///
    /// Returns a stable missing/expired/state error.
    pub fn resolve(&self, id: Uuid) -> Result<ResolvedSelectionSnapshot, SelectionError> {
        self.resolve_at(id, Utc::now())
    }

    /// Releases only one in-memory selection snapshot.
    ///
    /// # Errors
    ///
    /// Returns a state error when the bounded store lock is unavailable.
    pub fn release(&self, id: Uuid) -> Result<bool, SelectionError> {
        let mut sessions = self
            .sessions
            .lock()
            .map_err(|_| SelectionError::StateUnavailable)?;
        let removed = sessions.snapshots.remove(&id);
        if let Some(snapshot) = removed {
            sessions.total_items = sessions
                .total_items
                .saturating_sub(snapshot.ordered_items.len());
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Returns only bounded counts, never keys or paths.
    ///
    /// # Errors
    ///
    /// Returns a state error when the bounded store lock is unavailable.
    pub fn stats(&self) -> Result<SelectionSessionStats, SelectionError> {
        let mut sessions = self
            .sessions
            .lock()
            .map_err(|_| SelectionError::StateUnavailable)?;
        prune_expired(&mut sessions, Utc::now().timestamp_millis());
        Ok(SelectionSessionStats {
            snapshot_count: sessions.snapshots.len(),
            total_item_count: sessions.total_items,
            maximum_snapshot_count: MAX_SELECTION_SNAPSHOTS,
            maximum_item_count: MAX_SELECTION_ITEMS,
            maximum_total_item_count: MAX_TOTAL_SELECTION_ITEMS,
        })
    }

    fn store_items(
        &self,
        items: Vec<SelectionSnapshotItem>,
        catalog_revision: u64,
        now: DateTime<Utc>,
    ) -> Result<SelectionSnapshotSummary, SelectionError> {
        if items.is_empty() {
            return Err(SelectionError::EmptySelection);
        }
        if items.len() > MAX_SELECTION_ITEMS {
            return Err(SelectionError::TooManyItems);
        }
        let mut sessions = self
            .sessions
            .lock()
            .map_err(|_| SelectionError::StateUnavailable)?;
        prune_expired(&mut sessions, now.timestamp_millis());
        if sessions.snapshots.len() >= MAX_SELECTION_SNAPSHOTS
            || sessions.total_items.saturating_add(items.len()) > MAX_TOTAL_SELECTION_ITEMS
        {
            return Err(SelectionError::SessionBudgetExceeded);
        }
        let expires = now + Duration::minutes(SELECTION_TTL_MINUTES);
        let summary = SelectionSnapshotSummary {
            id: Uuid::now_v7(),
            catalog_revision,
            item_count: items.len(),
            created_at: timestamp(now),
            expires_at: timestamp(expires),
        };
        sessions.total_items += items.len();
        sessions.snapshots.insert(
            summary.id,
            StoredSnapshot {
                summary: summary.clone(),
                ordered_items: items,
                expires_unix_ms: expires.timestamp_millis(),
            },
        );
        Ok(summary)
    }

    fn resolve_at(
        &self,
        id: Uuid,
        now: DateTime<Utc>,
    ) -> Result<ResolvedSelectionSnapshot, SelectionError> {
        let mut sessions = self
            .sessions
            .lock()
            .map_err(|_| SelectionError::StateUnavailable)?;
        let Some(snapshot) = sessions.snapshots.get(&id) else {
            return Err(SelectionError::SnapshotNotFound);
        };
        if snapshot.expires_unix_ms <= now.timestamp_millis() {
            let removed = sessions
                .snapshots
                .remove(&id)
                .expect("snapshot exists after lookup");
            sessions.total_items = sessions
                .total_items
                .saturating_sub(removed.ordered_items.len());
            return Err(SelectionError::SnapshotExpired);
        }
        Ok(ResolvedSelectionSnapshot {
            summary: snapshot.summary.clone(),
            ordered_items: snapshot.ordered_items.clone(),
        })
    }
}

fn query_items(
    catalog: &AssetCatalog,
    input: &QuerySelectionInput,
) -> Result<Vec<SelectionSnapshotItem>, SelectionError> {
    ensure_revision(catalog, input.expected_catalog_revision)?;
    if input.scope_root_ids.is_empty() {
        return Err(SelectionError::EmptyRootScope);
    }
    let query = parse_query(&input.expression).map_err(|error| SelectionError::InvalidQuery {
        kind: error.kind,
        offset: error.offset,
    })?;
    Ok(catalog
        .query_ordered(&query, &input.scope_root_ids, input.sort)
        .into_iter()
        .filter_map(|key| {
            let stable_id = catalog.get(&key)?.id;
            Some(SelectionSnapshotItem { key, stable_id })
        })
        .collect())
}

fn ensure_revision(catalog: &AssetCatalog, expected: u64) -> Result<(), SelectionError> {
    if catalog.revision() == expected {
        Ok(())
    } else {
        Err(SelectionError::CatalogChanged {
            actual_revision: catalog.revision(),
        })
    }
}

fn prune_expired(sessions: &mut SelectionSessions, now_unix_ms: i64) {
    let expired = sessions
        .snapshots
        .iter()
        .filter_map(|(id, snapshot)| (snapshot.expires_unix_ms <= now_unix_ms).then_some(*id))
        .collect::<Vec<_>>();
    for id in expired {
        if let Some(snapshot) = sessions.snapshots.remove(&id) {
            sessions.total_items = sessions
                .total_items
                .saturating_sub(snapshot.ordered_items.len());
        }
    }
}

fn timestamp(value: DateTime<Utc>) -> String {
    value.to_rfc3339_opts(SecondsFormat::Millis, true)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use asset_core::AssetRecord;
    use asset_index::{AssetSortDirection, AssetSortField};

    use super::*;

    #[test]
    fn query_snapshot_is_exact_after_catalog_expands() {
        let root = Uuid::now_v7();
        let mut catalog = AssetCatalog::default();
        catalog.ingest([record("b", root), record("a", root)]);
        let store = SelectionSessionStore::default();
        let summary = store
            .create_query_snapshot(&catalog, &query_input(catalog.revision(), root))
            .expect("query snapshot");
        assert_eq!(summary.item_count, 2);

        catalog.ingest([record("new", root)]);
        let resolved = store.resolve(summary.id).expect("resolve snapshot");
        assert_eq!(
            resolved
                .ordered_items
                .iter()
                .map(|item| item.key.as_str())
                .collect::<Vec<_>>(),
            vec!["a", "b"]
        );
        assert_eq!(resolved.summary.catalog_revision, 1);
        assert_eq!(catalog.revision(), 2);
    }

    #[test]
    fn stale_revision_range_and_explicit_order_have_precise_errors() {
        let root = Uuid::now_v7();
        let mut catalog = AssetCatalog::default();
        catalog.ingest([record("a", root), record("b", root), record("c", root)]);
        let store = SelectionSessionStore::default();
        let mut stale = query_input(catalog.revision(), root);
        catalog.ingest([record("d", root)]);
        assert!(matches!(
            store.create_query_snapshot(&catalog, &stale),
            Err(SelectionError::CatalogChanged { actual_revision: 2 })
        ));

        stale.expected_catalog_revision = catalog.revision();
        let range = store
            .create_range_snapshot(
                &catalog,
                &RangeSelectionInput {
                    query: stale,
                    anchor_key: "c".into(),
                    target_key: "a".into(),
                },
            )
            .expect("reverse range");
        let resolved = store.resolve(range.id).expect("range snapshot");
        assert_eq!(
            resolved
                .ordered_items
                .iter()
                .map(|item| item.key.as_str())
                .collect::<Vec<_>>(),
            vec!["a", "b", "c"]
        );

        let explicit = store
            .create_explicit_snapshot(
                &catalog,
                &ExplicitSelectionInput {
                    expected_catalog_revision: catalog.revision(),
                    keys: vec!["c".into(), "a".into(), "c".into()],
                },
            )
            .expect("explicit snapshot");
        assert_eq!(
            store
                .resolve(explicit.id)
                .expect("explicit result")
                .ordered_items
                .iter()
                .map(|item| item.key.as_str())
                .collect::<Vec<_>>(),
            vec!["c", "a"]
        );
    }

    #[test]
    fn expiration_release_and_budget_counts_never_persist_items() {
        let root = Uuid::now_v7();
        let mut catalog = AssetCatalog::default();
        catalog.ingest([record("a", root)]);
        let store = SelectionSessionStore::default();
        let now = Utc::now();
        let summary = store
            .store_items(
                vec![SelectionSnapshotItem {
                    key: "a".into(),
                    stable_id: None,
                }],
                catalog.revision(),
                now,
            )
            .expect("snapshot");
        assert!(matches!(
            store.resolve_at(summary.id, now + Duration::minutes(SELECTION_TTL_MINUTES)),
            Err(SelectionError::SnapshotExpired)
        ));
        assert_eq!(store.stats().expect("stats").total_item_count, 0);

        let live = store
            .create_query_snapshot(&catalog, &query_input(catalog.revision(), root))
            .expect("live snapshot");
        assert!(store.release(live.id).expect("release"));
        assert!(!store.release(live.id).expect("release missing"));
        assert_eq!(store.stats().expect("empty stats").snapshot_count, 0);
    }

    #[test]
    fn maximum_query_snapshot_is_exact_bounded_and_releasable() {
        let root = Uuid::now_v7();
        let mut catalog = AssetCatalog::default();
        catalog.ingest(
            (0..MAX_SELECTION_ITEMS).map(|index| record(&format!("asset-{index:06}"), root)),
        );
        let store = SelectionSessionStore::default();
        let summary = store
            .create_query_snapshot(&catalog, &query_input(catalog.revision(), root))
            .expect("maximum supported snapshot");
        assert_eq!(summary.item_count, MAX_SELECTION_ITEMS);
        let resolved = store.resolve(summary.id).expect("resolve maximum snapshot");
        assert_eq!(resolved.ordered_items.len(), MAX_SELECTION_ITEMS);
        assert_eq!(resolved.ordered_items[0].key, "asset-000000");
        assert_eq!(
            resolved.ordered_items[MAX_SELECTION_ITEMS - 1].key,
            "asset-099999"
        );
        assert!(store.release(summary.id).expect("release maximum snapshot"));
        assert_eq!(store.stats().expect("released stats").total_item_count, 0);
    }

    fn query_input(revision: u64, root: Uuid) -> QuerySelectionInput {
        QuerySelectionInput {
            expected_catalog_revision: revision,
            expression: String::new(),
            scope_root_ids: BTreeSet::from([root]),
            sort: AssetSort {
                field: AssetSortField::FileName,
                direction: AssetSortDirection::Ascending,
            },
        }
    }

    fn record(key: &str, root: Uuid) -> AssetRecord {
        let mut record = AssetRecord::untagged(
            key.into(),
            PathBuf::from(format!("/assets/{key}.png")),
            "image/png".into(),
            1,
            1,
        );
        record.root_id = Some(root);
        record.id = Some(Uuid::now_v7());
        record
    }
}

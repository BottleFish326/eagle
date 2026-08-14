use std::collections::{BTreeSet, HashMap, HashSet};

use asset_core::{AssetKind, AssetRecord};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AssetQuery {
    pub all_tags: BTreeSet<String>,
    pub any_tags: BTreeSet<String>,
    pub excluded_tags: BTreeSet<String>,
    pub kind: Option<AssetKind>,
    pub favorite: Option<bool>,
}

#[derive(Debug, Default)]
pub struct AssetIndex {
    records: HashMap<String, AssetRecord>,
    tags: HashMap<String, HashSet<String>>,
    kinds: HashMap<AssetKind, HashSet<String>>,
    favorites: HashMap<bool, HashSet<String>>,
}

impl AssetIndex {
    #[must_use]
    pub fn from_records(records: impl IntoIterator<Item = AssetRecord>) -> Self {
        let mut index = Self::default();
        for record in records {
            index.upsert(record);
        }
        index
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.records.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    pub fn upsert(&mut self, record: AssetRecord) {
        let key = record.key.clone();
        if let Some(previous) = self.records.remove(&key) {
            self.remove_postings(&previous);
        }
        self.add_postings(&record);
        self.records.insert(key, record);
    }

    pub fn remove(&mut self, key: &str) -> Option<AssetRecord> {
        let record = self.records.remove(key)?;
        self.remove_postings(&record);
        Some(record)
    }

    #[must_use]
    pub fn get(&self, key: &str) -> Option<&AssetRecord> {
        self.records.get(key)
    }

    #[must_use]
    pub fn query(&self, query: &AssetQuery) -> BTreeSet<String> {
        let mut result: HashSet<String> = self.records.keys().cloned().collect();

        for tag in &query.all_tags {
            let required = self.keys_for_tag_expression(tag);
            result.retain(|key| required.contains(key));
        }

        if !query.any_tags.is_empty() {
            let any_keys = query
                .any_tags
                .iter()
                .flat_map(|tag| self.keys_for_tag_expression(tag))
                .collect::<HashSet<_>>();
            result.retain(|key| any_keys.contains(key));
        }

        for tag in &query.excluded_tags {
            let excluded = self.keys_for_tag_expression(tag);
            result.retain(|key| !excluded.contains(key));
        }

        if let Some(kind) = query.kind {
            let keys = self.kinds.get(&kind);
            result.retain(|key| keys.is_some_and(|set| set.contains(key)));
        }

        if let Some(favorite) = query.favorite {
            let keys = self.favorites.get(&favorite);
            result.retain(|key| keys.is_some_and(|set| set.contains(key)));
        }

        result.into_iter().collect()
    }

    fn keys_for_tag_expression(&self, expression: &str) -> HashSet<String> {
        if let Some(namespace) = expression.strip_suffix("/*") {
            let prefix = format!("{namespace}/");
            self.tags
                .iter()
                .filter(|(tag, _)| tag.as_str() == namespace || tag.starts_with(&prefix))
                .flat_map(|(_, keys)| keys.iter().cloned())
                .collect()
        } else {
            self.tags.get(expression).cloned().unwrap_or_default()
        }
    }

    fn add_postings(&mut self, record: &AssetRecord) {
        for tag in &record.tags {
            self.tags
                .entry(tag.clone())
                .or_default()
                .insert(record.key.clone());
        }
        self.kinds
            .entry(record.kind)
            .or_default()
            .insert(record.key.clone());
        self.favorites
            .entry(record.favorite)
            .or_default()
            .insert(record.key.clone());
    }

    fn remove_postings(&mut self, record: &AssetRecord) {
        for tag in &record.tags {
            remove_key(&mut self.tags, tag, &record.key);
        }
        remove_key(&mut self.kinds, &record.kind, &record.key);
        remove_key(&mut self.favorites, &record.favorite, &record.key);
    }
}

fn remove_key<K>(postings: &mut HashMap<K, HashSet<String>>, group: &K, key: &str)
where
    K: std::hash::Hash + Eq,
{
    if let Some(keys) = postings.get_mut(group) {
        keys.remove(key);
        if keys.is_empty() {
            postings.remove(group);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::path::PathBuf;

    use asset_core::AssetRecord;

    use super::{AssetIndex, AssetQuery};

    fn record(key: &str, tags: &[&str], favorite: bool) -> AssetRecord {
        let mut record = AssetRecord::untagged(
            key.into(),
            PathBuf::from(format!("/{key}.png")),
            "image/png".into(),
            1,
            0,
        );
        record.tags = tags.iter().map(|tag| (*tag).to_owned()).collect();
        record.favorite = favorite;
        record
    }

    #[test]
    fn combines_all_any_excluded_and_namespace_tags() {
        let index = AssetIndex::from_records([
            record("a", &["ui/icon", "color/blue"], true),
            record("b", &["ui/photo", "color/blue"], false),
            record("c", &["ui/icon", "draft"], false),
        ]);
        let query = AssetQuery {
            all_tags: BTreeSet::from(["ui/*".into()]),
            any_tags: BTreeSet::from(["color/blue".into(), "draft".into()]),
            excluded_tags: BTreeSet::from(["draft".into()]),
            kind: None,
            favorite: None,
        };

        assert_eq!(
            index.query(&query),
            BTreeSet::from(["a".into(), "b".into()])
        );
    }

    #[test]
    fn upsert_replaces_old_postings() {
        let mut index = AssetIndex::from_records([record("a", &["old"], false)]);
        index.upsert(record("a", &["new"], true));

        let old = AssetQuery {
            all_tags: BTreeSet::from(["old".into()]),
            ..AssetQuery::default()
        };
        let new = AssetQuery {
            all_tags: BTreeSet::from(["new".into()]),
            favorite: Some(true),
            ..AssetQuery::default()
        };

        assert!(index.query(&old).is_empty());
        assert_eq!(index.query(&new), BTreeSet::from(["a".into()]));
    }
}

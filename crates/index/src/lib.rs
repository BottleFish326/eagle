use std::collections::{BTreeSet, HashMap, HashSet};

use asset_core::{AssetKind, AssetRecord};

mod query;

pub use query::{AssetQuery, QueryParseError, QueryParseErrorKind, parse_query};

#[derive(Debug, Default)]
pub struct AssetIndex {
    records: HashMap<String, AssetRecord>,
    tags: HashMap<String, HashSet<String>>,
    kinds: HashMap<AssetKind, HashSet<String>>,
    extensions: HashMap<String, HashSet<String>>,
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

        for group in &query.any_tag_groups {
            let any_keys = group
                .iter()
                .flat_map(|tag| self.keys_for_tag_expression(tag))
                .collect::<HashSet<_>>();
            result.retain(|key| any_keys.contains(key));
        }

        for tag in &query.excluded_tags {
            let excluded = self.keys_for_tag_expression(tag);
            result.retain(|key| !excluded.contains(key));
        }

        if !query.kinds.is_empty() {
            let kinds = query
                .kinds
                .iter()
                .filter_map(|kind| self.kinds.get(kind))
                .flatten()
                .collect::<HashSet<_>>();
            result.retain(|key| kinds.contains(key));
        }

        if !query.extensions.is_empty() {
            let extensions = query
                .extensions
                .iter()
                .filter_map(|extension| self.extensions.get(extension))
                .flatten()
                .collect::<HashSet<_>>();
            result.retain(|key| extensions.contains(key));
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
                .filter(|(tag, _)| tag.starts_with(&prefix))
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
        if let Some(extension) = &record.extension {
            self.extensions
                .entry(extension.to_ascii_lowercase())
                .or_default()
                .insert(record.key.clone());
        }
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
        if let Some(extension) = &record.extension {
            remove_key(
                &mut self.extensions,
                &extension.to_ascii_lowercase(),
                &record.key,
            );
        }
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

    use super::{AssetIndex, AssetQuery, parse_query};

    fn record(key: &str, tags: &[&str], favorite: bool) -> AssetRecord {
        typed_record(key, "png", "image/png", tags, favorite)
    }

    fn typed_record(
        key: &str,
        extension: &str,
        mime: &str,
        tags: &[&str],
        favorite: bool,
    ) -> AssetRecord {
        let mut record = AssetRecord::untagged(
            key.into(),
            PathBuf::from(format!("/{key}.{extension}")),
            mime.into(),
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
            typed_record("b", "jpg", "image/jpeg", &["ui/photo", "color/red"], false),
            typed_record(
                "c",
                "mp4",
                "video/mp4",
                &["ui/icon", "color/blue", "draft"],
                true,
            ),
            record("namespace-only", &["ui", "color/blue"], true),
        ]);
        let query = parse_query(
            "ui/* any:(color/blue|color/red) -draft type:image ext:png|jpg favorite:true",
        )
        .expect("parse query");

        assert_eq!(index.query(&query), BTreeSet::from(["a".into()]));

        let and_query = parse_query("ui/icon color/blue").expect("parse AND query");
        assert_eq!(
            index.query(&and_query),
            BTreeSet::from(["a".into(), "c".into()])
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

    #[test]
    fn upsert_replaces_extension_postings() {
        let mut index =
            AssetIndex::from_records([typed_record("a", "png", "image/png", &[], false)]);
        index.upsert(typed_record("a", "jpg", "image/jpeg", &[], false));

        assert!(
            index
                .query(&parse_query("ext:png").expect("PNG query"))
                .is_empty()
        );
        assert_eq!(
            index.query(&parse_query("ext:JPG").expect("JPEG query")),
            BTreeSet::from(["a".into()])
        );
    }

    #[test]
    fn multiple_or_groups_are_combined_with_and() {
        let index = AssetIndex::from_records([
            record("a", &["color/blue", "usage/hero"], false),
            record("b", &["color/red", "usage/card"], false),
            record("c", &["color/blue", "usage/other"], false),
            record("d", &["color/green", "usage/hero"], false),
        ]);
        let query = parse_query("any:(color/blue|color/red) any:(usage/hero|usage/card)")
            .expect("parse OR groups");

        assert_eq!(
            index.query(&query),
            BTreeSet::from(["a".into(), "b".into()])
        );
    }

    #[test]
    fn large_query_matches_an_independent_linear_filter() {
        let records = (0..10_000)
            .map(|index| {
                let mut tags = Vec::new();
                if index % 2 == 0 {
                    tags.push("group/even");
                }
                if index % 11 == 0 {
                    tags.push("state/draft");
                }
                record(&index.to_string(), &tags, index % 13 == 0)
            })
            .collect::<Vec<_>>();
        let expected = records
            .iter()
            .filter(|record| {
                record.tags.contains("group/even") && !record.tags.contains("state/draft")
            })
            .map(|record| record.key.clone())
            .collect::<BTreeSet<_>>();
        let index = AssetIndex::from_records(records);
        let query = AssetQuery {
            all_tags: BTreeSet::from(["group/even".into()]),
            excluded_tags: BTreeSet::from(["state/draft".into()]),
            ..AssetQuery::default()
        };

        assert_eq!(index.query(&query), expected);
    }
}

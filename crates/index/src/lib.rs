use std::collections::{BTreeSet, HashMap, HashSet};

use asset_core::{AssetKind, AssetRecord};
use unicode_normalization::UnicodeNormalization;
use uuid::Uuid;

mod advanced;
mod query;

pub use advanced::{
    InstantField, IntegerField, NullableBoolean, Orientation, RangeBound, RangeConstraint, Ratio,
    RatioField, UnknownField,
};
pub use query::{AssetQuery, QueryParseError, QueryParseErrorKind, parse_query};

#[derive(Debug, Default)]
pub struct AssetIndex {
    records: HashMap<String, AssetRecord>,
    tags: HashMap<String, HashSet<String>>,
    kinds: HashMap<AssetKind, HashSet<String>>,
    extensions: HashMap<String, HashSet<String>>,
    favorites: HashMap<bool, HashSet<String>>,
    roots: HashMap<Uuid, HashSet<String>>,
    orientations: HashMap<Orientation, HashSet<String>>,
    color_spaces: HashMap<String, HashSet<String>>,
    has_notes: HashMap<bool, HashSet<String>>,
    has_alphas: HashMap<Option<bool>, HashSet<String>>,
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

    pub fn clear(&mut self) {
        self.records.clear();
        self.tags.clear();
        self.kinds.clear();
        self.extensions.clear();
        self.favorites.clear();
        self.roots.clear();
        self.orientations.clear();
        self.color_spaces.clear();
        self.has_notes.clear();
        self.has_alphas.clear();
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

        if !query.root_ids.is_empty() {
            let roots = query
                .root_ids
                .iter()
                .filter_map(|root| self.roots.get(root))
                .flatten()
                .collect::<HashSet<_>>();
            result.retain(|key| roots.contains(key));
        }

        if !query.orientations.is_empty() {
            let orientations = query
                .orientations
                .iter()
                .filter_map(|orientation| self.orientations.get(orientation))
                .flatten()
                .collect::<HashSet<_>>();
            result.retain(|key| orientations.contains(key));
        }

        if !query.color_spaces.is_empty() {
            let color_spaces = query
                .color_spaces
                .iter()
                .filter_map(|color_space| self.color_spaces.get(color_space))
                .flatten()
                .collect::<HashSet<_>>();
            result.retain(|key| color_spaces.contains(key));
        }

        if let Some(has_note) = query.has_note {
            let keys = self.has_notes.get(&has_note);
            result.retain(|key| keys.is_some_and(|set| set.contains(key)));
        }

        if let Some(has_alpha) = query.has_alpha {
            let value = match has_alpha {
                NullableBoolean::Known(value) => Some(value),
                NullableBoolean::Unknown => None,
            };
            let keys = self.has_alphas.get(&value);
            result.retain(|key| keys.is_some_and(|set| set.contains(key)));
        }

        result.retain(|key| {
            self.records
                .get(key)
                .is_some_and(|record| matches_advanced_query(record, query))
        });

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
        if let Some(root_id) = record.root_id {
            self.roots
                .entry(root_id)
                .or_default()
                .insert(record.key.clone());
        }
        if let Some(orientation) = record_orientation(record) {
            self.orientations
                .entry(orientation)
                .or_default()
                .insert(record.key.clone());
        }
        if let Some(color_space) = record
            .media
            .as_ref()
            .and_then(|media| media.color_space.as_ref())
        {
            self.color_spaces
                .entry(color_space.clone())
                .or_default()
                .insert(record.key.clone());
        }
        self.has_notes
            .entry(!record.note.trim().is_empty())
            .or_default()
            .insert(record.key.clone());
        self.has_alphas
            .entry(record.media.as_ref().and_then(|media| media.has_alpha))
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
        if let Some(root_id) = record.root_id {
            remove_key(&mut self.roots, &root_id, &record.key);
        }
        if let Some(orientation) = record_orientation(record) {
            remove_key(&mut self.orientations, &orientation, &record.key);
        }
        if let Some(color_space) = record
            .media
            .as_ref()
            .and_then(|media| media.color_space.as_ref())
        {
            remove_key(&mut self.color_spaces, color_space, &record.key);
        }
        remove_key(
            &mut self.has_notes,
            &(!record.note.trim().is_empty()),
            &record.key,
        );
        remove_key(
            &mut self.has_alphas,
            &record.media.as_ref().and_then(|media| media.has_alpha),
            &record.key,
        );
    }
}

fn matches_advanced_query(record: &AssetRecord, query: &AssetQuery) -> bool {
    for (field, range) in &query.integer_ranges {
        let value = match field {
            IntegerField::Rating => Some(u64::from(record.rating)),
            IntegerField::Size => record.size,
            IntegerField::Width => effective_dimensions(record).map(|(width, _)| u64::from(width)),
            IntegerField::Height => {
                effective_dimensions(record).map(|(_, height)| u64::from(height))
            }
            IntegerField::Duration => record.media.as_ref().and_then(|media| media.duration_ms),
            IntegerField::Pages => record
                .media
                .as_ref()
                .and_then(|media| media.page_count)
                .map(u64::from),
        };
        if !value.is_some_and(|value| range_contains(range, &value)) {
            return false;
        }
    }

    for (field, range) in &query.instant_ranges {
        let value = match field {
            InstantField::Created => record.created_unix_ms,
            InstantField::Modified => record.modified_unix_ms,
        };
        if !value.is_some_and(|value| range_contains(range, &value)) {
            return false;
        }
    }

    for (field, range) in &query.ratio_ranges {
        let value = match field {
            RatioField::Aspect => effective_dimensions(record).map(|(width, height)| Ratio {
                numerator: width,
                denominator: height,
            }),
        };
        if !value.is_some_and(|value| range_contains(range, &value)) {
            return false;
        }
    }

    if query
        .unknown_fields
        .iter()
        .any(|field| !field_is_unknown(record, *field))
    {
        return false;
    }

    if !query.orientations.is_empty() {
        let Some(orientation) =
            effective_dimensions(record).map(|(width, height)| match width.cmp(&height) {
                std::cmp::Ordering::Greater => Orientation::Landscape,
                std::cmp::Ordering::Less => Orientation::Portrait,
                std::cmp::Ordering::Equal => Orientation::Square,
            })
        else {
            return false;
        };
        if !query.orientations.contains(&orientation) {
            return false;
        }
    }

    if !query.color_spaces.is_empty()
        && !record
            .media
            .as_ref()
            .and_then(|media| media.color_space.as_ref())
            .is_some_and(|color_space| query.color_spaces.contains(color_space))
    {
        return false;
    }

    if let Some(has_note) = query.has_note {
        let actual = !record.note.trim().is_empty();
        if actual != has_note {
            return false;
        }
    }

    if let Some(has_alpha) = query.has_alpha {
        let actual = record.media.as_ref().and_then(|media| media.has_alpha);
        match has_alpha {
            NullableBoolean::Known(expected) if actual != Some(expected) => return false,
            NullableBoolean::Unknown if actual.is_some() => return false,
            NullableBoolean::Known(_) | NullableBoolean::Unknown => {}
        }
    }

    let relative_path = record
        .relative_path
        .to_string_lossy()
        .replace('\\', "/")
        .nfc()
        .collect::<String>();
    query
        .path_contains
        .iter()
        .all(|fragment| relative_path.contains(fragment))
}

fn range_contains<T: Ord>(range: &RangeConstraint<T>, value: &T) -> bool {
    let lower_matches = range
        .lower
        .as_ref()
        .is_none_or(|lower| match value.cmp(&lower.value) {
            std::cmp::Ordering::Greater => true,
            std::cmp::Ordering::Equal => lower.inclusive,
            std::cmp::Ordering::Less => false,
        });
    let upper_matches = range
        .upper
        .as_ref()
        .is_none_or(|upper| match value.cmp(&upper.value) {
            std::cmp::Ordering::Less => true,
            std::cmp::Ordering::Equal => upper.inclusive,
            std::cmp::Ordering::Greater => false,
        });
    lower_matches && upper_matches
}

fn effective_dimensions(record: &AssetRecord) -> Option<(u32, u32)> {
    let dimensions = record.dimensions?;
    let image_swaps_axes = record
        .native_metadata
        .as_ref()
        .and_then(|metadata| metadata.orientation)
        .is_some_and(|orientation| (5..=8).contains(&orientation));
    let video_swaps_axes = record
        .media
        .as_ref()
        .and_then(|media| media.display_quarter_turns)
        .is_some_and(|turns| turns % 2 == 1);
    let swaps_axes = image_swaps_axes || video_swaps_axes;
    if swaps_axes {
        Some((dimensions.height, dimensions.width))
    } else {
        Some((dimensions.width, dimensions.height))
    }
}

fn record_orientation(record: &AssetRecord) -> Option<Orientation> {
    effective_dimensions(record).map(|(width, height)| match width.cmp(&height) {
        std::cmp::Ordering::Greater => Orientation::Landscape,
        std::cmp::Ordering::Less => Orientation::Portrait,
        std::cmp::Ordering::Equal => Orientation::Square,
    })
}

fn field_is_unknown(record: &AssetRecord, field: UnknownField) -> bool {
    match field {
        UnknownField::Size => record.size.is_none(),
        UnknownField::Width | UnknownField::Height | UnknownField::Aspect => {
            effective_dimensions(record).is_none()
        }
        UnknownField::Created => record.created_unix_ms.is_none(),
        UnknownField::Modified => record.modified_unix_ms.is_none(),
        UnknownField::Duration => record
            .media
            .as_ref()
            .and_then(|media| media.duration_ms)
            .is_none(),
        UnknownField::Pages => record
            .media
            .as_ref()
            .and_then(|media| media.page_count)
            .is_none(),
        UnknownField::Orientation => effective_dimensions(record).is_none(),
        UnknownField::Root => record.root_id.is_none(),
        UnknownField::ColorSpace => record
            .media
            .as_ref()
            .and_then(|media| media.color_space.as_ref())
            .is_none(),
        UnknownField::HasAlpha => record
            .media
            .as_ref()
            .and_then(|media| media.has_alpha)
            .is_none(),
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

    use asset_core::{AssetDimensions, AssetRecord, MediaProperties, NativeImageMetadata};
    use uuid::Uuid;

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

    fn advanced_record(key: &str, root_id: Option<Uuid>) -> AssetRecord {
        let mut record = typed_record(key, "jpg", "image/jpeg", &["portfolio"], true);
        record.root_id = root_id;
        record.relative_path = PathBuf::from(format!("Brand Assets/{key}.jpg"));
        record.size = Some(20 * 1024 * 1024);
        record.created_unix_ms = Some(1_700_000_000_000);
        record.modified_unix_ms = Some(1_800_000_000_000);
        record.dimensions = Some(AssetDimensions {
            width: 1080,
            height: 1920,
        });
        record.native_metadata = Some(NativeImageMetadata {
            orientation: Some(6),
            ..NativeImageMetadata::default()
        });
        record.media = Some(MediaProperties {
            duration_ms: Some(30_000),
            page_count: Some(2),
            color_space: Some("srgb".into()),
            has_alpha: Some(false),
            ..MediaProperties::default()
        });
        record.rating = 4;
        record.note = "  curated  ".into();
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
    fn clear_removes_records_and_all_postings() {
        let mut index = AssetIndex::from_records([
            record("a", &["ui/icon"], true),
            record("b", &["color/blue"], false),
        ]);

        index.clear();

        assert!(index.is_empty());
        assert!(index.query(&AssetQuery::default()).is_empty());
        assert!(
            index
                .query(&AssetQuery {
                    all_tags: BTreeSet::from(["ui/icon".into()]),
                    ..AssetQuery::default()
                })
                .is_empty()
        );
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

    #[test]
    fn advanced_ranges_use_display_dimensions_and_exact_missing_semantics() {
        let root = Uuid::parse_str("0198e8c0-7451-7af1-8bca-0123456789ab").expect("root");
        let known = advanced_record("éclair", Some(root));
        let mut unknown = typed_record("unknown", "jpg", "image/jpeg", &[], false);
        unknown.size = None;
        unknown.created_unix_ms = None;
        unknown.modified_unix_ms = None;
        unknown.dimensions = None;
        unknown.media = None;
        let index = AssetIndex::from_records([known, unknown]);

        for expression in [
            "portfolio".to_owned(),
            format!("root:{root}"),
            "path:écl".to_owned(),
            "rating:>=4".to_owned(),
            "size:>=10MiB".to_owned(),
            "width:1920".to_owned(),
            "height:1080".to_owned(),
            "aspect:16/9".to_owned(),
            "orientation:landscape".to_owned(),
            "created:>=2023-01-01T00:00:00Z".to_owned(),
            "modified:<2030-01-01T00:00:00Z".to_owned(),
            "duration:30s".to_owned(),
            "pages:2".to_owned(),
            "color-space:srgb".to_owned(),
            "has-note:true".to_owned(),
            "has-alpha:false".to_owned(),
        ] {
            assert_eq!(
                index.query(&parse_query(&expression).expect("single advanced predicate")),
                BTreeSet::from(["éclair".into()]),
                "expression: {expression}",
            );
        }

        let query = parse_query(&format!(
            "portfolio root:{root} path:écl rating:>=4 size:>=10MiB width:1920 \
             height:1080 aspect:16/9 orientation:landscape \
             created:>=2023-01-01T00:00:00Z modified:<2030-01-01T00:00:00Z \
             duration:30s pages:2 color-space:srgb has-note:true has-alpha:false",
        ))
        .expect("advanced query");
        assert_eq!(index.query(&query), BTreeSet::from(["éclair".into()]));

        let unknown_query = parse_query(
            "size:unknown width:unknown created:unknown modified:unknown \
             duration:unknown pages:unknown color-space:unknown has-alpha:unknown root:unknown",
        )
        .expect("unknown query");
        assert_eq!(
            index.query(&unknown_query),
            BTreeSet::from(["unknown".into()])
        );
    }

    #[test]
    fn root_postings_are_replaced_and_removed_without_stale_matches() {
        let root_a = Uuid::parse_str("0198e8c0-7451-7af1-8bca-0123456789ab").expect("root A");
        let root_b = Uuid::parse_str("0198e8c0-7451-7af1-8bca-abcdef012345").expect("root B");
        let mut index = AssetIndex::from_records([advanced_record("asset", Some(root_a))]);
        index.upsert(advanced_record("asset", Some(root_b)));

        assert!(
            index
                .query(&parse_query(&format!("root:{root_a}")).expect("root A query"))
                .is_empty()
        );
        assert_eq!(
            index.query(&parse_query(&format!("root:{root_b}")).expect("root B query")),
            BTreeSet::from(["asset".into()])
        );
        index.remove("asset");
        assert!(
            index
                .query(&parse_query(&format!("root:{root_b}")).expect("removed root query"))
                .is_empty()
        );
    }

    #[test]
    fn category_postings_are_replaced_and_removed_without_stale_matches() {
        let mut index = AssetIndex::from_records([advanced_record("asset", None)]);
        let mut replacement = advanced_record("asset", None);
        replacement.dimensions = Some(AssetDimensions {
            width: 640,
            height: 640,
        });
        replacement.native_metadata = None;
        replacement.note.clear();
        replacement.media = Some(MediaProperties {
            color_space: Some("display-p3".into()),
            has_alpha: Some(true),
            ..MediaProperties::default()
        });
        index.upsert(replacement);

        for stale in [
            "orientation:landscape",
            "color-space:srgb",
            "has-note:true",
            "has-alpha:false",
        ] {
            assert!(
                index
                    .query(&parse_query(stale).expect("stale category query"))
                    .is_empty(),
                "stale posting matched: {stale}",
            );
        }
        for current in [
            "orientation:square",
            "color-space:display-p3",
            "has-note:false",
            "has-alpha:true",
        ] {
            assert_eq!(
                index.query(&parse_query(current).expect("current category query")),
                BTreeSet::from(["asset".into()]),
                "current posting missing: {current}",
            );
        }

        index.remove("asset");
        assert!(
            index
                .query(&parse_query("orientation:square").expect("removed category query"))
                .is_empty()
        );
    }

    #[test]
    fn video_display_rotation_changes_effective_dimensions() {
        let mut record = typed_record("rotated-video", "mp4", "video/mp4", &[], false);
        record.dimensions = Some(AssetDimensions {
            width: 1080,
            height: 1920,
        });
        record.media = Some(MediaProperties {
            display_quarter_turns: Some(1),
            ..MediaProperties::default()
        });
        let index = AssetIndex::from_records([record]);

        for expression in [
            "width:1920",
            "height:1080",
            "aspect:16/9",
            "orientation:landscape",
        ] {
            assert_eq!(
                index.query(&parse_query(expression).expect("rotated video query")),
                BTreeSet::from(["rotated-video".into()]),
                "expression: {expression}",
            );
        }
    }
}

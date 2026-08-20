use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::num::IntErrorKind;

use chrono::DateTime;
use serde::{Deserialize, Serialize};
use unicode_normalization::UnicodeNormalization;
use uuid::Uuid;

use crate::query::{AssetQuery, QueryParseError, QueryParseErrorKind, Token, error};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum IntegerField {
    Rating,
    Size,
    Width,
    Height,
    Duration,
    Pages,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum InstantField {
    Created,
    Modified,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RatioField {
    Aspect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum UnknownField {
    Size,
    Width,
    Height,
    Aspect,
    Created,
    Modified,
    Duration,
    Pages,
    Orientation,
    Root,
    ColorSpace,
    HasAlpha,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Orientation {
    Landscape,
    Portrait,
    Square,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", tag = "kind", content = "value")]
pub enum NullableBoolean {
    Known(bool),
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RangeBound<T> {
    pub value: T,
    pub inclusive: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RangeConstraint<T> {
    pub lower: Option<RangeBound<T>>,
    pub upper: Option<RangeBound<T>>,
}

impl<T> Default for RangeConstraint<T> {
    fn default() -> Self {
        Self {
            lower: None,
            upper: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Ratio {
    pub numerator: u32,
    pub denominator: u32,
}

impl Ord for Ratio {
    fn cmp(&self, other: &Self) -> Ordering {
        (u64::from(self.numerator) * u64::from(other.denominator))
            .cmp(&(u64::from(other.numerator) * u64::from(self.denominator)))
    }
}

impl PartialOrd for Ratio {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ComparisonOperator {
    Equal,
    Less,
    LessOrEqual,
    Greater,
    GreaterOrEqual,
}

pub(super) fn parse_advanced_filter(
    token: &Token,
    query: &mut AssetQuery,
) -> Result<bool, QueryParseError> {
    let Some((field, value)) = token.value.split_once(':') else {
        return Ok(false);
    };
    match field {
        "rating" => parse_integer_filter(
            IntegerField::Rating,
            None,
            value,
            token,
            query,
            parse_rating,
        )?,
        "size" => parse_integer_filter(
            IntegerField::Size,
            Some(UnknownField::Size),
            value,
            token,
            query,
            parse_size,
        )?,
        "width" => parse_integer_filter(
            IntegerField::Width,
            Some(UnknownField::Width),
            value,
            token,
            query,
            |value, token| parse_positive_integer(value, token, "width"),
        )?,
        "height" => parse_integer_filter(
            IntegerField::Height,
            Some(UnknownField::Height),
            value,
            token,
            query,
            |value, token| parse_positive_integer(value, token, "height"),
        )?,
        "duration" => parse_integer_filter(
            IntegerField::Duration,
            Some(UnknownField::Duration),
            value,
            token,
            query,
            parse_duration,
        )?,
        "pages" => parse_integer_filter(
            IntegerField::Pages,
            Some(UnknownField::Pages),
            value,
            token,
            query,
            |value, token| parse_positive_integer(value, token, "pages"),
        )?,
        "created" => parse_instant_filter(
            InstantField::Created,
            UnknownField::Created,
            value,
            token,
            query,
        )?,
        "modified" => parse_instant_filter(
            InstantField::Modified,
            UnknownField::Modified,
            value,
            token,
            query,
        )?,
        "aspect" => parse_ratio_filter(value, token, query)?,
        "orientation" => parse_orientation_filter(value, token, query)?,
        "root" => parse_root_filter(value, token, query)?,
        "path" => parse_path_filter(value, token, query)?,
        "color-space" => parse_color_space_filter(value, token, query)?,
        "has-note" => parse_has_note_filter(value, token, query)?,
        "has-alpha" => parse_has_alpha_filter(value, token, query)?,
        _ => return Ok(false),
    }
    Ok(true)
}

fn parse_integer_filter(
    field: IntegerField,
    unknown: Option<UnknownField>,
    value: &str,
    token: &Token,
    query: &mut AssetQuery,
    parser: impl Fn(&str, &Token) -> Result<u64, QueryParseError>,
) -> Result<(), QueryParseError> {
    let (operator, value) = split_operator(value, token)?;
    if value == "unknown" {
        if operator != ComparisonOperator::Equal {
            return Err(error(
                QueryParseErrorKind::InvalidOperator,
                token,
                "unknown only supports equality",
            ));
        }
        let Some(unknown) = unknown else {
            return Err(error(
                QueryParseErrorKind::UnsupportedUnknown,
                token,
                "this field cannot be unknown",
            ));
        };
        if query.integer_ranges.contains_key(&field) {
            return Err(conflicting_value(token));
        }
        query.unknown_fields.insert(unknown);
        return Ok(());
    }
    if unknown.is_some_and(|unknown| query.unknown_fields.contains(&unknown)) {
        return Err(conflicting_value(token));
    }
    let parsed = parser(value, token)?;
    apply_range(&mut query.integer_ranges, field, operator, parsed, token)
}

fn parse_instant_filter(
    field: InstantField,
    unknown: UnknownField,
    value: &str,
    token: &Token,
    query: &mut AssetQuery,
) -> Result<(), QueryParseError> {
    let (operator, value) = split_operator(value, token)?;
    if value == "unknown" {
        if operator != ComparisonOperator::Equal {
            return Err(error(
                QueryParseErrorKind::InvalidOperator,
                token,
                "unknown only supports equality",
            ));
        }
        if query.instant_ranges.contains_key(&field) {
            return Err(conflicting_value(token));
        }
        query.unknown_fields.insert(unknown);
        return Ok(());
    }
    if query.unknown_fields.contains(&unknown) {
        return Err(conflicting_value(token));
    }
    let parsed = parse_instant(value, token)?;
    apply_range(&mut query.instant_ranges, field, operator, parsed, token)
}

fn parse_ratio_filter(
    value: &str,
    token: &Token,
    query: &mut AssetQuery,
) -> Result<(), QueryParseError> {
    let (operator, value) = split_operator(value, token)?;
    if value == "unknown" {
        if operator != ComparisonOperator::Equal {
            return Err(error(
                QueryParseErrorKind::InvalidOperator,
                token,
                "unknown only supports equality",
            ));
        }
        if query.ratio_ranges.contains_key(&RatioField::Aspect) {
            return Err(conflicting_value(token));
        }
        query.unknown_fields.insert(UnknownField::Aspect);
        return Ok(());
    }
    if query.unknown_fields.contains(&UnknownField::Aspect) {
        return Err(conflicting_value(token));
    }
    let parsed = parse_ratio(value, token)?;
    apply_range(
        &mut query.ratio_ranges,
        RatioField::Aspect,
        operator,
        parsed,
        token,
    )
}

fn parse_orientation_filter(
    value: &str,
    token: &Token,
    query: &mut AssetQuery,
) -> Result<(), QueryParseError> {
    let value = equality_value(value, token)?;
    for value in split_values(value, token)? {
        if value == "unknown" {
            if !query.orientations.is_empty() {
                return Err(conflicting_value(token));
            }
            query.unknown_fields.insert(UnknownField::Orientation);
            continue;
        }
        if query.unknown_fields.contains(&UnknownField::Orientation) {
            return Err(conflicting_value(token));
        }
        let orientation = match value {
            "landscape" => Orientation::Landscape,
            "portrait" => Orientation::Portrait,
            "square" => Orientation::Square,
            _ => {
                return Err(error(
                    QueryParseErrorKind::InvalidEnum,
                    token,
                    "orientation requires landscape, portrait, square, or unknown",
                ));
            }
        };
        query.orientations.insert(orientation);
    }
    Ok(())
}

fn parse_root_filter(
    value: &str,
    token: &Token,
    query: &mut AssetQuery,
) -> Result<(), QueryParseError> {
    let value = equality_value(value, token)?;
    for value in split_values(value, token)? {
        if value == "unknown" {
            if !query.root_ids.is_empty() {
                return Err(conflicting_value(token));
            }
            query.unknown_fields.insert(UnknownField::Root);
            continue;
        }
        if query.unknown_fields.contains(&UnknownField::Root) {
            return Err(conflicting_value(token));
        }
        let root = Uuid::parse_str(value).map_err(|_| {
            error(
                QueryParseErrorKind::InvalidRootId,
                token,
                "root requires a canonical lowercase UUID",
            )
        })?;
        if root.to_string() != value {
            return Err(error(
                QueryParseErrorKind::InvalidRootId,
                token,
                "root requires a canonical lowercase UUID",
            ));
        }
        query.root_ids.insert(root);
    }
    Ok(())
}

fn parse_path_filter(
    value: &str,
    token: &Token,
    query: &mut AssetQuery,
) -> Result<(), QueryParseError> {
    let value = equality_value(value, token)?;
    if value == "unknown" {
        return Err(error(
            QueryParseErrorKind::UnsupportedUnknown,
            token,
            "path cannot be unknown",
        ));
    }
    let normalized = value.nfc().collect::<String>();
    let invalid_segment = normalized
        .split('/')
        .any(|segment| segment == "." || segment == "..");
    let drive_prefix = normalized
        .as_bytes()
        .get(..2)
        .is_some_and(|prefix| prefix[0].is_ascii_alphabetic() && prefix[1] == b':');
    if normalized.is_empty()
        || normalized.starts_with('/')
        || normalized.contains('\\')
        || drive_prefix
        || invalid_segment
        || normalized.chars().any(char::is_control)
    {
        return Err(error(
            QueryParseErrorKind::InvalidPath,
            token,
            "path requires a non-empty root-relative substring without dot segments",
        ));
    }
    query.path_contains.push(normalized);
    Ok(())
}

fn parse_color_space_filter(
    value: &str,
    token: &Token,
    query: &mut AssetQuery,
) -> Result<(), QueryParseError> {
    let value = equality_value(value, token)?;
    for value in split_values(value, token)? {
        if value == "unknown" {
            if !query.color_spaces.is_empty() {
                return Err(conflicting_value(token));
            }
            query.unknown_fields.insert(UnknownField::ColorSpace);
            continue;
        }
        if query.unknown_fields.contains(&UnknownField::ColorSpace) {
            return Err(conflicting_value(token));
        }
        if value.len() > 64
            || !value.chars().enumerate().all(|(index, character)| {
                character.is_ascii_lowercase()
                    || character.is_ascii_digit()
                    || (index > 0 && matches!(character, '.' | '_' | '-'))
            })
        {
            return Err(error(
                QueryParseErrorKind::InvalidEnum,
                token,
                "color-space requires a normalized lowercase provider value",
            ));
        }
        query.color_spaces.insert(value.to_owned());
    }
    Ok(())
}

fn parse_has_note_filter(
    value: &str,
    token: &Token,
    query: &mut AssetQuery,
) -> Result<(), QueryParseError> {
    let value = equality_value(value, token)?;
    if value == "unknown" {
        return Err(error(
            QueryParseErrorKind::UnsupportedUnknown,
            token,
            "has-note cannot be unknown",
        ));
    }
    let parsed = parse_boolean(value, token)?;
    if query.has_note.is_some_and(|current| current != parsed) {
        return Err(conflicting_value(token));
    }
    query.has_note = Some(parsed);
    Ok(())
}

fn parse_has_alpha_filter(
    value: &str,
    token: &Token,
    query: &mut AssetQuery,
) -> Result<(), QueryParseError> {
    let value = equality_value(value, token)?;
    let parsed = if value == "unknown" {
        query.unknown_fields.insert(UnknownField::HasAlpha);
        NullableBoolean::Unknown
    } else {
        NullableBoolean::Known(parse_boolean(value, token)?)
    };
    if query.has_alpha.is_some_and(|current| current != parsed) {
        return Err(conflicting_value(token));
    }
    query.has_alpha = Some(parsed);
    Ok(())
}

fn parse_boolean(value: &str, token: &Token) -> Result<bool, QueryParseError> {
    match value {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err(error(
            QueryParseErrorKind::InvalidEnum,
            token,
            "boolean field requires true or false",
        )),
    }
}

fn split_values<'a>(value: &'a str, token: &Token) -> Result<Vec<&'a str>, QueryParseError> {
    let values = value.split('|').collect::<Vec<_>>();
    if values.iter().any(|value| value.is_empty()) {
        return Err(error(
            QueryParseErrorKind::InvalidEnum,
            token,
            "OR values cannot be empty",
        ));
    }
    Ok(values)
}

fn equality_value<'a>(value: &'a str, token: &Token) -> Result<&'a str, QueryParseError> {
    let (operator, value) = split_operator(value, token)?;
    if operator != ComparisonOperator::Equal {
        return Err(error(
            QueryParseErrorKind::InvalidOperator,
            token,
            "this field only supports equality",
        ));
    }
    Ok(value)
}

fn split_operator<'a>(
    value: &'a str,
    token: &Token,
) -> Result<(ComparisonOperator, &'a str), QueryParseError> {
    let (operator, remainder) = if let Some(value) = value.strip_prefix(">=") {
        (ComparisonOperator::GreaterOrEqual, value)
    } else if let Some(value) = value.strip_prefix("<=") {
        (ComparisonOperator::LessOrEqual, value)
    } else if let Some(value) = value.strip_prefix('>') {
        (ComparisonOperator::Greater, value)
    } else if let Some(value) = value.strip_prefix('<') {
        (ComparisonOperator::Less, value)
    } else if let Some(value) = value.strip_prefix('=') {
        (ComparisonOperator::Equal, value)
    } else if value.starts_with(['!', '~']) {
        return Err(error(
            QueryParseErrorKind::InvalidOperator,
            token,
            "operator must be =, <, <=, >, or >=",
        ));
    } else {
        (ComparisonOperator::Equal, value)
    };
    if remainder.is_empty() || remainder.starts_with(['=', '<', '>', '!', '~']) {
        return Err(error(
            QueryParseErrorKind::InvalidOperator,
            token,
            "operator must be followed by exactly one value",
        ));
    }
    Ok((operator, remainder))
}

fn apply_range<K, T>(
    ranges: &mut BTreeMap<K, RangeConstraint<T>>,
    field: K,
    operator: ComparisonOperator,
    value: T,
    token: &Token,
) -> Result<(), QueryParseError>
where
    K: Ord,
    T: Copy + Ord,
{
    let range = ranges.entry(field).or_default();
    match operator {
        ComparisonOperator::Equal => {
            merge_lower(range, value, true);
            merge_upper(range, value, true);
        }
        ComparisonOperator::Less => merge_upper(range, value, false),
        ComparisonOperator::LessOrEqual => merge_upper(range, value, true),
        ComparisonOperator::Greater => merge_lower(range, value, false),
        ComparisonOperator::GreaterOrEqual => merge_lower(range, value, true),
    }
    if range_is_empty(range) {
        return Err(error(
            QueryParseErrorKind::ConflictingRange,
            token,
            "range predicates have an empty intersection",
        ));
    }
    Ok(())
}

fn merge_lower<T: Copy + Ord>(range: &mut RangeConstraint<T>, value: T, inclusive: bool) {
    if range.lower.is_none_or(|current| {
        value > current.value || (value == current.value && !inclusive && current.inclusive)
    }) {
        range.lower = Some(RangeBound { value, inclusive });
    }
}

fn merge_upper<T: Copy + Ord>(range: &mut RangeConstraint<T>, value: T, inclusive: bool) {
    if range.upper.is_none_or(|current| {
        value < current.value || (value == current.value && !inclusive && current.inclusive)
    }) {
        range.upper = Some(RangeBound { value, inclusive });
    }
}

fn range_is_empty<T: Ord>(range: &RangeConstraint<T>) -> bool {
    match (&range.lower, &range.upper) {
        (Some(lower), Some(upper)) => {
            lower.value > upper.value
                || (lower.value == upper.value && (!lower.inclusive || !upper.inclusive))
        }
        _ => false,
    }
}

fn parse_rating(value: &str, token: &Token) -> Result<u64, QueryParseError> {
    let value = parse_decimal(value, token)?;
    if value > 5 {
        return Err(error(
            QueryParseErrorKind::InvalidInteger,
            token,
            "rating must be between 0 and 5",
        ));
    }
    Ok(value)
}

fn parse_positive_integer(value: &str, token: &Token, field: &str) -> Result<u64, QueryParseError> {
    let value = parse_decimal(value, token)?;
    if value == 0 {
        return Err(error(
            QueryParseErrorKind::InvalidInteger,
            token,
            &format!("{field} must be greater than zero"),
        ));
    }
    Ok(value)
}

fn parse_size(value: &str, token: &Token) -> Result<u64, QueryParseError> {
    parse_unit_integer(
        value,
        token,
        &[
            ("TiB", 1024_u64.pow(4)),
            ("GiB", 1024_u64.pow(3)),
            ("MiB", 1024_u64.pow(2)),
            ("KiB", 1024),
            ("B", 1),
        ],
    )
}

fn parse_duration(value: &str, token: &Token) -> Result<u64, QueryParseError> {
    parse_unit_integer(
        value,
        token,
        &[("min", 60_000), ("ms", 1), ("s", 1_000), ("h", 3_600_000)],
    )
}

fn parse_unit_integer(
    value: &str,
    token: &Token,
    units: &[(&str, u64)],
) -> Result<u64, QueryParseError> {
    let (number, multiplier) = units
        .iter()
        .find_map(|(unit, multiplier)| value.strip_suffix(unit).map(|number| (number, *multiplier)))
        .unwrap_or((value, 1));
    if number
        .chars()
        .any(|character| character.is_ascii_alphabetic())
    {
        return Err(error(
            QueryParseErrorKind::InvalidUnit,
            token,
            "unit suffix is not supported or has the wrong case",
        ));
    }
    let number = parse_decimal(number, token)?;
    number.checked_mul(multiplier).ok_or_else(|| {
        error(
            QueryParseErrorKind::NumericOverflow,
            token,
            "numeric value exceeds the supported integer range",
        )
    })
}

fn parse_decimal(value: &str, token: &Token) -> Result<u64, QueryParseError> {
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(error(
            QueryParseErrorKind::InvalidInteger,
            token,
            "value must be an unsigned decimal integer",
        ));
    }
    value.parse::<u64>().map_err(|parse_error| {
        let kind = if matches!(parse_error.kind(), IntErrorKind::PosOverflow) {
            QueryParseErrorKind::NumericOverflow
        } else {
            QueryParseErrorKind::InvalidInteger
        };
        error(kind, token, "integer could not be represented")
    })
}

fn parse_ratio(value: &str, token: &Token) -> Result<Ratio, QueryParseError> {
    let Some((numerator, denominator)) = value.split_once('/') else {
        return Err(invalid_ratio(token));
    };
    if denominator.contains('/') {
        return Err(invalid_ratio(token));
    }
    let numerator = numerator.parse::<u32>().map_err(|_| invalid_ratio(token))?;
    let denominator = denominator
        .parse::<u32>()
        .map_err(|_| invalid_ratio(token))?;
    if !(1..=1_000_000).contains(&numerator) || !(1..=1_000_000).contains(&denominator) {
        return Err(invalid_ratio(token));
    }
    let divisor = greatest_common_divisor(numerator, denominator);
    Ok(Ratio {
        numerator: numerator / divisor,
        denominator: denominator / divisor,
    })
}

fn invalid_ratio(token: &Token) -> QueryParseError {
    error(
        QueryParseErrorKind::InvalidRatio,
        token,
        "aspect requires a positive fraction with components at most 1000000",
    )
}

fn greatest_common_divisor(mut left: u32, mut right: u32) -> u32 {
    while right != 0 {
        (left, right) = (right, left % right);
    }
    left
}

fn parse_instant(value: &str, token: &Token) -> Result<i64, QueryParseError> {
    let instant = DateTime::parse_from_rfc3339(value).map_err(|_| {
        error(
            QueryParseErrorKind::InvalidDate,
            token,
            "date must be RFC 3339 with an explicit offset",
        )
    })?;
    let total_nanoseconds = i128::from(instant.timestamp()) * 1_000_000_000
        + i128::from(instant.timestamp_subsec_nanos());
    i64::try_from(total_nanoseconds / 1_000_000).map_err(|_| {
        error(
            QueryParseErrorKind::NumericOverflow,
            token,
            "date is outside the supported Unix millisecond range",
        )
    })
}

fn conflicting_value(token: &Token) -> QueryParseError {
    error(
        QueryParseErrorKind::ConflictingValue,
        token,
        "field cannot require conflicting known, unknown, or boolean values",
    )
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;
    use crate::{AssetQuery, QueryParseErrorKind, parse_query};

    #[test]
    fn parses_and_normalizes_integer_time_and_ratio_ranges() {
        let query = parse_query(
            "rating:>=4 size:<2MiB width:>=1920 width:<4096 height:1080 \
             duration:>=30s pages:>=2 aspect:>=32/18 \
             modified:>=2026-08-19T00:00:00+08:00",
        )
        .expect("advanced query");
        assert_eq!(
            query.integer_ranges[&IntegerField::Size].upper,
            Some(RangeBound {
                value: 2 * 1024 * 1024,
                inclusive: false,
            })
        );
        assert_eq!(
            query.integer_ranges[&IntegerField::Width],
            RangeConstraint {
                lower: Some(RangeBound {
                    value: 1920,
                    inclusive: true,
                }),
                upper: Some(RangeBound {
                    value: 4096,
                    inclusive: false,
                }),
            }
        );
        assert_eq!(
            query.ratio_ranges[&RatioField::Aspect].lower,
            Some(RangeBound {
                value: Ratio {
                    numerator: 16,
                    denominator: 9,
                },
                inclusive: true,
            })
        );
        assert_eq!(
            query.instant_ranges[&InstantField::Modified].lower,
            Some(RangeBound {
                value: 1_787_068_800_000,
                inclusive: true,
            })
        );
    }

    #[test]
    fn parses_enum_boolean_root_path_and_unknown_predicates() {
        let root_a = "0198e8c0-7451-7af1-8bca-0123456789ab";
        let root_b = "0198e8c0-7451-7af1-8bca-abcdef012345";
        let query = parse_query(&format!(
            r#"orientation:landscape|square root:{root_a}\|{root_b} path:"Brand Assets/é" color-space:srgb\|display-p3 has-note:true has-alpha:unknown size:unknown"#,
        ))
        .expect("categorical query");
        assert_eq!(query.root_ids.len(), 2);
        assert_eq!(query.path_contains, ["Brand Assets/é"]);
        assert_eq!(query.has_note, Some(true));
        assert_eq!(query.has_alpha, Some(NullableBoolean::Unknown));
        assert!(query.unknown_fields.contains(&UnknownField::Size));
        assert_eq!(
            query.orientations,
            BTreeSet::from([Orientation::Landscape, Orientation::Square])
        );
    }

    #[test]
    fn preserves_version_one_ast_while_adding_advanced_fields() {
        let query = parse_query(
            "ui/* any:(color/blue|color/red) -draft type:image|video ext:png favorite:true",
        )
        .expect("version one query");
        assert!(query.integer_ranges.is_empty());
        assert!(query.instant_ranges.is_empty());
        assert!(query.ratio_ranges.is_empty());
        assert!(query.unknown_fields.is_empty());
        assert_eq!(query.favorite, Some(true));
    }

    #[test]
    fn rejects_every_new_stable_error_family() {
        let cases = [
            ("size:!10", QueryParseErrorKind::InvalidOperator),
            ("width:1.5", QueryParseErrorKind::InvalidInteger),
            ("size:10MB", QueryParseErrorKind::InvalidUnit),
            (
                "size:18446744073709551616",
                QueryParseErrorKind::NumericOverflow,
            ),
            ("aspect:16/0", QueryParseErrorKind::InvalidRatio),
            ("modified:2026-08-19", QueryParseErrorKind::InvalidDate),
            ("orientation:wide", QueryParseErrorKind::InvalidEnum),
            ("root:NOT-A-UUID", QueryParseErrorKind::InvalidRootId),
            (r#"path:"../escape""#, QueryParseErrorKind::InvalidPath),
            (r#"path:"Brand\\escape""#, QueryParseErrorKind::InvalidPath),
            ("rating:unknown", QueryParseErrorKind::UnsupportedUnknown),
            (
                "width:>=10 width:<10",
                QueryParseErrorKind::ConflictingRange,
            ),
            (
                "has-note:true has-note:false",
                QueryParseErrorKind::ConflictingValue,
            ),
        ];
        for (expression, kind) in cases {
            let parsed = parse_query(expression).expect_err(expression);
            assert_eq!(parsed.kind, kind, "{expression}");
            assert_eq!(
                parsed.offset,
                expression.rfind(' ').map_or(0, |offset| offset + 1)
            );
        }
    }

    #[test]
    fn rejects_unknown_mixed_with_known_and_integer_overflow_after_units() {
        assert_eq!(
            parse_query("width:unknown width:>=1")
                .expect_err("known and unknown")
                .kind,
            QueryParseErrorKind::ConflictingValue
        );
        assert_eq!(
            parse_query("size:18446744073709551615TiB")
                .expect_err("unit overflow")
                .kind,
            QueryParseErrorKind::NumericOverflow
        );
    }

    #[test]
    fn advanced_errors_keep_zero_based_utf8_byte_offsets() {
        let expression = "标签 size:!10";
        let parsed = parse_query(expression).expect_err("invalid operator");
        assert_eq!(parsed.kind, QueryParseErrorKind::InvalidOperator);
        assert_eq!(parsed.offset, "标签 ".len());
    }

    #[test]
    fn truncates_fractional_instants_toward_zero() {
        let positive = parse_query("modified:1970-01-01T00:00:00.0009Z").expect("positive instant");
        let negative = parse_query("modified:1969-12-31T23:59:59.9991Z").expect("negative instant");
        assert_eq!(
            positive.instant_ranges[&InstantField::Modified].lower,
            Some(RangeBound {
                value: 0,
                inclusive: true,
            })
        );
        assert_eq!(
            negative.instant_ranges[&InstantField::Modified].lower,
            Some(RangeBound {
                value: 0,
                inclusive: true,
            })
        );
    }

    #[test]
    fn serialized_ast_contains_typed_values_without_floats() {
        let query = parse_query("size:>=10MiB aspect:16/9 modified:2026-01-01T00:00:00Z")
            .expect("typed query");
        let value = serde_json::to_value(AssetQuery { ..query }).expect("query JSON");
        let text = serde_json::to_string(&value).expect("query text");
        assert!(text.contains("10485760"));
        assert!(text.contains("\"numerator\":16"));
        assert!(!text.contains("1.777"));
    }
}

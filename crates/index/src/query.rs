use std::collections::{BTreeMap, BTreeSet};

use asset_core::AssetKind;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::advanced::{
    InstantField, IntegerField, NullableBoolean, Orientation, RangeConstraint, Ratio, RatioField,
    UnknownField, parse_advanced_filter,
};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetQuery {
    pub all_tags: BTreeSet<String>,
    pub any_tag_groups: Vec<BTreeSet<String>>,
    pub excluded_tags: BTreeSet<String>,
    pub kinds: BTreeSet<AssetKind>,
    pub extensions: BTreeSet<String>,
    pub favorite: Option<bool>,
    pub integer_ranges: BTreeMap<IntegerField, RangeConstraint<u64>>,
    pub instant_ranges: BTreeMap<InstantField, RangeConstraint<i64>>,
    pub ratio_ranges: BTreeMap<RatioField, RangeConstraint<Ratio>>,
    pub unknown_fields: BTreeSet<UnknownField>,
    pub orientations: BTreeSet<Orientation>,
    pub root_ids: BTreeSet<uuid::Uuid>,
    pub path_contains: Vec<String>,
    pub color_spaces: BTreeSet<String>,
    pub has_note: Option<bool>,
    pub has_alpha: Option<NullableBoolean>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum QueryParseErrorKind {
    UnclosedQuote,
    TrailingEscape,
    EmptyTag,
    TagTooLong,
    InvalidWildcard,
    InvalidOrGroup,
    UnknownFilter,
    InvalidType,
    InvalidExtension,
    InvalidFavorite,
    ConflictingFavorite,
    InvalidOperator,
    InvalidInteger,
    InvalidUnit,
    NumericOverflow,
    InvalidRatio,
    InvalidDate,
    InvalidEnum,
    InvalidRootId,
    InvalidPath,
    UnsupportedUnknown,
    ConflictingRange,
    ConflictingValue,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Error)]
#[serde(rename_all = "camelCase")]
#[error("{message} at byte offset {offset}")]
pub struct QueryParseError {
    pub kind: QueryParseErrorKind,
    pub offset: usize,
    pub token: Option<String>,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Token {
    pub(super) value: String,
    pub(super) offset: usize,
}

/// Parses a user-facing query expression into a validated index query.
///
/// Whitespace-separated tag terms are combined with AND. Prefix a tag with `-`
/// to exclude it, use `any:(tag-a|tag-b)` for an OR group, and use `type:`,
/// `ext:`, or `favorite:` for field filters. Double quotes preserve whitespace.
///
/// # Errors
///
/// Returns a structured error with a byte offset when the expression contains
/// malformed quoting, filters, tag wildcards, or OR groups.
pub fn parse_query(expression: &str) -> Result<AssetQuery, QueryParseError> {
    let mut query = AssetQuery::default();
    for token in tokenize(expression)? {
        parse_token(&token, &mut query)?;
    }
    Ok(query)
}

fn parse_token(token: &Token, query: &mut AssetQuery) -> Result<(), QueryParseError> {
    if let Some(excluded) = token.value.strip_prefix('-') {
        let explicit_tag = excluded.strip_prefix("tag:");
        let tag = explicit_tag.unwrap_or(excluded);
        if explicit_tag.is_none() && tag.contains(':') {
            return Err(error(
                QueryParseErrorKind::UnknownFilter,
                token,
                "only tag terms can use the exclusion prefix",
            ));
        }
        validate_tag(tag, token)?;
        query.excluded_tags.insert(tag.to_owned());
        return Ok(());
    }

    if let Some(value) = token.value.strip_prefix("any:") {
        query.any_tag_groups.push(parse_or_group(value, token)?);
    } else if let Some(value) = token.value.strip_prefix("type:") {
        parse_kinds(value, token, &mut query.kinds)?;
    } else if let Some(value) = token.value.strip_prefix("ext:") {
        parse_extensions(value, token, &mut query.extensions)?;
    } else if let Some(value) = token.value.strip_prefix("favorite:") {
        let favorite = parse_favorite(value, token)?;
        if query.favorite.is_some_and(|current| current != favorite) {
            return Err(error(
                QueryParseErrorKind::ConflictingFavorite,
                token,
                "favorite filter cannot require both true and false",
            ));
        }
        query.favorite = Some(favorite);
    } else if parse_advanced_filter(token, query)? {
    } else {
        let explicit_tag = token.value.strip_prefix("tag:");
        let tag = explicit_tag.unwrap_or(&token.value);
        if explicit_tag.is_none() && tag.contains(':') {
            return Err(error(
                QueryParseErrorKind::UnknownFilter,
                token,
                "unknown filter; use tag: to search for a tag containing a colon",
            ));
        }
        if tag.contains('|') {
            return Err(error(
                QueryParseErrorKind::InvalidOrGroup,
                token,
                "tag OR groups must use any:(tag-a|tag-b)",
            ));
        }
        validate_tag(tag, token)?;
        query.all_tags.insert(tag.to_owned());
    }
    Ok(())
}

fn parse_or_group(value: &str, token: &Token) -> Result<BTreeSet<String>, QueryParseError> {
    let Some(value) = value
        .strip_prefix('(')
        .and_then(|value| value.strip_suffix(')'))
    else {
        return Err(error(
            QueryParseErrorKind::InvalidOrGroup,
            token,
            "OR group must use any:(tag-a|tag-b)",
        ));
    };
    let tags = value.split('|').collect::<Vec<_>>();
    if tags.len() < 2 {
        return Err(error(
            QueryParseErrorKind::InvalidOrGroup,
            token,
            "OR group must contain at least two tags",
        ));
    }
    let mut group = BTreeSet::new();
    for tag in tags {
        validate_tag(tag, token)?;
        group.insert(tag.to_owned());
    }
    if group.len() < 2 {
        return Err(error(
            QueryParseErrorKind::InvalidOrGroup,
            token,
            "OR group must contain at least two distinct tags",
        ));
    }
    Ok(group)
}

fn parse_kinds(
    value: &str,
    token: &Token,
    kinds: &mut BTreeSet<AssetKind>,
) -> Result<(), QueryParseError> {
    if value.is_empty() {
        return Err(error(
            QueryParseErrorKind::InvalidType,
            token,
            "type filter requires image, video, audio, pdf, or other",
        ));
    }
    for value in value.split('|') {
        let kind = match value.to_ascii_lowercase().as_str() {
            "image" => AssetKind::Image,
            "video" => AssetKind::Video,
            "audio" => AssetKind::Audio,
            "pdf" => AssetKind::Pdf,
            "other" => AssetKind::Other,
            _ => {
                return Err(error(
                    QueryParseErrorKind::InvalidType,
                    token,
                    "type filter requires image, video, audio, pdf, or other",
                ));
            }
        };
        kinds.insert(kind);
    }
    Ok(())
}

fn parse_extensions(
    value: &str,
    token: &Token,
    extensions: &mut BTreeSet<String>,
) -> Result<(), QueryParseError> {
    if value.is_empty() {
        return Err(error(
            QueryParseErrorKind::InvalidExtension,
            token,
            "extension filter requires at least one extension",
        ));
    }
    for value in value.split('|') {
        let value = value.strip_prefix('.').unwrap_or(value);
        if value.is_empty()
            || value.len() > 32
            || !value
                .chars()
                .all(|character| character.is_ascii_alphanumeric())
        {
            return Err(error(
                QueryParseErrorKind::InvalidExtension,
                token,
                "extensions must contain 1 to 32 ASCII letters or digits",
            ));
        }
        extensions.insert(value.to_ascii_lowercase());
    }
    Ok(())
}

fn parse_favorite(value: &str, token: &Token) -> Result<bool, QueryParseError> {
    match value.to_ascii_lowercase().as_str() {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err(error(
            QueryParseErrorKind::InvalidFavorite,
            token,
            "favorite filter requires true or false",
        )),
    }
}

fn validate_tag(tag: &str, token: &Token) -> Result<(), QueryParseError> {
    if tag.trim().is_empty() {
        return Err(error(
            QueryParseErrorKind::EmptyTag,
            token,
            "tag cannot be empty",
        ));
    }
    if tag.chars().count() > 128 {
        return Err(error(
            QueryParseErrorKind::TagTooLong,
            token,
            "tag cannot exceed 128 characters",
        ));
    }
    if tag.contains('*')
        && (tag.matches('*').count() != 1
            || !tag.ends_with("/*")
            || tag.trim_end_matches("/*").is_empty())
    {
        return Err(error(
            QueryParseErrorKind::InvalidWildcard,
            token,
            "tag wildcard is only valid as a non-empty namespace followed by /*",
        ));
    }
    Ok(())
}

fn tokenize(expression: &str) -> Result<Vec<Token>, QueryParseError> {
    let mut tokens = Vec::new();
    let mut value = String::new();
    let mut offset = None;
    let mut quoted = false;
    let mut escaped = false;

    for (index, character) in expression.char_indices() {
        if escaped {
            value.push(character);
            escaped = false;
            continue;
        }
        if character == '\\' {
            offset.get_or_insert(index);
            escaped = true;
        } else if character == '"' {
            offset.get_or_insert(index);
            quoted = !quoted;
        } else if character.is_whitespace() && !quoted {
            finish_token(&mut tokens, &mut value, &mut offset);
        } else {
            offset.get_or_insert(index);
            value.push(character);
        }
    }

    if escaped {
        return Err(QueryParseError {
            kind: QueryParseErrorKind::TrailingEscape,
            offset: expression.len().saturating_sub(1),
            token: None,
            message: "query cannot end with an escape character".into(),
        });
    }
    if quoted {
        return Err(QueryParseError {
            kind: QueryParseErrorKind::UnclosedQuote,
            offset: offset.unwrap_or(expression.len()),
            token: None,
            message: "query contains an unclosed double quote".into(),
        });
    }
    finish_token(&mut tokens, &mut value, &mut offset);
    Ok(tokens)
}

fn finish_token(tokens: &mut Vec<Token>, value: &mut String, offset: &mut Option<usize>) {
    if let Some(token_offset) = offset.take() {
        tokens.push(Token {
            value: std::mem::take(value),
            offset: token_offset,
        });
    }
}

pub(super) fn error(kind: QueryParseErrorKind, token: &Token, message: &str) -> QueryParseError {
    QueryParseError {
        kind,
        offset: token.offset,
        token: Some(token.value.clone()),
        message: message.into(),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use asset_core::AssetKind;

    use super::{AssetQuery, QueryParseErrorKind, parse_query};

    #[test]
    fn empty_expression_matches_the_default_query() {
        assert_eq!(
            parse_query(" \t\n").expect("empty query"),
            AssetQuery::default()
        );
    }

    #[test]
    fn parses_tags_or_groups_and_normalized_field_filters() {
        let parsed = parse_query(
            r#"ui/* "visual style/minimal" any:(color/blue|color/red) -state/draft type:IMAGE|video ext:.PNG|Jpeg favorite:true"#,
        )
        .expect("parse query");

        assert_eq!(
            parsed,
            AssetQuery {
                all_tags: BTreeSet::from(["ui/*".into(), "visual style/minimal".into()]),
                any_tag_groups: vec![BTreeSet::from(["color/blue".into(), "color/red".into(),])],
                excluded_tags: BTreeSet::from(["state/draft".into()]),
                kinds: BTreeSet::from([AssetKind::Image, AssetKind::Video]),
                extensions: BTreeSet::from(["jpeg".into(), "png".into()]),
                favorite: Some(true),
                ..AssetQuery::default()
            }
        );
    }

    #[test]
    fn supports_multiple_or_groups_and_explicit_colon_tags() {
        let parsed = parse_query(
            r"any:(color/blue|color/red) any:(usage/hero|usage/card) tag:source:camera -tag:state:old",
        )
        .expect("parse query");

        assert_eq!(parsed.any_tag_groups.len(), 2);
        assert!(parsed.all_tags.contains("source:camera"));
        assert!(parsed.excluded_tags.contains("state:old"));
    }

    #[test]
    fn reports_actionable_syntax_errors_instead_of_an_empty_query() {
        let cases = [
            ("any:(only)", QueryParseErrorKind::InvalidOrGroup),
            ("ui*", QueryParseErrorKind::InvalidWildcard),
            ("kind:image", QueryParseErrorKind::UnknownFilter),
            ("type:document", QueryParseErrorKind::InvalidType),
            ("ext:png.exe", QueryParseErrorKind::InvalidExtension),
            ("favorite:yes", QueryParseErrorKind::InvalidFavorite),
            (
                "favorite:true favorite:false",
                QueryParseErrorKind::ConflictingFavorite,
            ),
            ("tag:", QueryParseErrorKind::EmptyTag),
            (r#""unclosed"#, QueryParseErrorKind::UnclosedQuote),
            ("tag\\", QueryParseErrorKind::TrailingEscape),
        ];

        for (expression, expected) in cases {
            let error = parse_query(expression).expect_err(expression);
            assert_eq!(error.kind, expected, "expression: {expression}");
            assert!(!error.message.is_empty());
        }

        let too_long = format!("tag:{}", "x".repeat(129));
        assert_eq!(
            parse_query(&too_long).expect_err("reject long tag").kind,
            QueryParseErrorKind::TagTooLong
        );
    }
}

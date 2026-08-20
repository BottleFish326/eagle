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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum QueryTagNodeKind {
    All,
    Explicit,
    Excluded,
    AnyMember,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueryByteSpan {
    pub start: usize,
    pub end: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueryTagNode {
    pub kind: QueryTagNodeKind,
    pub value: String,
    pub span: QueryByteSpan,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TagRenameMode {
    Exact,
    NamespaceWildcard,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueryTagRewrite {
    pub expression: String,
    pub node_count: usize,
    pub nodes: Vec<QueryTagNode>,
}

#[derive(Debug, Error)]
pub enum QueryTagRewriteError {
    #[error("query cannot be rewritten because it is invalid: {0}")]
    InvalidQuery(#[source] QueryParseError),
    #[error("tag rename input is not representable by the query grammar")]
    InvalidTag,
    #[error("rewritten query is invalid: {0}")]
    RewriteInvalid(#[source] QueryParseError),
    #[error("rewritten query changed semantics outside the selected tag")]
    EquivalenceFailed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DecodedUnit {
    value_start: usize,
    value_end: usize,
    source_start: usize,
    source_end: usize,
    quoted: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Token {
    pub(super) value: String,
    pub(super) offset: usize,
    end: usize,
    units: Vec<DecodedUnit>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct LexedTagNode {
    node: QueryTagNode,
    inside_quotes: bool,
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
    parse_query_with_tag_nodes_internal(expression).map(|(query, _)| query)
}

/// Parses a query and returns every syntactic Tag node with an exact UTF-8
/// byte span into the original expression.
///
/// The span covers the complete token for top-level Tag predicates and the
/// individual member for `any:(...)`. Non-Tag fields and free text inside
/// quoted field values are never reported.
///
/// # Errors
///
/// Returns the same structured parse error as [`parse_query`].
pub fn parse_query_with_tag_nodes(
    expression: &str,
) -> Result<(AssetQuery, Vec<QueryTagNode>), QueryParseError> {
    parse_query_with_tag_nodes_internal(expression)
        .map(|(query, nodes)| (query, nodes.into_iter().map(|node| node.node).collect()))
}

fn parse_query_with_tag_nodes_internal(
    expression: &str,
) -> Result<(AssetQuery, Vec<LexedTagNode>), QueryParseError> {
    let mut query = AssetQuery::default();
    let mut nodes = Vec::new();
    for token in tokenize(expression)? {
        parse_token(&token, &mut query)?;
        nodes.extend(tag_nodes_for_token(expression, &token));
    }
    Ok((query, nodes))
}

/// Rewrites every selected Tag node from the end of the expression toward the
/// beginning, then re-parses and proves that no other query semantics changed.
///
/// Exact renames deliberately ignore namespace wildcards. Wildcard nodes can
/// only be changed by `namespace-wildcard` with both values ending in `/*`.
///
/// # Errors
///
/// Returns a stable error when the source query or Tag input is invalid, the
/// replacement cannot be parsed, or the AST equivalence audit fails.
pub fn rewrite_query_tag(
    expression: &str,
    old_tag: &str,
    new_tag: &str,
    mode: TagRenameMode,
) -> Result<QueryTagRewrite, QueryTagRewriteError> {
    validate_rename_tags(old_tag, new_tag, mode)?;
    let (before, nodes) = parse_query_with_tag_nodes_internal(expression)
        .map_err(QueryTagRewriteError::InvalidQuery)?;
    let selected = nodes
        .into_iter()
        .filter(|node| match mode {
            TagRenameMode::Exact => node.node.value == old_tag && !node.node.value.ends_with("/*"),
            TagRenameMode::NamespaceWildcard => node.node.value == old_tag,
        })
        .collect::<Vec<_>>();
    if selected.is_empty() {
        return Ok(QueryTagRewrite {
            expression: expression.to_owned(),
            node_count: 0,
            nodes: Vec::new(),
        });
    }

    let mut rewritten = expression.to_owned();
    for node in selected.iter().rev() {
        let replacement = replacement_for_node(node, new_tag);
        rewritten.replace_range(node.node.span.start..node.node.span.end, &replacement);
    }
    let after = parse_query(&rewritten).map_err(QueryTagRewriteError::RewriteInvalid)?;
    let mut expected = before;
    replace_query_tag_value(&mut expected, old_tag, new_tag);
    if after != expected {
        return Err(QueryTagRewriteError::EquivalenceFailed);
    }
    Ok(QueryTagRewrite {
        expression: rewritten,
        node_count: selected.len(),
        nodes: selected.into_iter().map(|node| node.node).collect(),
    })
}

fn tag_nodes_for_token(expression: &str, token: &Token) -> Vec<LexedTagNode> {
    if let Some(excluded) = token.value.strip_prefix('-') {
        let value = excluded.strip_prefix("tag:").unwrap_or(excluded);
        return vec![top_level_tag_node(token, QueryTagNodeKind::Excluded, value)];
    }
    if let Some(group) = token
        .value
        .strip_prefix("any:(")
        .and_then(|value| value.strip_suffix(')'))
    {
        let mut cursor = "any:(".len();
        return group
            .split('|')
            .map(|value| {
                let start = cursor;
                let end = start + value.len();
                cursor = end + 1;
                any_member_tag_node(expression, token, value, start, end)
            })
            .collect();
    }
    if token.value.starts_with("type:")
        || token.value.starts_with("ext:")
        || token.value.starts_with("favorite:")
        || is_advanced_field(&token.value)
    {
        return Vec::new();
    }
    if let Some(value) = token.value.strip_prefix("tag:") {
        return vec![top_level_tag_node(token, QueryTagNodeKind::Explicit, value)];
    }
    vec![top_level_tag_node(
        token,
        QueryTagNodeKind::All,
        &token.value,
    )]
}

fn is_advanced_field(value: &str) -> bool {
    value.split_once(':').is_some_and(|(field, _)| {
        matches!(
            field,
            "rating"
                | "size"
                | "width"
                | "height"
                | "duration"
                | "pages"
                | "created"
                | "modified"
                | "aspect"
                | "orientation"
                | "root"
                | "path"
                | "color-space"
                | "has-note"
                | "has-alpha"
        )
    })
}

fn top_level_tag_node(token: &Token, kind: QueryTagNodeKind, value: &str) -> LexedTagNode {
    LexedTagNode {
        node: QueryTagNode {
            kind,
            value: value.to_owned(),
            span: QueryByteSpan {
                start: token.offset,
                end: token.end,
            },
        },
        inside_quotes: false,
    }
}

fn any_member_tag_node(
    expression: &str,
    token: &Token,
    value: &str,
    value_start: usize,
    value_end: usize,
) -> LexedTagNode {
    let units = token
        .units
        .iter()
        .filter(|unit| unit.value_start >= value_start && unit.value_end <= value_end)
        .collect::<Vec<_>>();
    let first = units
        .first()
        .expect("validated OR member has a source unit");
    let last = units.last().expect("validated OR member has a source unit");
    let mut source_start = first.source_start;
    let mut source_end = last.source_end;
    let bytes = expression.as_bytes();
    let locally_quoted = source_start > token.offset
        && source_end < token.end
        && bytes.get(source_start - 1) == Some(&b'"')
        && bytes.get(source_end) == Some(&b'"');
    if locally_quoted {
        source_start -= 1;
        source_end += 1;
    }
    LexedTagNode {
        node: QueryTagNode {
            kind: QueryTagNodeKind::AnyMember,
            value: value.to_owned(),
            span: QueryByteSpan {
                start: source_start,
                end: source_end,
            },
        },
        inside_quotes: !locally_quoted && units.iter().all(|unit| unit.quoted),
    }
}

fn validate_rename_tags(
    old_tag: &str,
    new_tag: &str,
    mode: TagRenameMode,
) -> Result<(), QueryTagRewriteError> {
    let namespace_mode_valid = old_tag.ends_with("/*")
        && new_tag.ends_with("/*")
        && old_tag.matches('*').count() == 1
        && new_tag.matches('*').count() == 1;
    match mode {
        TagRenameMode::Exact if old_tag.contains('*') || new_tag.contains('*') => {
            return Err(QueryTagRewriteError::InvalidTag);
        }
        TagRenameMode::NamespaceWildcard if !namespace_mode_valid => {
            return Err(QueryTagRewriteError::InvalidTag);
        }
        _ => {}
    }
    for tag in [old_tag, new_tag] {
        if tag.trim().is_empty()
            || tag.chars().count() > 128
            || tag.contains('|')
            || tag.contains('\n')
            || tag.contains('\r')
        {
            return Err(QueryTagRewriteError::InvalidTag);
        }
        let encoded = encode_tag_atom(tag);
        let parsed =
            parse_query(&format!("tag:{encoded}")).map_err(|_| QueryTagRewriteError::InvalidTag)?;
        if parsed.all_tags != BTreeSet::from([tag.to_owned()]) {
            return Err(QueryTagRewriteError::InvalidTag);
        }
    }
    Ok(())
}

fn replacement_for_node(node: &LexedTagNode, new_tag: &str) -> String {
    if node.inside_quotes {
        return escape_quoted_content(new_tag);
    }
    let atom = encode_tag_atom(new_tag);
    match node.node.kind {
        QueryTagNodeKind::All if new_tag.contains(':') => format!("tag:{atom}"),
        QueryTagNodeKind::All | QueryTagNodeKind::AnyMember => atom,
        QueryTagNodeKind::Explicit => format!("tag:{atom}"),
        QueryTagNodeKind::Excluded => format!("-tag:{atom}"),
    }
}

fn encode_tag_atom(tag: &str) -> String {
    if tag
        .chars()
        .any(|character| character.is_whitespace() || matches!(character, '"' | '\\'))
    {
        format!("\"{}\"", escape_quoted_content(tag))
    } else {
        tag.to_owned()
    }
}

fn escape_quoted_content(tag: &str) -> String {
    tag.replace('\\', "\\\\").replace('"', "\\\"")
}

fn replace_query_tag_value(query: &mut AssetQuery, old_tag: &str, new_tag: &str) {
    replace_tag_in_set(&mut query.all_tags, old_tag, new_tag);
    replace_tag_in_set(&mut query.excluded_tags, old_tag, new_tag);
    for group in &mut query.any_tag_groups {
        replace_tag_in_set(group, old_tag, new_tag);
    }
}

fn replace_tag_in_set(tags: &mut BTreeSet<String>, old_tag: &str, new_tag: &str) {
    if tags.remove(old_tag) {
        tags.insert(new_tag.to_owned());
    }
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
    let mut end = 0;
    let mut units = Vec::new();
    let mut quoted = false;
    let mut escaped = false;
    let mut escape_start = 0;

    for (index, character) in expression.char_indices() {
        if escaped {
            let value_start = value.len();
            value.push(character);
            units.push(DecodedUnit {
                value_start,
                value_end: value.len(),
                source_start: escape_start,
                source_end: index + character.len_utf8(),
                quoted,
            });
            end = index + character.len_utf8();
            escaped = false;
            continue;
        }
        if character == '\\' {
            offset.get_or_insert(index);
            escape_start = index;
            end = index + character.len_utf8();
            escaped = true;
        } else if character == '"' {
            offset.get_or_insert(index);
            end = index + character.len_utf8();
            quoted = !quoted;
        } else if character.is_whitespace() && !quoted {
            finish_token(&mut tokens, &mut value, &mut offset, &mut end, &mut units);
        } else {
            offset.get_or_insert(index);
            let value_start = value.len();
            value.push(character);
            units.push(DecodedUnit {
                value_start,
                value_end: value.len(),
                source_start: index,
                source_end: index + character.len_utf8(),
                quoted,
            });
            end = index + character.len_utf8();
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
    finish_token(&mut tokens, &mut value, &mut offset, &mut end, &mut units);
    Ok(tokens)
}

fn finish_token(
    tokens: &mut Vec<Token>,
    value: &mut String,
    offset: &mut Option<usize>,
    end: &mut usize,
    units: &mut Vec<DecodedUnit>,
) {
    if let Some(token_offset) = offset.take() {
        tokens.push(Token {
            value: std::mem::take(value),
            offset: token_offset,
            end: *end,
            units: std::mem::take(units),
        });
        *end = 0;
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

    use super::{
        AssetQuery, QueryParseErrorKind, QueryTagNodeKind, QueryTagRewriteError, TagRenameMode,
        parse_query, parse_query_with_tag_nodes, rewrite_query_tag,
    };

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

    #[test]
    fn reports_exact_tag_node_spans_without_treating_fields_or_prefixes_as_tags() {
        let expression = r#"ui/icon tag:ui/icon -ui/icon any:(ui/icon|ui/icons|"visual style") path:"ui/icon" color-space:ui-icon ui/*"#;
        let (_, nodes) = parse_query_with_tag_nodes(expression).expect("parse tag nodes");
        let exact = nodes
            .iter()
            .filter(|node| node.value == "ui/icon")
            .collect::<Vec<_>>();

        assert_eq!(exact.len(), 4);
        assert_eq!(
            exact.iter().map(|node| node.kind).collect::<Vec<_>>(),
            [
                QueryTagNodeKind::All,
                QueryTagNodeKind::Explicit,
                QueryTagNodeKind::Excluded,
                QueryTagNodeKind::AnyMember,
            ]
        );
        assert_eq!(
            exact
                .iter()
                .map(|node| &expression[node.span.start..node.span.end])
                .collect::<Vec<_>>(),
            ["ui/icon", "tag:ui/icon", "-ui/icon", "ui/icon"]
        );
        assert!(nodes.iter().any(|node| node.value == "ui/icons"));
        assert!(nodes.iter().any(|node| node.value == "ui/*"));
        assert!(!nodes.iter().any(|node| node.value == "color-space:ui-icon"));
    }

    #[test]
    fn rewrites_only_selected_tag_nodes_and_reaudits_the_ast() {
        let expression = r#"ui/icon tag:ui/icon -ui/icon any:(ui/icon|ui/icons) path:"ui/icon" color-space:ui-icon ui/*"#;
        let rewritten =
            rewrite_query_tag(expression, "ui/icon", "design:icon", TagRenameMode::Exact)
                .expect("rewrite exact tags");

        assert_eq!(rewritten.node_count, 4);
        assert_eq!(
            rewritten.expression,
            r#"tag:design:icon tag:design:icon -tag:design:icon any:(design:icon|ui/icons) path:"ui/icon" color-space:ui-icon ui/*"#
        );
        assert_eq!(
            parse_query(&rewritten.expression).expect("rewritten AST"),
            {
                let mut expected = parse_query(expression).expect("source AST");
                expected.all_tags.remove("ui/icon");
                expected.all_tags.insert("design:icon".into());
                expected.excluded_tags.remove("ui/icon");
                expected.excluded_tags.insert("design:icon".into());
                expected.any_tag_groups[0].remove("ui/icon");
                expected.any_tag_groups[0].insert("design:icon".into());
                expected
            }
        );
    }

    #[test]
    fn preserves_quoted_context_for_unicode_and_whitespace_tags() {
        let ordinary = rewrite_query_tag(
            r#""视觉 风格" any:("视觉 风格"|other)"#,
            "视觉 风格",
            "新的 风格",
            TagRenameMode::Exact,
        )
        .expect("rewrite locally quoted tags");
        assert_eq!(
            ordinary.expression,
            r#""新的 风格" any:("新的 风格"|other)"#
        );

        let whole_token = rewrite_query_tag(
            r#""any:(old tag|other)""#,
            "old tag",
            "new tag",
            TagRenameMode::Exact,
        )
        .expect("rewrite member inside whole-token quotes");
        assert_eq!(whole_token.expression, r#""any:(new tag|other)""#);
    }

    #[test]
    fn wildcard_rewrite_requires_an_explicit_namespace_operation() {
        assert!(matches!(
            rewrite_query_tag("ui/* ui/icon", "ui/*", "design/*", TagRenameMode::Exact),
            Err(QueryTagRewriteError::InvalidTag)
        ));
        let rewritten = rewrite_query_tag(
            "ui/* ui/icon",
            "ui/*",
            "design/*",
            TagRenameMode::NamespaceWildcard,
        )
        .expect("rewrite namespace wildcard");
        assert_eq!(rewritten.expression, "design/* ui/icon");
        assert_eq!(rewritten.node_count, 1);
    }

    #[test]
    fn refuses_invalid_sources_and_or_groups_collapsed_by_a_rename() {
        assert!(matches!(
            rewrite_query_tag("\"open", "old", "new", TagRenameMode::Exact),
            Err(QueryTagRewriteError::InvalidQuery(_))
        ));
        assert!(matches!(
            rewrite_query_tag("any:(old|new)", "old", "new", TagRenameMode::Exact),
            Err(QueryTagRewriteError::RewriteInvalid(_))
        ));
    }
}

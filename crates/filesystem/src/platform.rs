use std::path::{Component, Path};

use serde::{Deserialize, Serialize};
use unicode_normalization::UnicodeNormalization;

/// Path rules whose differences affect library identity and portability.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PlatformFamily {
    Windows,
    MacOs,
    Linux,
}

impl PlatformFamily {
    #[must_use]
    pub const fn current() -> Self {
        #[cfg(target_os = "windows")]
        {
            Self::Windows
        }
        #[cfg(target_os = "macos")]
        {
            Self::MacOs
        }
        #[cfg(not(any(target_os = "windows", target_os = "macos")))]
        {
            Self::Linux
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PathRelation {
    Same,
    Ancestor,
    Descendant,
    Distinct,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PathCompatibilityIssueKind {
    WindowsReservedName,
    WindowsForbiddenCharacter,
    WindowsTrailingDotOrSpace,
    WindowsLegacyLength,
}

/// A portability diagnostic. It never authorizes changing or hiding the source file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PathCompatibilityIssue {
    pub platform: PlatformFamily,
    pub kind: PathCompatibilityIssueKind,
    pub component: Option<String>,
}

/// Builds a deterministic identity key for fixture paths under the selected platform rules.
///
/// Windows comparison is case-insensitive and NFC-normalized. macOS normalizes canonical
/// Unicode equivalents while retaining case because APFS can be either case-sensitive or
/// case-insensitive. Linux retains both spelling and case.
#[must_use]
pub fn platform_path_key(path: &str, platform: PlatformFamily) -> String {
    let components = if platform == PlatformFamily::Windows {
        path.split(['/', '\\']).collect::<Vec<_>>()
    } else {
        path.split('/').collect::<Vec<_>>()
    };
    components
        .into_iter()
        .filter(|component| !component.is_empty() && *component != ".")
        .map(|component| normalize_component(component, platform))
        .collect::<Vec<_>>()
        .join("/")
}

/// Reports Windows-specific portability hazards without rejecting a valid macOS/Linux path.
#[must_use]
pub fn inspect_relative_path_compatibility(path: &str) -> Vec<PathCompatibilityIssue> {
    let mut issues = Vec::new();
    for component in path
        .split(['/', '\\'])
        .filter(|component| !component.is_empty())
    {
        if component.chars().any(is_windows_forbidden_character) {
            issues.push(PathCompatibilityIssue {
                platform: PlatformFamily::Windows,
                kind: PathCompatibilityIssueKind::WindowsForbiddenCharacter,
                component: Some(component.to_owned()),
            });
        }
        if component.ends_with(['.', ' ']) {
            issues.push(PathCompatibilityIssue {
                platform: PlatformFamily::Windows,
                kind: PathCompatibilityIssueKind::WindowsTrailingDotOrSpace,
                component: Some(component.to_owned()),
            });
        }
        if is_windows_reserved_name(component) {
            issues.push(PathCompatibilityIssue {
                platform: PlatformFamily::Windows,
                kind: PathCompatibilityIssueKind::WindowsReservedName,
                component: Some(component.to_owned()),
            });
        }
    }
    if path.encode_utf16().count() >= 260 {
        issues.push(PathCompatibilityIssue {
            platform: PlatformFamily::Windows,
            kind: PathCompatibilityIssueKind::WindowsLegacyLength,
            component: None,
        });
    }
    issues
}

/// Compares already parsed native paths with explicitly selected platform semantics.
#[must_use]
pub fn path_relation_for_platform(
    left: &Path,
    right: &Path,
    platform: PlatformFamily,
) -> PathRelation {
    let Some(left) = native_component_keys(left, platform) else {
        return PathRelation::Distinct;
    };
    let Some(right) = native_component_keys(right, platform) else {
        return PathRelation::Distinct;
    };
    if left == right {
        PathRelation::Same
    } else if right.starts_with(&left) {
        PathRelation::Ancestor
    } else if left.starts_with(&right) {
        PathRelation::Descendant
    } else {
        PathRelation::Distinct
    }
}

fn native_component_keys(path: &Path, platform: PlatformFamily) -> Option<Vec<String>> {
    path.components()
        .map(|component| match component {
            Component::CurDir => Some(".".to_owned()),
            Component::ParentDir => Some("..".to_owned()),
            Component::RootDir => Some("/".to_owned()),
            Component::Prefix(prefix) => prefix
                .as_os_str()
                .to_str()
                .map(|value| normalize_component(value, platform)),
            Component::Normal(value) => value
                .to_str()
                .map(|value| normalize_component(value, platform)),
        })
        .collect()
}

fn normalize_component(component: &str, platform: PlatformFamily) -> String {
    match platform {
        PlatformFamily::Windows => component.nfc().flat_map(char::to_lowercase).collect(),
        PlatformFamily::MacOs => component.nfd().collect(),
        PlatformFamily::Linux => component.to_owned(),
    }
}

fn is_windows_forbidden_character(character: char) -> bool {
    character <= '\u{1f}' || matches!(character, '<' | '>' | ':' | '"' | '|' | '?' | '*')
}

fn is_windows_reserved_name(component: &str) -> bool {
    let basename = component
        .trim_end_matches(['.', ' '])
        .split('.')
        .next()
        .unwrap_or_default()
        .to_ascii_uppercase();
    matches!(basename.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || basename
            .strip_prefix("COM")
            .or_else(|| basename.strip_prefix("LPT"))
            .is_some_and(|suffix| {
                matches!(suffix, "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9")
            })
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{
        PathCompatibilityIssueKind, PathRelation, PlatformFamily,
        inspect_relative_path_compatibility, path_relation_for_platform, platform_path_key,
    };

    #[test]
    fn p2_platform_windows_keys_are_case_insensitive_and_unicode_normalized() {
        assert_eq!(
            platform_path_key("Design/Caf\u{e9}.PNG", PlatformFamily::Windows),
            platform_path_key("design/Cafe\u{301}.png", PlatformFamily::Windows)
        );
        assert_eq!(
            path_relation_for_platform(
                Path::new("/Library/Art"),
                Path::new("/library/art/icons"),
                PlatformFamily::Windows,
            ),
            PathRelation::Ancestor
        );
    }

    #[test]
    fn p2_platform_macos_normalizes_unicode_without_assuming_volume_case_mode() {
        assert_eq!(
            platform_path_key("Caf\u{e9}.png", PlatformFamily::MacOs),
            platform_path_key("Cafe\u{301}.png", PlatformFamily::MacOs)
        );
        assert_ne!(
            platform_path_key("Logo.png", PlatformFamily::MacOs),
            platform_path_key("logo.png", PlatformFamily::MacOs)
        );
    }

    #[test]
    fn p2_platform_linux_keys_preserve_case_and_unicode_spelling() {
        assert_ne!(
            platform_path_key("Logo.png", PlatformFamily::Linux),
            platform_path_key("logo.png", PlatformFamily::Linux)
        );
        assert_ne!(
            platform_path_key("Caf\u{e9}.png", PlatformFamily::Linux),
            platform_path_key("Cafe\u{301}.png", PlatformFamily::Linux)
        );
    }

    #[test]
    fn p2_platform_windows_portability_diagnostics_cover_names_characters_and_length() {
        let issues = inspect_relative_path_compatibility("references/CON.txt/bad?.png/trailing. ");
        assert!(issues.iter().any(|issue| {
            issue.kind == PathCompatibilityIssueKind::WindowsReservedName
                && issue.component.as_deref() == Some("CON.txt")
        }));
        assert!(issues.iter().any(|issue| {
            issue.kind == PathCompatibilityIssueKind::WindowsForbiddenCharacter
                && issue.component.as_deref() == Some("bad?.png")
        }));
        assert!(issues.iter().any(|issue| {
            issue.kind == PathCompatibilityIssueKind::WindowsTrailingDotOrSpace
                && issue.component.as_deref() == Some("trailing. ")
        }));

        let long_path = format!("{}.png", "a".repeat(260));
        assert!(
            inspect_relative_path_compatibility(&long_path)
                .iter()
                .any(|issue| issue.kind == PathCompatibilityIssueKind::WindowsLegacyLength)
        );
    }
}

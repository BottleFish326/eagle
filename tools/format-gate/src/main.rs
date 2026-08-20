use std::collections::BTreeMap;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::process::ExitCode;
use std::time::Instant;

use anyhow::{Context, Result, bail};
use asset_core::{AssetIssue, AssetKind, AssetRecord};
use asset_filesystem::{ScanOptions, scan_root};
use asset_preview::{
    ThumbnailOutcome, ThumbnailPlaceholderReason, ThumbnailReady, ThumbnailService,
};
use clap::Parser;
use format_worker::open_libheif_worker_bundle;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tempfile::TempDir;

const MAX_MANIFEST_BYTES: u64 = 1024 * 1024;

#[derive(Debug, Parser)]
#[command(about = "Replay the extended-format manifest against scanner and preview runtime")]
struct Cli {
    #[arg(long, default_value = "fixtures/formats/manifest.json")]
    manifest: PathBuf,
    #[arg(long, default_value = "core-only")]
    provider_profile: String,
    #[arg(long)]
    worker_bundle: Option<PathBuf>,
}

#[derive(Debug, Deserialize)]
struct Manifest {
    fixtures: Vec<Fixture>,
}

#[derive(Debug, Deserialize)]
struct Fixture {
    id: String,
    path: String,
    expectations: Vec<PlatformExpectation>,
    budgets: Budgets,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PlatformExpectation {
    platforms: Vec<String>,
    provider_profile: String,
    result: ExpectedResult,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExpectedResult {
    recognized: bool,
    mime: Option<String>,
    kind: Option<String>,
    issue_codes: Vec<String>,
    metadata: ExpectedCapability,
    preview: ExpectedCapability,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExpectedCapability {
    status: String,
    reason_code: Option<String>,
    #[serde(default)]
    properties: BTreeMap<String, Value>,
    reference_sha256: Option<String>,
    width: Option<u32>,
    height: Option<u32>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Budgets {
    preview_max_ms: u128,
    max_preview_bytes: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct GateFailure {
    fixture_id: String,
    layer: String,
    message: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct GateReport {
    schema: u32,
    accepted: bool,
    platform: String,
    provider_profile: String,
    manifest_sha256: String,
    fixture_count: usize,
    checked_fixture_count: usize,
    scan_elapsed_ms: u128,
    source_bytes: u64,
    source_digest_unchanged: bool,
    failures: Vec<GateFailure>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SourceSnapshot {
    size: u64,
    sha256: String,
}

#[derive(Debug, PartialEq, Eq)]
struct ActualCapability {
    status: &'static str,
    reason_code: Option<&'static str>,
    properties: BTreeMap<String, Value>,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run_gate(
        &cli.manifest,
        &cli.provider_profile,
        cli.worker_bundle.as_deref(),
    ) {
        Ok(report) => {
            println!(
                "{}",
                serde_json::to_string_pretty(&report).expect("gate report serialization")
            );
            if report.accepted {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            }
        }
        Err(error) => {
            eprintln!("format gate could not run: {error:#}");
            ExitCode::FAILURE
        }
    }
}

#[allow(clippy::too_many_lines)]
fn run_gate(
    manifest_path: &Path,
    provider_profile: &str,
    worker_bundle: Option<&Path>,
) -> Result<GateReport> {
    validate_profile(provider_profile, worker_bundle)?;
    let manifest_path = canonical_regular_file(manifest_path, MAX_MANIFEST_BYTES)?;
    let manifest_root = manifest_path
        .parent()
        .context("format manifest has no parent directory")?;
    let manifest_bytes = fs::read(&manifest_path).context("read format manifest")?;
    let manifest: Manifest =
        serde_json::from_slice(&manifest_bytes).context("parse format manifest")?;
    let source_root = manifest_root.join("sources");
    let platform = current_platform()?;
    let before = snapshot_sources(manifest_root, &manifest.fixtures)?;
    let source_bytes = before.values().map(|snapshot| snapshot.size).sum();

    let scan_started = Instant::now();
    let scan = scan_root(&source_root, &ScanOptions::default()).context("scan format sources")?;
    let scan_elapsed_ms = scan_started.elapsed().as_millis();
    let mut failures = scan
        .problems
        .iter()
        .map(|problem| GateFailure {
            fixture_id: "<scan>".into(),
            layer: "scan".into(),
            message: problem.message.clone(),
        })
        .collect::<Vec<_>>();
    let mut assets = scan
        .assets
        .into_iter()
        .map(|asset| (portable_relative_path(&asset.relative_path), asset))
        .collect::<BTreeMap<_, _>>();

    let cache = TempDir::new().context("create isolated preview cache")?;
    let mut previews = ThumbnailService::open(cache.path(), 1).context("open preview cache")?;
    if let Some(bundle) = worker_bundle {
        let worker = open_libheif_worker_bundle(bundle).context("open libheif worker bundle")?;
        previews = previews
            .with_libheif_worker(worker)
            .context("attach libheif worker")?;
    }

    let mut checked_fixture_count = 0;
    for fixture in &manifest.fixtures {
        let Some(expectation) = selected_expectation(fixture, provider_profile, platform) else {
            failure(
                &mut failures,
                &fixture.id,
                "manifest",
                format!("missing unique {provider_profile}/{platform} expectation"),
            );
            continue;
        };
        checked_fixture_count += 1;
        let relative = fixture
            .path
            .strip_prefix("sources/")
            .unwrap_or(fixture.path.as_str());
        let Some(mut asset) = assets.remove(relative) else {
            if expectation.result.recognized {
                failure(
                    &mut failures,
                    &fixture.id,
                    "recognition",
                    "fixture was not returned by the scanner",
                );
            }
            continue;
        };
        if provider_profile == "bundled-codecs" {
            previews
                .enrich_media_properties(&mut asset, &source_root)
                .with_context(|| format!("enrich metadata for {}", fixture.id))?;
        }
        compare_record(&mut failures, fixture, &asset, &expectation.result);
        compare_preview(
            &mut failures,
            fixture,
            &asset,
            &expectation.result.preview,
            &previews,
            &source_root,
            provider_profile,
        )?;
    }
    for relative in assets.keys() {
        failure(
            &mut failures,
            relative,
            "recognition",
            "scanner returned an asset absent from the manifest",
        );
    }

    let after = snapshot_sources(manifest_root, &manifest.fixtures)?;
    let source_digest_unchanged = before == after;
    if !source_digest_unchanged {
        failure(
            &mut failures,
            "<source-set>",
            "source-protection",
            "one or more fixture bytes changed during replay",
        );
    }
    failures.sort_by(|left, right| {
        (&left.fixture_id, &left.layer, &left.message).cmp(&(
            &right.fixture_id,
            &right.layer,
            &right.message,
        ))
    });
    Ok(GateReport {
        schema: 1,
        accepted: failures.is_empty(),
        platform: platform.into(),
        provider_profile: provider_profile.into(),
        manifest_sha256: digest_bytes(&manifest_bytes),
        fixture_count: manifest.fixtures.len(),
        checked_fixture_count,
        scan_elapsed_ms,
        source_bytes,
        source_digest_unchanged,
        failures,
    })
}

fn validate_profile(profile: &str, worker_bundle: Option<&Path>) -> Result<()> {
    match (profile, worker_bundle) {
        ("core-only", None) | ("bundled-codecs", Some(_)) => Ok(()),
        ("core-only", Some(_)) => bail!("core-only profile cannot load a worker bundle"),
        ("bundled-codecs", None) => bail!("bundled-codecs profile requires --worker-bundle"),
        _ => bail!("unsupported provider profile: {profile}"),
    }
}

fn current_platform() -> Result<&'static str> {
    match std::env::consts::OS {
        "windows" => Ok("windows"),
        "macos" => Ok("macos"),
        "linux" => Ok("linux"),
        other => bail!("unsupported gate platform: {other}"),
    }
}

fn canonical_regular_file(path: &Path, max_bytes: u64) -> Result<PathBuf> {
    let metadata =
        fs::symlink_metadata(path).with_context(|| format!("inspect {}", path.display()))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() || metadata.len() > max_bytes {
        bail!("manifest must be a non-symlink regular file of at most {max_bytes} bytes");
    }
    path.canonicalize()
        .with_context(|| format!("canonicalize {}", path.display()))
}

fn selected_expectation<'a>(
    fixture: &'a Fixture,
    profile: &str,
    platform: &str,
) -> Option<&'a PlatformExpectation> {
    let exact = fixture
        .expectations
        .iter()
        .filter(|expectation| {
            expectation.provider_profile == profile
                && expectation
                    .platforms
                    .iter()
                    .any(|candidate| candidate == platform)
        })
        .collect::<Vec<_>>();
    if exact.len() == 1 {
        return Some(exact[0]);
    }
    if !exact.is_empty() || profile == "core-only" {
        return None;
    }
    let inherited = fixture
        .expectations
        .iter()
        .filter(|expectation| {
            expectation.provider_profile == "core-only"
                && expectation
                    .platforms
                    .iter()
                    .any(|candidate| candidate == platform)
        })
        .collect::<Vec<_>>();
    (inherited.len() == 1).then_some(inherited[0])
}

fn snapshot_sources(
    manifest_root: &Path,
    fixtures: &[Fixture],
) -> Result<BTreeMap<String, SourceSnapshot>> {
    fixtures
        .iter()
        .map(|fixture| {
            let path = manifest_root.join(&fixture.path);
            let metadata = fs::symlink_metadata(&path)
                .with_context(|| format!("inspect fixture {}", fixture.id))?;
            if metadata.file_type().is_symlink() || !metadata.is_file() {
                bail!("fixture {} is not a non-symlink regular file", fixture.id);
            }
            let bytes = fs::read(&path).with_context(|| format!("read fixture {}", fixture.id))?;
            Ok((
                fixture.path.clone(),
                SourceSnapshot {
                    size: metadata.len(),
                    sha256: digest_bytes(&bytes),
                },
            ))
        })
        .collect()
}

fn compare_record(
    failures: &mut Vec<GateFailure>,
    fixture: &Fixture,
    asset: &AssetRecord,
    expected: &ExpectedResult,
) {
    compare(
        failures,
        &fixture.id,
        "recognition",
        "mime",
        &Some(asset.mime.clone()),
        &expected.mime,
    );
    compare(
        failures,
        &fixture.id,
        "recognition",
        "kind",
        &Some(kind_name(asset.kind).into()),
        &expected.kind,
    );
    let mut actual_issues = asset.issues.iter().map(issue_code).collect::<Vec<_>>();
    let mut expected_issues = expected.issue_codes.clone();
    actual_issues.sort_unstable();
    expected_issues.sort();
    compare(
        failures,
        &fixture.id,
        "issues",
        "issue codes",
        &actual_issues,
        &expected_issues,
    );
    let actual_metadata = metadata_capability(asset);
    compare_capability(
        failures,
        &fixture.id,
        "metadata",
        &actual_metadata,
        &expected.metadata,
    );
}

fn compare_preview(
    failures: &mut Vec<GateFailure>,
    fixture: &Fixture,
    asset: &AssetRecord,
    expected: &ExpectedCapability,
    previews: &ThumbnailService,
    source_root: &Path,
    profile: &str,
) -> Result<()> {
    let edge = expected
        .width
        .zip(expected.height)
        .map_or(64, |(width, height)| width.max(height));
    let started = Instant::now();
    let outcome = if profile == "bundled-codecs" {
        previews.request_with_authorized_root(asset, edge, source_root)
    } else {
        previews.request(asset, edge)
    }
    .with_context(|| format!("request preview for {}", fixture.id))?;
    let elapsed_ms = started.elapsed().as_millis();
    if elapsed_ms > fixture.budgets.preview_max_ms {
        failure(
            failures,
            &fixture.id,
            "preview",
            format!(
                "preview took {elapsed_ms} ms, budget is {} ms",
                fixture.budgets.preview_max_ms
            ),
        );
    }
    let actual = match outcome {
        ThumbnailOutcome::Ready { thumbnail } => {
            compare_ready_preview(failures, fixture, expected, previews, &thumbnail)?;
            ActualCapability {
                status: "available",
                reason_code: None,
                properties: BTreeMap::new(),
            }
        }
        ThumbnailOutcome::Placeholder { reason, .. } => ActualCapability {
            status: placeholder_status(reason),
            reason_code: Some(preview_reason_code(asset, reason, profile)),
            properties: BTreeMap::new(),
        },
    };
    compare_capability(failures, &fixture.id, "preview", &actual, expected);
    Ok(())
}

fn compare_ready_preview(
    failures: &mut Vec<GateFailure>,
    fixture: &Fixture,
    expected: &ExpectedCapability,
    previews: &ThumbnailService,
    thumbnail: &ThumbnailReady,
) -> Result<()> {
    compare(
        failures,
        &fixture.id,
        "preview",
        "width",
        &thumbnail.width,
        &expected.width.unwrap_or_default(),
    );
    compare(
        failures,
        &fixture.id,
        "preview",
        "height",
        &thumbnail.height,
        &expected.height.unwrap_or_default(),
    );
    let bytes = previews
        .read(&thumbnail.cache_key)
        .with_context(|| format!("read generated preview for {}", fixture.id))?;
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > fixture.budgets.max_preview_bytes {
        failure(
            failures,
            &fixture.id,
            "preview",
            "generated preview exceeds the manifest byte budget",
        );
    }
    compare(
        failures,
        &fixture.id,
        "preview",
        "SHA-256",
        &Some(digest_bytes(&bytes)),
        &expected.reference_sha256,
    );
    Ok(())
}

fn metadata_capability(asset: &AssetRecord) -> ActualCapability {
    let properties = asset_properties(asset);
    if !properties.is_empty() {
        return ActualCapability {
            status: "available",
            reason_code: None,
            properties,
        };
    }
    if let Some(issue) = asset
        .issues
        .iter()
        .find(|issue| !matches!(issue, AssetIssue::MimeMismatch(_)))
    {
        let (status, reason_code) = metadata_issue_status(asset, issue);
        return ActualCapability {
            status,
            reason_code: Some(reason_code),
            properties,
        };
    }
    let (status, reason_code) = match asset.mime.as_str() {
        "image/avif" | "image/heic" | "image/heif" => {
            ("codec-unavailable", "libheif-worker-unavailable")
        }
        "audio/wav" => ("unsupported-feature", "unknown-audio-codec"),
        "application/pdf" => ("unsupported-feature", "pdf-isolated-worker-required"),
        _ => ("unsupported-feature", "metadata-provider-unavailable"),
    };
    ActualCapability {
        status,
        reason_code: Some(reason_code),
        properties,
    }
}

fn metadata_issue_status(asset: &AssetRecord, issue: &AssetIssue) -> (&'static str, &'static str) {
    match issue {
        AssetIssue::InvalidImageMetadata(_) => ("invalid-content", "invalid-svg-xml"),
        AssetIssue::InvalidNativeMetadata(_) => match asset.mime.as_str() {
            "image/avif" | "image/heic" | "image/heif" => {
                ("invalid-content", "worker-invalid-content")
            }
            "application/pdf" => ("invalid-content", "invalid-pdf-structure"),
            mime if mime.starts_with("video/") => ("invalid-content", "invalid-video-container"),
            mime if mime.starts_with("audio/") => ("invalid-content", "invalid-audio-metadata"),
            _ => ("invalid-content", "invalid-native-metadata"),
        },
        AssetIssue::ResourceLimited(_) => match asset.mime.as_str() {
            "image/avif" | "image/heic" | "image/heif" => {
                ("resource-limited", "worker-resource-limited")
            }
            "application/pdf" => ("resource-limited", "pdf-metadata-limit"),
            mime if mime.starts_with("video/") => ("resource-limited", "video-metadata-limit"),
            mime if mime.starts_with("audio/") => ("resource-limited", "audio-metadata-limit"),
            _ => ("resource-limited", "metadata-resource-limit"),
        },
        AssetIssue::UnreadableFile(_) | AssetIssue::MissingAsset => {
            ("unreadable", "source-unreadable")
        }
        AssetIssue::UnsafeEmbeddedContent(_) => ("invalid-content", svg_invalid_reason(asset)),
        _ => ("invalid-content", "metadata-invalid"),
    }
}

fn asset_properties(asset: &AssetRecord) -> BTreeMap<String, Value> {
    let mut properties = BTreeMap::new();
    if let Some(dimensions) = asset.dimensions {
        properties.insert("width".into(), json!(dimensions.width));
        properties.insert("height".into(), json!(dimensions.height));
    }
    if let Some(media) = &asset.media {
        insert_property(&mut properties, "durationMs", media.duration_ms);
        insert_property(&mut properties, "pageCount", media.page_count);
        insert_property(&mut properties, "frameCount", media.frame_count);
        insert_property(&mut properties, "videoTrackCount", media.video_track_count);
        insert_property(&mut properties, "audioTrackCount", media.audio_track_count);
        insert_property(&mut properties, "sampleRateHz", media.sample_rate_hz);
        insert_property(&mut properties, "channelCount", media.channel_count);
        insert_property(&mut properties, "bitDepth", media.bit_depth);
        insert_property(&mut properties, "colorSpace", media.color_space.clone());
        insert_property(&mut properties, "codec", media.codec.clone());
        insert_property(&mut properties, "hasAlpha", media.has_alpha);
    }
    properties
}

fn insert_property<T: Serialize>(
    properties: &mut BTreeMap<String, Value>,
    name: &str,
    value: Option<T>,
) {
    if let Some(value) = value {
        properties.insert(
            name.into(),
            serde_json::to_value(value).expect("media property serialization"),
        );
    }
}

fn placeholder_status(reason: ThumbnailPlaceholderReason) -> &'static str {
    match reason {
        ThumbnailPlaceholderReason::CodecUnavailable => "codec-unavailable",
        ThumbnailPlaceholderReason::PreviewUnavailable
        | ThumbnailPlaceholderReason::UnsupportedFormat => "unsupported-feature",
        ThumbnailPlaceholderReason::Unreadable | ThumbnailPlaceholderReason::MissingAsset => {
            "unreadable"
        }
        ThumbnailPlaceholderReason::InvalidContent | ThumbnailPlaceholderReason::DecodeFailed => {
            "invalid-content"
        }
        ThumbnailPlaceholderReason::ResourceLimited => "resource-limited",
        ThumbnailPlaceholderReason::TimedOut => "timed-out",
        ThumbnailPlaceholderReason::SourceChanged => "source-changed",
    }
}

fn preview_reason_code(
    asset: &AssetRecord,
    reason: ThumbnailPlaceholderReason,
    profile: &str,
) -> &'static str {
    match reason {
        ThumbnailPlaceholderReason::CodecUnavailable => {
            if profile == "bundled-codecs" {
                "worker-codec-unavailable"
            } else {
                "libheif-worker-unavailable"
            }
        }
        ThumbnailPlaceholderReason::PreviewUnavailable => match asset.mime.as_str() {
            "video/mp4" | "video/quicktime" | "video/webm" => "video-frame-worker-unavailable",
            "application/pdf" => "pdfium-worker-unavailable",
            "audio/wav" => "audio-card-only",
            "audio/mpeg" | "audio/flac" => "no-embedded-cover",
            _ => "preview-provider-unavailable",
        },
        ThumbnailPlaceholderReason::InvalidContent | ThumbnailPlaceholderReason::DecodeFailed => {
            match asset.mime.as_str() {
                "image/avif" | "image/heic" | "image/heif" => "worker-invalid-content",
                "image/svg+xml" => svg_invalid_reason(asset),
                mime if mime.starts_with("audio/") => "invalid-audio-metadata",
                _ => "invalid-preview-content",
            }
        }
        ThumbnailPlaceholderReason::ResourceLimited => match asset.mime.as_str() {
            "image/avif" | "image/heic" | "image/heif" => "worker-resource-limited",
            mime if mime.starts_with("audio/") => "audio-cover-limit",
            _ => "preview-resource-limit",
        },
        ThumbnailPlaceholderReason::TimedOut => "preview-timeout",
        ThumbnailPlaceholderReason::SourceChanged => "source-changed",
        ThumbnailPlaceholderReason::Unreadable | ThumbnailPlaceholderReason::MissingAsset => {
            "source-unreadable"
        }
        ThumbnailPlaceholderReason::UnsupportedFormat => "preview-provider-unavailable",
    }
}

fn svg_invalid_reason(asset: &AssetRecord) -> &'static str {
    asset
        .issues
        .iter()
        .find_map(|issue| match issue {
            AssetIssue::UnsafeEmbeddedContent(feature) if feature.contains("external") => {
                Some("external-reference-blocked")
            }
            AssetIssue::UnsafeEmbeddedContent(_) => Some("unsafe-active-content"),
            _ => None,
        })
        .unwrap_or("invalid-svg-xml")
}

fn compare_capability(
    failures: &mut Vec<GateFailure>,
    fixture_id: &str,
    layer: &str,
    actual: &ActualCapability,
    expected: &ExpectedCapability,
) {
    compare(
        failures,
        fixture_id,
        layer,
        "status",
        &actual.status,
        &expected.status.as_str(),
    );
    compare(
        failures,
        fixture_id,
        layer,
        "reason code",
        &actual.reason_code,
        &expected.reason_code.as_deref(),
    );
    if layer == "metadata" {
        compare(
            failures,
            fixture_id,
            layer,
            "properties",
            &actual.properties,
            &expected.properties,
        );
    }
}

fn compare<T: std::fmt::Debug + PartialEq>(
    failures: &mut Vec<GateFailure>,
    fixture_id: &str,
    layer: &str,
    field: &str,
    actual: &T,
    expected: &T,
) {
    if actual != expected {
        failure(
            failures,
            fixture_id,
            layer,
            format!("{field} mismatch: expected {expected:?}, received {actual:?}"),
        );
    }
}

fn failure(
    failures: &mut Vec<GateFailure>,
    fixture_id: impl Into<String>,
    layer: impl Into<String>,
    message: impl Into<String>,
) {
    failures.push(GateFailure {
        fixture_id: fixture_id.into(),
        layer: layer.into(),
        message: message.into(),
    });
}

fn issue_code(issue: &AssetIssue) -> String {
    match issue {
        AssetIssue::InvalidSidecar(_) => "invalid-sidecar",
        AssetIssue::MismatchedSidecar(_) => "mismatched-sidecar",
        AssetIssue::UnreadableFile(_) => "unreadable-file",
        AssetIssue::InvalidImageMetadata(_) => "invalid-image-metadata",
        AssetIssue::InvalidNativeMetadata(_) => "invalid-native-metadata",
        AssetIssue::MimeMismatch(_) => "mime-mismatch",
        AssetIssue::UnsafeEmbeddedContent(_) => "unsafe-embedded-content",
        AssetIssue::ResourceLimited(_) => "resource-limited",
        AssetIssue::MissingAsset => "missing-asset",
        AssetIssue::UnsupportedFormat => "unsupported-format",
    }
    .into()
}

const fn kind_name(kind: AssetKind) -> &'static str {
    match kind {
        AssetKind::Image => "image",
        AssetKind::Video => "video",
        AssetKind::Audio => "audio",
        AssetKind::Pdf => "pdf",
        AssetKind::Other => "other",
    }
}

fn portable_relative_path(path: &Path) -> String {
    path.components()
        .filter_map(|component| match component {
            Component::Normal(value) => Some(value.to_string_lossy()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/")
}

fn digest_bytes(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tracked_core_only_manifest_matches_the_runtime() {
        let repository = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let report = run_gate(
            &repository.join("fixtures/formats/manifest.json"),
            "core-only",
            None,
        )
        .expect("run tracked core-only gate");
        assert!(report.accepted, "{:#?}", report.failures);
        assert_eq!(report.fixture_count, 42);
        assert_eq!(report.checked_fixture_count, 42);
        assert!(report.source_digest_unchanged);
    }

    #[test]
    fn profile_requires_the_matching_worker_boundary() {
        assert!(validate_profile("core-only", None).is_ok());
        assert!(validate_profile("bundled-codecs", None).is_err());
        assert!(validate_profile("other", None).is_err());
        assert!(validate_profile("core-only", Some(Path::new("bundle"))).is_err());
    }

    #[test]
    fn issue_codes_are_stable_and_do_not_expose_messages() {
        assert_eq!(
            issue_code(&AssetIssue::UnsafeEmbeddedContent("secret path".into())),
            "unsafe-embedded-content"
        );
        assert_eq!(
            issue_code(&AssetIssue::ResourceLimited("details".into())),
            "resource-limited"
        );
    }

    #[test]
    fn mismatched_runtime_values_produce_a_rejected_failure() {
        let mut failures = Vec::new();
        compare(
            &mut failures,
            "fixture",
            "metadata",
            "status",
            &"available",
            &"invalid-content",
        );
        assert_eq!(failures.len(), 1);
        assert_eq!(failures[0].fixture_id, "fixture");
        assert_eq!(failures[0].layer, "metadata");
        assert!(failures[0].message.contains("status mismatch"));
    }
}

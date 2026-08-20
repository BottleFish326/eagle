use std::ffi::OsString;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

pub const WORKER_PROTOCOL_SCHEMA: u32 = 1;
pub const MAX_REQUEST_JSON_BYTES: usize = 64 * 1024;
pub const MAX_RESPONSE_JSON_BYTES: usize = 64 * 1024;
pub const DEFAULT_MAX_SOURCE_DIMENSION: u32 = 65_535;
pub const DEFAULT_MAX_DECODE_BYTES: u64 = 256 * 1024 * 1024;
pub const DEFAULT_MAX_OUTPUT_BYTES: u64 = 32 * 1024 * 1024;
pub const DEFAULT_WORKER_TIMEOUT: Duration = Duration::from_secs(10);
const MIN_THUMBNAIL_EDGE: u32 = 16;
const MAX_THUMBNAIL_EDGE: u32 = 2_048;
const MAX_TOKEN_BYTES: usize = 128;
const MAX_ERROR_MESSAGE_BYTES: usize = 1_024;
const MAX_NATIVE_PATH_BYTES: usize = 32 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkerRequest {
    pub schema: u32,
    pub request_id: Uuid,
    pub provider_id: String,
    pub provider_version: String,
    pub operation: WorkerOperation,
    pub source_path: NativePath,
    pub source_size: u64,
    pub source_modified_unix_ns: i128,
    pub limits: WorkerLimits,
}

impl WorkerRequest {
    /// Validates all protocol and resource limits before any source is opened.
    ///
    /// # Errors
    ///
    /// Returns [`ProtocolError`] when the request is malformed or exceeds a fixed bound.
    pub fn validate(&self) -> Result<(), ProtocolError> {
        if self.schema != WORKER_PROTOCOL_SCHEMA {
            return Err(ProtocolError::UnsupportedSchema(self.schema));
        }
        validate_token("provider ID", &self.provider_id)?;
        validate_token("provider version", &self.provider_version)?;
        if self.request_id.get_version_num() != 7 {
            return Err(ProtocolError::InvalidRequestId);
        }
        if self.source_path.encoded_len() > MAX_NATIVE_PATH_BYTES {
            return Err(ProtocolError::PathTooLong);
        }
        self.source_path.to_path_buf()?;
        self.limits.validate(self.operation)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WorkerOperation {
    Metadata,
    Thumbnail,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkerLimits {
    pub max_edge: Option<u32>,
    pub max_source_dimension: u32,
    pub max_decode_bytes: u64,
    pub max_output_bytes: u64,
    pub timeout_ms: u64,
}

impl WorkerLimits {
    fn validate(self, operation: WorkerOperation) -> Result<(), ProtocolError> {
        match (operation, self.max_edge) {
            (WorkerOperation::Thumbnail, Some(edge))
                if (MIN_THUMBNAIL_EDGE..=MAX_THUMBNAIL_EDGE).contains(&edge) => {}
            (WorkerOperation::Metadata, None) => {}
            _ => return Err(ProtocolError::InvalidLimits("maxEdge")),
        }
        if self.max_source_dimension == 0
            || self.max_source_dimension > DEFAULT_MAX_SOURCE_DIMENSION
        {
            return Err(ProtocolError::InvalidLimits("maxSourceDimension"));
        }
        if self.max_decode_bytes == 0 || self.max_decode_bytes > DEFAULT_MAX_DECODE_BYTES {
            return Err(ProtocolError::InvalidLimits("maxDecodeBytes"));
        }
        if self.max_output_bytes == 0 || self.max_output_bytes > DEFAULT_MAX_OUTPUT_BYTES {
            return Err(ProtocolError::InvalidLimits("maxOutputBytes"));
        }
        let max_timeout_ms = u64::try_from(DEFAULT_WORKER_TIMEOUT.as_millis()).unwrap_or(u64::MAX);
        if self.timeout_ms == 0 || self.timeout_ms > max_timeout_ms {
            return Err(ProtocolError::InvalidLimits("timeoutMs"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "encoding", content = "data", rename_all = "kebab-case")]
pub enum NativePath {
    UnixBytes(String),
    WindowsUtf16(String),
    Utf8(String),
}

impl NativePath {
    #[must_use]
    pub fn from_path(path: &Path) -> Self {
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStrExt;
            Self::UnixBytes(encode_hex(path.as_os_str().as_bytes()))
        }
        #[cfg(windows)]
        {
            use std::os::windows::ffi::OsStrExt;
            let bytes = path
                .as_os_str()
                .encode_wide()
                .flat_map(u16::to_be_bytes)
                .collect::<Vec<_>>();
            Self::WindowsUtf16(encode_hex(&bytes))
        }
        #[cfg(not(any(unix, windows)))]
        Self::Utf8(path.to_string_lossy().into_owned())
    }

    /// Reconstructs the native path on the current worker platform.
    ///
    /// # Errors
    ///
    /// Returns [`ProtocolError`] for malformed hex or a path encoding from another platform.
    pub fn to_path_buf(&self) -> Result<PathBuf, ProtocolError> {
        match self {
            #[cfg(unix)]
            Self::UnixBytes(value) => {
                use std::os::unix::ffi::OsStringExt;
                Ok(PathBuf::from(OsString::from_vec(decode_hex(value)?)))
            }
            #[cfg(windows)]
            Self::WindowsUtf16(value) => {
                use std::os::windows::ffi::OsStringExt;
                let bytes = decode_hex(value)?;
                if bytes.len() % 2 != 0 {
                    return Err(ProtocolError::InvalidPathEncoding);
                }
                let units = bytes
                    .chunks_exact(2)
                    .map(|bytes| u16::from_be_bytes([bytes[0], bytes[1]]))
                    .collect::<Vec<_>>();
                Ok(PathBuf::from(OsString::from_wide(&units)))
            }
            Self::Utf8(value) => Ok(PathBuf::from(value)),
            _ => Err(ProtocolError::UnsupportedPathEncoding),
        }
    }

    fn encoded_len(&self) -> usize {
        match self {
            Self::UnixBytes(value) | Self::WindowsUtf16(value) | Self::Utf8(value) => value.len(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkerResponseHeader {
    pub schema: u32,
    pub request_id: Uuid,
    pub provider_id: String,
    pub provider_version: String,
    #[serde(flatten)]
    pub outcome: WorkerOutcome,
}

impl WorkerResponseHeader {
    #[must_use]
    pub fn error(
        request: &WorkerRequest,
        code: WorkerErrorCode,
        message: impl Into<String>,
    ) -> Self {
        Self {
            schema: WORKER_PROTOCOL_SCHEMA,
            request_id: request.request_id,
            provider_id: request.provider_id.clone(),
            provider_version: request.provider_version.clone(),
            outcome: WorkerOutcome::Error {
                code,
                message: message.into(),
            },
        }
    }

    /// Validates response identity and payload declarations.
    ///
    /// # Errors
    ///
    /// Returns [`ProtocolError`] for an invalid schema, token, message, or payload declaration.
    pub fn validate(&self) -> Result<(), ProtocolError> {
        if self.schema != WORKER_PROTOCOL_SCHEMA {
            return Err(ProtocolError::UnsupportedSchema(self.schema));
        }
        validate_token("provider ID", &self.provider_id)?;
        validate_token("provider version", &self.provider_version)?;
        if self.request_id.get_version_num() != 7 {
            return Err(ProtocolError::InvalidRequestId);
        }
        match &self.outcome {
            WorkerOutcome::Ready {
                properties,
                payload,
            } => {
                properties.validate()?;
                if let Some(payload) = payload {
                    payload.validate()?;
                }
            }
            WorkerOutcome::Error { message, .. } => {
                if message.is_empty() || message.len() > MAX_ERROR_MESSAGE_BYTES {
                    return Err(ProtocolError::InvalidErrorMessage);
                }
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "kebab-case")]
pub enum WorkerOutcome {
    Ready {
        properties: HeifProperties,
        payload: Option<PngPayload>,
    },
    Error {
        code: WorkerErrorCode,
        message: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HeifProperties {
    pub width: u32,
    pub height: u32,
    pub orientation: Option<u8>,
    pub color_space: Option<String>,
    pub has_alpha: Option<bool>,
    pub image_count: u32,
}

impl HeifProperties {
    fn validate(&self) -> Result<(), ProtocolError> {
        if self.width == 0
            || self.height == 0
            || self.width > DEFAULT_MAX_SOURCE_DIMENSION
            || self.height > DEFAULT_MAX_SOURCE_DIMENSION
            || self.image_count == 0
            || self
                .orientation
                .is_some_and(|orientation| !(1..=8).contains(&orientation))
            || self
                .color_space
                .as_ref()
                .is_some_and(|value| value.is_empty() || value.len() > MAX_TOKEN_BYTES)
        {
            return Err(ProtocolError::InvalidProperties);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PngPayload {
    pub byte_length: u64,
    pub sha256: String,
    pub width: u32,
    pub height: u32,
}

impl PngPayload {
    fn validate(&self) -> Result<(), ProtocolError> {
        if self.byte_length == 0
            || self.byte_length > DEFAULT_MAX_OUTPUT_BYTES
            || self.sha256.len() != 64
            || !self
                .sha256
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
            || self.width == 0
            || self.height == 0
            || self.width > MAX_THUMBNAIL_EDGE
            || self.height > MAX_THUMBNAIL_EDGE
        {
            return Err(ProtocolError::InvalidPayload);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WorkerErrorCode {
    CodecUnavailable,
    UnsupportedFeature,
    InvalidContent,
    ResourceLimited,
    TimedOut,
    SourceChanged,
    Unreadable,
    DecodeFailed,
    Internal,
}

#[derive(Debug, Error)]
pub enum ProtocolError {
    #[error("worker I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("unsupported worker protocol schema {0}")]
    UnsupportedSchema(u32),
    #[error("worker request ID must be UUIDv7")]
    InvalidRequestId,
    #[error("invalid worker protocol token: {0}")]
    InvalidToken(&'static str),
    #[error("native path exceeds the worker protocol bound")]
    PathTooLong,
    #[error("native path encoding is malformed")]
    InvalidPathEncoding,
    #[error("native path encoding is not valid on this platform")]
    UnsupportedPathEncoding,
    #[error("invalid worker resource limit: {0}")]
    InvalidLimits(&'static str),
    #[error("worker request JSON exceeds its bound")]
    RequestTooLarge,
    #[error("worker response JSON exceeds its bound")]
    ResponseTooLarge,
    #[error("worker PNG payload exceeds its bound")]
    PayloadTooLarge,
    #[error("worker JSON is invalid: {0}")]
    InvalidJson(#[from] serde_json::Error),
    #[error("worker properties are invalid")]
    InvalidProperties,
    #[error("worker payload declaration is invalid")]
    InvalidPayload,
    #[error("worker error message is empty or exceeds its bound")]
    InvalidErrorMessage,
    #[error("worker response has trailing bytes")]
    TrailingBytes,
}

/// Writes one bounded length-prefixed request frame.
///
/// # Errors
///
/// Returns [`ProtocolError`] for invalid data, oversized JSON, or an I/O failure.
pub fn write_request(mut writer: impl Write, request: &WorkerRequest) -> Result<(), ProtocolError> {
    request.validate()?;
    let json = serde_json::to_vec(request)?;
    if json.len() > MAX_REQUEST_JSON_BYTES {
        return Err(ProtocolError::RequestTooLarge);
    }
    write_length(&mut writer, json.len())?;
    writer.write_all(&json)?;
    writer.flush()?;
    Ok(())
}

/// Reads one bounded length-prefixed request frame.
///
/// # Errors
///
/// Returns [`ProtocolError`] for invalid data, oversized JSON, or an I/O failure.
pub fn read_request(mut reader: impl Read) -> Result<WorkerRequest, ProtocolError> {
    let json = read_bounded_frame(
        &mut reader,
        MAX_REQUEST_JSON_BYTES,
        ProtocolError::RequestTooLarge,
    )?;
    let request = serde_json::from_slice::<WorkerRequest>(&json)?;
    request.validate()?;
    Ok(request)
}

/// Writes a response header followed by its exact optional PNG payload.
///
/// # Errors
///
/// Returns [`ProtocolError`] when identity, lengths, JSON, or output bytes are invalid.
pub fn write_response(
    mut writer: impl Write,
    response: &WorkerResponseHeader,
    png: &[u8],
) -> Result<(), ProtocolError> {
    response.validate()?;
    validate_payload_pair(response, png, DEFAULT_MAX_OUTPUT_BYTES)?;
    let json = serde_json::to_vec(response)?;
    if json.len() > MAX_RESPONSE_JSON_BYTES {
        return Err(ProtocolError::ResponseTooLarge);
    }
    write_length(&mut writer, json.len())?;
    writer.write_all(&json)?;
    writer.write_all(png)?;
    writer.flush()?;
    Ok(())
}

/// Reads a response frame while enforcing caller-selected JSON and payload bounds.
///
/// # Errors
///
/// Returns [`ProtocolError`] for invalid framing, identity fields, or payload bytes.
pub fn read_response(
    mut reader: impl Read,
    max_json_bytes: usize,
    max_png_bytes: u64,
) -> Result<(WorkerResponseHeader, Vec<u8>), ProtocolError> {
    let json = read_bounded_frame(
        &mut reader,
        max_json_bytes.min(MAX_RESPONSE_JSON_BYTES),
        ProtocolError::ResponseTooLarge,
    )?;
    let response = serde_json::from_slice::<WorkerResponseHeader>(&json)?;
    response.validate()?;
    let payload_len = match &response.outcome {
        WorkerOutcome::Ready {
            payload: Some(payload),
            ..
        } => payload.byte_length,
        _ => 0,
    };
    if payload_len > max_png_bytes || payload_len > DEFAULT_MAX_OUTPUT_BYTES {
        return Err(ProtocolError::PayloadTooLarge);
    }
    let mut png =
        vec![0; usize::try_from(payload_len).map_err(|_| ProtocolError::PayloadTooLarge)?];
    reader.read_exact(&mut png)?;
    let mut trailing = [0_u8; 1];
    if reader.read(&mut trailing)? != 0 {
        return Err(ProtocolError::TrailingBytes);
    }
    validate_payload_pair(&response, &png, max_png_bytes)?;
    Ok((response, png))
}

fn validate_payload_pair(
    response: &WorkerResponseHeader,
    png: &[u8],
    max_png_bytes: u64,
) -> Result<(), ProtocolError> {
    let declared = match &response.outcome {
        WorkerOutcome::Ready { payload, .. } => payload.as_ref(),
        WorkerOutcome::Error { .. } => None,
    };
    match (declared, png.is_empty()) {
        (None, true) => Ok(()),
        (Some(payload), false)
            if payload.byte_length == u64::try_from(png.len()).unwrap_or(u64::MAX)
                && payload.byte_length <= max_png_bytes =>
        {
            Ok(())
        }
        _ => Err(ProtocolError::InvalidPayload),
    }
}

fn read_bounded_frame(
    reader: &mut impl Read,
    maximum: usize,
    too_large: ProtocolError,
) -> Result<Vec<u8>, ProtocolError> {
    let mut length = [0_u8; 4];
    reader.read_exact(&mut length)?;
    let Ok(length) = usize::try_from(u32::from_be_bytes(length)) else {
        return Err(too_large);
    };
    if length > maximum {
        return Err(too_large);
    }
    let mut bytes = vec![0; length];
    reader.read_exact(&mut bytes)?;
    Ok(bytes)
}

fn write_length(writer: &mut impl Write, length: usize) -> Result<(), ProtocolError> {
    let length = u32::try_from(length).map_err(|_| ProtocolError::ResponseTooLarge)?;
    writer.write_all(&length.to_be_bytes())?;
    Ok(())
}

fn validate_token(name: &'static str, value: &str) -> Result<(), ProtocolError> {
    if value.is_empty()
        || value.len() > MAX_TOKEN_BYTES
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    {
        return Err(ProtocolError::InvalidToken(name));
    }
    Ok(())
}

fn encode_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len().saturating_mul(2));
    for byte in bytes {
        output.push(char::from(HEX[usize::from(byte >> 4)]));
        output.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    output
}

fn decode_hex(value: &str) -> Result<Vec<u8>, ProtocolError> {
    if value.len() % 2 != 0 {
        return Err(ProtocolError::InvalidPathEncoding);
    }
    value
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let high = decode_nibble(pair[0])?;
            let low = decode_nibble(pair[1])?;
            Ok((high << 4) | low)
        })
        .collect()
}

fn decode_nibble(value: u8) -> Result<u8, ProtocolError> {
    match value {
        b'0'..=b'9' => Ok(value - b'0'),
        b'a'..=b'f' => Ok(value - b'a' + 10),
        _ => Err(ProtocolError::InvalidPathEncoding),
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use sha2::{Digest, Sha256};

    use super::*;

    #[test]
    fn request_round_trip_preserves_native_paths_and_limits() {
        let request = request(WorkerOperation::Thumbnail);
        let mut frame = Vec::new();
        write_request(&mut frame, &request).expect("write request");
        assert_eq!(
            read_request(Cursor::new(frame)).expect("read request"),
            request
        );
        assert_eq!(
            request.source_path.to_path_buf().expect("native path"),
            Path::new("/tmp/素材.avif")
        );
    }

    #[test]
    fn response_round_trip_binds_exact_png_length() {
        let request = request(WorkerOperation::Thumbnail);
        let png = b"\x89PNG\r\n\x1a\nfixture";
        let response = WorkerResponseHeader {
            schema: WORKER_PROTOCOL_SCHEMA,
            request_id: request.request_id,
            provider_id: request.provider_id.clone(),
            provider_version: request.provider_version.clone(),
            outcome: WorkerOutcome::Ready {
                properties: properties(),
                payload: Some(PngPayload {
                    byte_length: u64::try_from(png.len()).unwrap(),
                    sha256: format!("{:x}", Sha256::digest(png)),
                    width: 16,
                    height: 16,
                }),
            },
        };
        let mut frame = Vec::new();
        write_response(&mut frame, &response, png).expect("write response");
        let (decoded, decoded_png) =
            read_response(Cursor::new(frame), 65_536, 1_024).expect("read response");
        assert_eq!(decoded, response);
        assert_eq!(decoded_png, png);
    }

    #[test]
    fn rejects_invalid_limits_messages_and_trailing_bytes() {
        let mut invalid = request(WorkerOperation::Thumbnail);
        invalid.limits.max_edge = Some(4_096);
        assert!(matches!(
            invalid.validate(),
            Err(ProtocolError::InvalidLimits("maxEdge"))
        ));

        let request = request(WorkerOperation::Metadata);
        let response =
            WorkerResponseHeader::error(&request, WorkerErrorCode::InvalidContent, "bad");
        let mut frame = Vec::new();
        write_response(&mut frame, &response, &[]).expect("write error");
        frame.push(0);
        assert!(matches!(
            read_response(Cursor::new(frame), 65_536, 1_024),
            Err(ProtocolError::TrailingBytes)
        ));
    }

    fn request(operation: WorkerOperation) -> WorkerRequest {
        WorkerRequest {
            schema: WORKER_PROTOCOL_SCHEMA,
            request_id: Uuid::now_v7(),
            provider_id: "fixture-provider".into(),
            provider_version: "fixture-v1".into(),
            operation,
            source_path: NativePath::from_path(Path::new("/tmp/素材.avif")),
            source_size: 42,
            source_modified_unix_ns: 123,
            limits: WorkerLimits {
                max_edge: (operation == WorkerOperation::Thumbnail).then_some(16),
                max_source_dimension: DEFAULT_MAX_SOURCE_DIMENSION,
                max_decode_bytes: DEFAULT_MAX_DECODE_BYTES,
                max_output_bytes: 1_024,
                timeout_ms: 1_000,
            },
        }
    }

    fn properties() -> HeifProperties {
        HeifProperties {
            width: 16,
            height: 16,
            orientation: Some(1),
            color_space: Some("srgb".into()),
            has_alpha: Some(true),
            image_count: 1,
        }
    }
}

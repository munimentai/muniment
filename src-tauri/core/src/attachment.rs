//! Streaming ingestion of local chat attachments into CAS and the run journal.

use crate::cas::{CasError, ContentHash, LocalCas};
use crate::journal::{EventEnvelope, EventPayload, JournalError, RunJournal};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::io::Read;

pub const ATTACHMENT_EVENT_TYPE: &str = "chat.attachment.ingested";
pub const ATTACHMENT_EVENT_VERSION: u32 = 1;
pub const MAX_PI_IMAGE_BYTES: u64 = 10 * 1024 * 1024;
pub const MAX_PI_IMAGE_COUNT: usize = 10;
pub const MAX_PI_IMAGE_TOTAL_BYTES: u64 = 20 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PiAttachmentImage {
    pub data: String,
    pub mime_type: &'static str,
}

#[derive(Debug)]
pub enum AttachmentDeliveryError {
    Storage(CasError),
    InvalidStoredLength,
    ImageLimit,
    AmbiguousFormat,
}

/// Resolves durable attachment events in journal order without exposing their
/// storage addresses. Unsupported byte formats remain local-only.
pub fn prepare_pi_images(
    cas: &LocalCas,
    events: &[EventEnvelope],
) -> Result<Vec<PiAttachmentImage>, AttachmentDeliveryError> {
    let mut decoded = Vec::new();
    let mut total = 0_u64;
    for event in events {
        let EventPayload::Attachment { attachment } = &event.payload else {
            continue;
        };
        let bytes = cas
            .get_verified(attachment.sha256())
            .map_err(AttachmentDeliveryError::Storage)?;
        let byte_length =
            u64::try_from(bytes.len()).map_err(|_| AttachmentDeliveryError::ImageLimit)?;
        if byte_length != attachment.byte_length() {
            return Err(AttachmentDeliveryError::InvalidStoredLength);
        }
        let Some(mime_type) = validated_image_type(&bytes)? else {
            continue;
        };
        if byte_length > MAX_PI_IMAGE_BYTES || decoded.len() == MAX_PI_IMAGE_COUNT {
            return Err(AttachmentDeliveryError::ImageLimit);
        }
        total = total
            .checked_add(byte_length)
            .ok_or(AttachmentDeliveryError::ImageLimit)?;
        if total > MAX_PI_IMAGE_TOTAL_BYTES {
            return Err(AttachmentDeliveryError::ImageLimit);
        }
        decoded.push((bytes, mime_type));
    }

    Ok(decoded
        .into_iter()
        .map(|(bytes, mime_type)| PiAttachmentImage {
            data: STANDARD.encode(bytes),
            mime_type,
        })
        .collect())
}

fn validated_image_type(bytes: &[u8]) -> Result<Option<&'static str>, AttachmentDeliveryError> {
    let matches = [
        (valid_png(bytes), "image/png"),
        (valid_jpeg(bytes), "image/jpeg"),
        (valid_gif(bytes), "image/gif"),
        (valid_webp(bytes), "image/webp"),
    ];
    let mut supported = matches
        .into_iter()
        .filter_map(|(valid, mime)| valid.then_some(mime));
    let result = supported.next();
    if supported.next().is_some() {
        return Err(AttachmentDeliveryError::AmbiguousFormat);
    }
    Ok(result)
}

fn valid_png(bytes: &[u8]) -> bool {
    bytes.len() >= 45
        && bytes.starts_with(b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR")
        && bytes.ends_with(b"\0\0\0\0IEND\xaeB`\x82")
}

fn valid_jpeg(bytes: &[u8]) -> bool {
    bytes.len() >= 4 && bytes.starts_with(b"\xff\xd8") && bytes.ends_with(b"\xff\xd9")
}

fn valid_gif(bytes: &[u8]) -> bool {
    bytes.len() >= 14
        && (bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a"))
        && bytes.last() == Some(&0x3b)
}

fn valid_webp(bytes: &[u8]) -> bool {
    if bytes.len() < 20
        || !bytes.starts_with(b"RIFF")
        || bytes.get(8..12) != Some(&b"WEBP"[..])
        || !matches!(
            bytes.get(12..16),
            Some(b"VP8 ") | Some(b"VP8L") | Some(b"VP8X")
        )
    {
        return false;
    }
    let Some(size) = bytes
        .get(4..8)
        .and_then(|value| <[u8; 4]>::try_from(value).ok())
        .map(u32::from_le_bytes)
    else {
        return false;
    };
    u64::from(size) + 8 == bytes.len() as u64
}

/// Safe, durable attachment fields. The source path is deliberately absent.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ChatAttachment {
    sha256: ContentHash,
    display_name: String,
    byte_length: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    media_type: Option<String>,
}

impl ChatAttachment {
    pub fn sha256(&self) -> &ContentHash {
        &self.sha256
    }

    pub fn display_name(&self) -> &str {
        &self.display_name
    }

    pub fn byte_length(&self) -> u64 {
        self.byte_length
    }

    pub fn media_type(&self) -> Option<&str> {
        self.media_type.as_deref()
    }
}

impl<'de> Deserialize<'de> for ChatAttachment {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct StoredAttachment {
            sha256: ContentHash,
            display_name: String,
            byte_length: u64,
            media_type: Option<String>,
        }

        let stored = StoredAttachment::deserialize(deserializer)?;
        let display_name = sanitize_display_name(&stored.display_name)
            .filter(|name| name == &stored.display_name)
            .ok_or_else(|| serde::de::Error::custom("invalid sanitized attachment display name"))?;
        let media_type = stored
            .media_type
            .as_deref()
            .map(validate_media_type)
            .transpose()
            .map_err(serde::de::Error::custom)?;
        Ok(Self {
            sha256: stored.sha256,
            display_name,
            byte_length: stored.byte_length,
            media_type,
        })
    }
}

/// Untrusted metadata supplied by the caller alongside the streaming reader.
#[derive(Clone, Copy, Debug)]
pub struct AttachmentMetadata<'a> {
    pub display_name: &'a str,
    pub byte_length: u64,
    pub media_type: Option<&'a str>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum AttachmentValidationError {
    InvalidDisplayName,
    InvalidMediaType(String),
}

impl fmt::Display for AttachmentValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidDisplayName => f.write_str("invalid display name"),
            Self::InvalidMediaType(value) => write!(f, "invalid media type: {value}"),
        }
    }
}

#[derive(Debug)]
pub enum AttachmentIngestError {
    Validation(AttachmentValidationError),
    Cas(CasError),
    ByteCountMismatch {
        declared: u64,
        actual: u64,
        published_hash: ContentHash,
    },
    /// CAS publication succeeded, but the journal reference was not appended.
    /// The published object is unreferenced and may be collected.
    JournalAppend {
        source: Box<JournalError>,
        attachment: ChatAttachment,
    },
}

impl fmt::Display for AttachmentIngestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Validation(error) => write!(f, "invalid attachment metadata: {error:?}"),
            Self::Cas(error) => write!(f, "attachment CAS ingestion failed: {error}"),
            Self::ByteCountMismatch {
                declared, actual, ..
            } => write!(
                f,
                "attachment declared {declared} bytes but reader produced {actual}"
            ),
            Self::JournalAppend { source, .. } => {
                write!(
                    f,
                    "attachment was published but journal append failed: {source}"
                )
            }
        }
    }
}

impl std::error::Error for AttachmentIngestError {}

/// Publishes the complete reader to CAS, then builds and appends its event.
///
/// The caller should hold the application state lock for this entire call.
pub fn ingest_attachment<R, F>(
    cas: &LocalCas,
    journal: &mut RunJournal,
    expected_last_seq: u64,
    reader: &mut R,
    metadata: AttachmentMetadata<'_>,
    event_with_attachment: F,
) -> Result<ChatAttachment, AttachmentIngestError>
where
    R: Read,
    F: FnOnce(ChatAttachment) -> EventEnvelope,
{
    let display_name = sanitize_display_name(metadata.display_name)
        .ok_or(AttachmentValidationError::InvalidDisplayName)
        .map_err(AttachmentIngestError::Validation)?;
    let media_type = metadata
        .media_type
        .map(validate_media_type)
        .transpose()
        .map_err(AttachmentIngestError::Validation)?;

    let mut counted = CountingReader {
        inner: reader,
        count: 0,
    };
    let hash = cas
        .put_reader(&mut counted)
        .map_err(AttachmentIngestError::Cas)?;
    if counted.count != metadata.byte_length {
        return Err(AttachmentIngestError::ByteCountMismatch {
            declared: metadata.byte_length,
            actual: counted.count,
            published_hash: hash,
        });
    }
    let attachment = ChatAttachment {
        sha256: hash,
        display_name,
        byte_length: counted.count,
        media_type,
    };
    let mut event = event_with_attachment(attachment.clone());
    event.event_type = ATTACHMENT_EVENT_TYPE.into();
    event.event_version = ATTACHMENT_EVENT_VERSION;
    event.payload = EventPayload::Attachment {
        attachment: attachment.clone(),
    };
    journal
        .append(expected_last_seq, &event)
        .map_err(|source| AttachmentIngestError::JournalAppend {
            source: Box::new(source),
            attachment: attachment.clone(),
        })?;
    Ok(attachment)
}

fn sanitize_display_name(value: &str) -> Option<String> {
    let name = value.rsplit(['/', '\\']).next()?.trim();
    if name.is_empty()
        || matches!(name, "." | "..")
        || name.chars().any(|character| character.is_control())
    {
        None
    } else {
        Some(name.to_owned())
    }
}

fn validate_media_type(value: &str) -> Result<String, AttachmentValidationError> {
    fn token(value: &str) -> bool {
        !value.is_empty()
            && value.bytes().all(|byte| {
                byte.is_ascii_alphanumeric()
                    || matches!(
                        byte,
                        b'!' | b'#' | b'$' | b'&' | b'^' | b'_' | b'.' | b'+' | b'-'
                    )
            })
    }
    let mut parts = value.split('/');
    if !token(parts.next().unwrap_or_default())
        || !token(parts.next().unwrap_or_default())
        || parts.next().is_some()
    {
        return Err(AttachmentValidationError::InvalidMediaType(value.into()));
    }
    Ok(value.to_ascii_lowercase())
}

struct CountingReader<'a, R> {
    inner: &'a mut R,
    count: u64,
}

impl<R: Read> Read for CountingReader<'_, R> {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        let count = self.inner.read(buffer)?;
        self.count = self.count.checked_add(count as u64).ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "attachment exceeds u64 bytes",
            )
        })?;
        Ok(count)
    }
}

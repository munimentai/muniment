use super::protocol::{self, ProtocolError};
use base64::Engine;
use serde::{de::DeserializeOwned, Serialize};
use serde_json::Value;
use std::{
    fmt,
    io::{self, Read, Write},
};

/// ADR 0009 caps control frames to bound allocation at the local trust boundary.
pub const MAX_FRAME_BYTES: usize = 1024 * 1024;
/// Artifact windows keep decoded chunks independently bounded by ADR 0009.
pub const MAX_ARTIFACT_CHUNK_BYTES: usize = 256 * 1024;
/// Defensive RFC 8259 implementation limits; ordinary protocol bodies stay far below these.
pub const MAX_JSON_DEPTH: usize = 32;
pub const MAX_STRING_BYTES: usize = 64 * 1024;
pub const MAX_COLLECTION_ITEMS: usize = 4096;

#[derive(Debug)]
pub enum CodecError {
    Io(io::Error),
    ZeroLength,
    FrameTooLarge,
    Truncated,
    InvalidUtf8,
    InvalidJson,
    TrailingData,
    LimitExceeded,
    ArtifactChunkTooLarge,
    Protocol(ProtocolError),
}
impl fmt::Display for CodecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}",
            match self {
                Self::Io(_) => "attach frame I/O failed",
                Self::ZeroLength => "zero-length attach frame",
                Self::FrameTooLarge => "attach frame exceeds limit",
                Self::Truncated => "truncated attach frame",
                Self::InvalidUtf8 => "attach frame is not UTF-8",
                Self::InvalidJson => "invalid attach JSON",
                Self::TrailingData => "trailing attach JSON data",
                Self::LimitExceeded => "attach JSON exceeds structural limits",
                Self::ArtifactChunkTooLarge => "artifact chunk exceeds decoded limit",
                Self::Protocol(_) => "invalid attach protocol message",
            }
        )
    }
}
impl std::error::Error for CodecError {}

pub fn read_frame<R: Read, T: DeserializeOwned>(reader: &mut R) -> Result<T, CodecError> {
    let mut prefix = [0; 4];
    read_exact(reader, &mut prefix, true)?;
    let len = u32::from_be_bytes(prefix) as usize;
    if len == 0 {
        return Err(CodecError::ZeroLength);
    }
    if len > MAX_FRAME_BYTES {
        return Err(CodecError::FrameTooLarge);
    }
    let mut body = vec![0; len];
    read_exact(reader, &mut body, false)?;
    decode_json(&body)
}
/// Reads and semantically validates a request at the trust boundary.
pub fn read_request<R: Read>(reader: &mut R) -> Result<super::protocol::Request, CodecError> {
    let request = read_frame(reader)?;
    protocol::validate_request(&request).map_err(CodecError::Protocol)?;
    Ok(request)
}
fn read_exact<R: Read>(reader: &mut R, mut out: &mut [u8], prefix: bool) -> Result<(), CodecError> {
    while !out.is_empty() {
        match reader.read(out) {
            Ok(0) => return Err(CodecError::Truncated),
            Ok(n) => out = &mut out[n..],
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(CodecError::Io(e)),
        }
    }
    let _ = prefix;
    Ok(())
}
pub fn decode_frame<T: DeserializeOwned>(frame: &[u8]) -> Result<T, CodecError> {
    if frame.len() < 4 {
        return Err(CodecError::Truncated);
    }
    let len = u32::from_be_bytes(frame[..4].try_into().unwrap()) as usize;
    if len == 0 {
        return Err(CodecError::ZeroLength);
    }
    if len > MAX_FRAME_BYTES {
        return Err(CodecError::FrameTooLarge);
    }
    if frame.len() < 4 + len {
        return Err(CodecError::Truncated);
    }
    if frame.len() != 4 + len {
        return Err(CodecError::TrailingData);
    }
    decode_json(&frame[4..])
}
pub fn decode_request(frame: &[u8]) -> Result<super::protocol::Request, CodecError> {
    let request = decode_frame(frame)?;
    protocol::validate_request(&request).map_err(CodecError::Protocol)?;
    Ok(request)
}
macro_rules! validated_decoder {
    ($name:ident, $ty:ty, $validator:path) => {
        pub fn $name(frame: &[u8]) -> Result<$ty, CodecError> {
            let value = decode_frame(frame)?;
            $validator(&value).map_err(CodecError::Protocol)?;
            Ok(value)
        }
    };
}
validated_decoder!(decode_hello, protocol::Hello, protocol::validate_hello);
validated_decoder!(
    decode_welcome,
    protocol::Welcome,
    protocol::validate_welcome
);
validated_decoder!(
    decode_authorized,
    protocol::Authorized,
    protocol::validate_authorized
);
validated_decoder!(decode_event, protocol::Event, protocol::validate_event);
fn decode_json<T: DeserializeOwned>(body: &[u8]) -> Result<T, CodecError> {
    let text = std::str::from_utf8(body).map_err(|_| CodecError::InvalidUtf8)?;
    let mut stream = serde_json::Deserializer::from_str(text).into_iter::<Value>();
    let value = stream
        .next()
        .ok_or(CodecError::InvalidJson)?
        .map_err(|_| CodecError::InvalidJson)?;
    if stream.next().is_some() {
        return Err(CodecError::TrailingData);
    }
    let artifact_chunk = value.get("event").and_then(Value::as_str) == Some("artifact.chunk");
    check_artifact(&value)?;
    check(&value, 1, &mut 0, artifact_chunk, false)?;
    serde_json::from_value(value).map_err(|_| CodecError::InvalidJson)
}
fn check(
    value: &Value,
    depth: usize,
    count: &mut usize,
    artifact_chunk: bool,
    artifact_body: bool,
) -> Result<(), CodecError> {
    if depth > MAX_JSON_DEPTH {
        return Err(CodecError::LimitExceeded);
    }
    match value {
        Value::String(v) if v.len() > MAX_STRING_BYTES => Err(CodecError::LimitExceeded),
        Value::Array(v) => {
            *count += v.len();
            if *count > MAX_COLLECTION_ITEMS {
                return Err(CodecError::LimitExceeded);
            }
            for x in v {
                check(x, depth + 1, count, artifact_chunk, false)?
            }
            Ok(())
        }
        Value::Object(v) => {
            *count += v.len();
            if *count > MAX_COLLECTION_ITEMS {
                return Err(CodecError::LimitExceeded);
            }
            for (k, x) in v {
                if k.len() > MAX_STRING_BYTES {
                    return Err(CodecError::LimitExceeded);
                }
                // Base64 expands a permitted 256 KiB artifact beyond the
                // ordinary string bound; it has already been decoded and
                // checked above. No other field gets this exception.
                if !(artifact_body && k == "data") {
                    check(
                        x,
                        depth + 1,
                        count,
                        artifact_chunk,
                        artifact_chunk && k == "body",
                    )?
                }
            }
            Ok(())
        }
        _ => Ok(()),
    }
}
fn check_artifact(v: &Value) -> Result<(), CodecError> {
    if v.get("event").and_then(Value::as_str) == Some("artifact.chunk") {
        let body = v
            .get("body")
            .and_then(Value::as_object)
            .ok_or(CodecError::InvalidJson)?;
        let data = body
            .get("data")
            .and_then(Value::as_str)
            .ok_or(CodecError::InvalidJson)?;
        let estimated = data.len().saturating_mul(3) / 4;
        if estimated > MAX_ARTIFACT_CHUNK_BYTES + 2 {
            return Err(CodecError::ArtifactChunkTooLarge);
        }
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(data)
            .map_err(|_| CodecError::InvalidJson)?;
        if decoded.len() > MAX_ARTIFACT_CHUNK_BYTES {
            return Err(CodecError::ArtifactChunkTooLarge);
        }
        let declared = body
            .get("byte_length")
            .and_then(Value::as_u64)
            .ok_or(CodecError::InvalidJson)?;
        if declared != decoded.len() as u64 {
            return Err(CodecError::InvalidJson);
        }
    }
    Ok(())
}
pub fn encode_frame<T: Serialize>(value: &T) -> Result<Vec<u8>, CodecError> {
    let body = serde_json::to_vec(value).map_err(|_| CodecError::InvalidJson)?;
    if body.is_empty() {
        return Err(CodecError::ZeroLength);
    }
    if body.len() > MAX_FRAME_BYTES {
        return Err(CodecError::FrameTooLarge);
    }
    let mut frame = Vec::with_capacity(body.len() + 4);
    frame.extend_from_slice(&(body.len() as u32).to_be_bytes());
    frame.extend(body);
    Ok(frame)
}
pub fn write_frame<W: Write, T: Serialize>(writer: &mut W, value: &T) -> Result<(), CodecError> {
    let frame = encode_frame(value)?;
    writer.write_all(&frame).map_err(CodecError::Io)
}

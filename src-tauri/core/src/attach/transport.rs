//! Native transport boundary for the attach protocol.
//!
//! Transports authenticate a peer before returning its byte stream. Protocol
//! framing and envelope interpretation remain in the pure attach modules.

use std::io::{Read, Write};

#[cfg(target_os = "linux")]
pub mod linux;

pub trait AcceptedStream: Read + Write + Send {}

impl<T: Read + Write + Send> AcceptedStream for T {}

pub trait AttachListener {
    type Stream: AcceptedStream;
    type Error: std::error::Error + Send + Sync + 'static;

    fn accept(&self) -> Result<Self::Stream, Self::Error>;
}

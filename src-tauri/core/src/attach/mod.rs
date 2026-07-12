//! Pure, transport-independent `muniment.attach/1` wire contract.

pub mod codec;
pub mod protocol;

pub use codec::{
    decode_frame, decode_request, encode_frame, read_frame, read_request, write_frame, CodecError,
};
pub use protocol::*;

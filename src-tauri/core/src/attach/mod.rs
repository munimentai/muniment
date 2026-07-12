//! Pure, transport-independent `muniment.attach/1` wire contract.

pub mod codec;
pub mod protocol;

pub use codec::{
    decode_authorized, decode_event, decode_frame, decode_hello, decode_request, decode_welcome,
    encode_frame, read_frame, read_request, write_frame, CodecError,
};
pub use protocol::*;

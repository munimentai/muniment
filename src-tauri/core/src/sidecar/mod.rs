//! Portable supervision for line-oriented child processes.

mod io;
mod jsonrpc;
mod pi;
mod supervisor;

pub use io::{LineReader, LineWriter, SidecarError, SidecarIo};
pub use jsonrpc::{
    JsonRpcCancellationToken, JsonRpcErrorObject, JsonRpcErrorResponse, JsonRpcId,
    JsonRpcNotification, JsonRpcRequest, JsonRpcSuccess, JsonRpcTransport, JsonRpcTransportError,
    JsonRpcVersion,
};
pub use pi::{pi_readiness_probe, pi_sidecar_config, PI_NPM_PACKAGE, PI_VERSION};
pub use supervisor::{
    ProbeOutcome, RestartPolicy, SidecarConfig, SidecarEvent, SidecarEventCause, SidecarStatus,
    SidecarSupervisor,
};

//! Portable supervision for line-oriented child processes.

mod io;
mod jsonrpc;
mod pi;
pub mod pi_chat;
pub mod pi_install;
mod supervisor;

pub use io::{LineReader, LineWriter, SidecarError, SidecarIo};
pub use jsonrpc::{
    JsonRpcCancellationToken, JsonRpcErrorObject, JsonRpcErrorResponse, JsonRpcId,
    JsonRpcNotification, JsonRpcRequest, JsonRpcSuccess, JsonRpcTransport, JsonRpcTransportError,
    JsonRpcVersion,
};
pub use pi::{
    parse_pi_timeout, pi_readiness_probe, pi_sidecar_config, pi_startup_timeout,
    validate_pi_session, PiRpcTransport, PiRpcWiring, PiSessionLocator, DEFAULT_PI_TIMEOUT,
    PI_FIRST_EVENT_TIMEOUT_ENV, PI_NPM_PACKAGE, PI_STARTUP_TIMEOUT_ENV, PI_VERSION,
};
pub use supervisor::{
    ProbeOutcome, RestartPolicy, SidecarConfig, SidecarEvent, SidecarEventCause, SidecarStatus,
    SidecarSupervisor, SECRET_VARIABLE_PREFIX,
};

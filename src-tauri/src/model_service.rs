//! The forward to the runtime's model service. Each Settings → Models command
//! sends one `models.request` over a runtime connection of its own, so a long
//! action such as the inventory, a classifier test or a sign-in start never
//! holds the shared desktop client. The runtime's refusal reaches the window
//! as the command's error text.

use std::time::Duration;

use muniment_core::attach::{ClientError, DesktopClient, ModelAction};
use serde_json::Value;
use tauri::Manager;

use crate::attach_service::{AttachCompanionState, DesktopClientSession};
use crate::auth;

/// How long a command waits for the runtime connection at window open.
const CONNECT_WAIT: Duration = Duration::from_secs(3);
/// The bound on connecting and on each frame of the handshake.
const IO_TIMEOUT: Duration = Duration::from_secs(5);

/// Runs one model service action in the selected runtime and answers its result.
pub(crate) async fn request<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    action: ModelAction,
    data: Value,
) -> Result<Value, String> {
    let runtime = app.state::<AttachCompanionState>().remote_runtime_id();
    request_on(app, runtime, action, data).await
}

/// Runs one model service action in the paired runtime `runtime`, or in this
/// computer's runtime for `None`, whichever runtime is selected.
pub(crate) async fn request_on<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    runtime: Option<String>,
    action: ModelAction,
    data: Value,
) -> Result<Value, String> {
    let waiting_app = app.clone();
    let session = tauri::async_runtime::spawn_blocking(move || {
        waiting_app
            .state::<AttachCompanionState>()
            .desktop_client_session_within(CONNECT_WAIT)
    })
    .await
    .map_err(|_| auth::background_service_error())?;
    if !matches!(session, DesktopClientSession::Connected(_)) {
        return Err(auth::background_service_error());
    }
    let connect_app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut client = connect_to(&connect_app, runtime.as_deref())
            .map_err(auth::desktop_client_error)?;
        client
            .models_request(action, data)
            .map_err(|error| auth::desktop_request_error(error, client.last_request_error()))
    })
    .await
    .map_err(|_| auth::background_service_error())?
}

/// Opens a connection of its own to the paired runtime `runtime`, else to
/// this computer's runtime.
fn connect_to<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    runtime: Option<&str>,
) -> Result<DesktopClient, ClientError> {
    let Some(id) = runtime else {
        return connect();
    };
    app.state::<AttachCompanionState>()
        .paired_connector(id)
        .ok_or(ClientError::DesktopUnavailable)?
        .connect()
        .map_err(|_| ClientError::DesktopUnavailable)
}

/// Opens a desktop client connection of its own to this computer's runtime.
#[cfg(unix)]
pub(crate) fn connect() -> Result<DesktopClient, ClientError> {
    let endpoint =
        crate::attach_service::runtime_profile_endpoint().ok_or(ClientError::DesktopUnavailable)?;
    muniment_core::attach::connect_desktop_client_at(
        &endpoint,
        env!("CARGO_PKG_VERSION"),
        IO_TIMEOUT,
    )
}

/// Opens a desktop client connection of its own to this computer's runtime.
#[cfg(target_os = "windows")]
pub(crate) fn connect() -> Result<DesktopClient, ClientError> {
    muniment_core::attach::connect_windows_desktop_client(
        env!("CARGO_PKG_VERSION"),
        IO_TIMEOUT,
        std::time::Instant::now() + IO_TIMEOUT,
    )
}

/// Builds a command's data object from its arguments, leaving out the
/// optional ones the window did not send.
pub(crate) fn data(fields: impl IntoIterator<Item = (&'static str, Value)>) -> Value {
    Value::Object(
        fields
            .into_iter()
            .filter(|(_, value)| !value.is_null())
            .map(|(name, value)| (name.to_owned(), value))
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_data_leaves_out_the_arguments_the_window_did_not_send() {
        assert_eq!(
            data([
                ("id", Value::String("a1".into())),
                ("label", Value::Null),
                ("weight", Value::from(3)),
            ]),
            serde_json::json!({"id": "a1", "weight": 3})
        );
    }
}

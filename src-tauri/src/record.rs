//! The record panel's commands: the companies a machine holds and one
//! company's kind catalogue, read from the runtime over the desktop client.

use std::time::Duration;

use crate::attach_service::{AttachCompanionState, DesktopClientSession};
use crate::auth;
use muniment_core::attach::ClientError;
use muniment_core::attach::DesktopClientHolder;
use serde_json::Value;

/// How long a record read waits for the runtime connection before it reports
/// the service unreachable.
const RECORD_CONNECT_WAIT: Duration = Duration::from_secs(3);

fn record_client_error(error: ClientError) -> String {
    match error {
        ClientError::RequestRejected => "The desktop rejected the record request.".to_owned(),
        ClientError::DesktopUnavailable
        | ClientError::ConnectionClosed
        | ClientError::Timeout
        | ClientError::DesktopBusy => auth::background_service_error(),
        other => other.to_string(),
    }
}

async fn with_desktop_client(
    app: tauri::AppHandle,
    call: impl FnOnce(&DesktopClientHolder) -> Result<Value, ClientError> + Send + 'static,
) -> Result<Value, String> {
    let session = tauri::async_runtime::spawn_blocking(move || {
        use tauri::Manager;
        app.state::<AttachCompanionState>()
            .desktop_client_session_within(RECORD_CONNECT_WAIT)
    })
    .await
    .map_err(|_| auth::background_service_error())?;
    match session {
        DesktopClientSession::Connected(client) => call(&client).map_err(record_client_error),
        DesktopClientSession::NoSupervisor | DesktopClientSession::Disconnected => {
            Err(auth::background_service_error())
        }
    }
}

/// Every company on this machine, oldest first, with the current one marked.
#[tauri::command]
pub async fn record_companies(app: tauri::AppHandle) -> Result<Value, String> {
    with_desktop_client(app, |client| client.list_companies()).await
}

#[tauri::command]
pub async fn record_company_create(app: tauri::AppHandle, name: String) -> Result<Value, String> {
    with_desktop_client(app, move |client| client.create_company(&name)).await
}

#[tauri::command]
pub async fn record_company_select(
    app: tauri::AppHandle,
    company_id: String,
) -> Result<Value, String> {
    with_desktop_client(app, move |client| client.select_company(&company_id)).await
}

#[tauri::command]
pub async fn record_company_rename(
    app: tauri::AppHandle,
    company_id: String,
    name: String,
) -> Result<Value, String> {
    with_desktop_client(app, move |client| client.rename_company(&company_id, &name)).await
}

/// The kind catalogue of the named company, or of the current one.
#[tauri::command]
pub async fn record_kinds(
    app: tauri::AppHandle,
    company_id: Option<String>,
) -> Result<Value, String> {
    with_desktop_client(app, move |client| {
        let mut body = serde_json::Map::new();
        if let Some(company_id) = company_id {
            body.insert("company_id".to_owned(), Value::String(company_id));
        }
        client.record_kinds(Value::Object(body))
    })
    .await
}

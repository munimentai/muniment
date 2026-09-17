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

#[tauri::command]
pub async fn record_company_delete(
    app: tauri::AppHandle,
    company_id: String,
) -> Result<Value, String> {
    with_desktop_client(app, move |client| client.delete_company(&company_id)).await
}

fn record_body(company_id: Option<String>, fields: Vec<(&str, Value)>) -> Value {
    let mut body = serde_json::Map::new();
    if let Some(company_id) = company_id {
        body.insert("company_id".to_owned(), Value::String(company_id));
    }
    for (key, value) in fields {
        if !value.is_null() {
            body.insert(key.to_owned(), value);
        }
    }
    Value::Object(body)
}

/// One page of a kind's rows.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn record_query(
    app: tauri::AppHandle,
    company_id: Option<String>,
    kind: String,
    limit: Option<u64>,
    offset: Option<u64>,
    sort: Option<String>,
    descending: Option<bool>,
    state: Option<String>,
    search: Option<String>,
) -> Result<Value, String> {
    let body = record_body(
        company_id,
        vec![
            ("kind", Value::String(kind)),
            ("limit", limit.map_or(Value::Null, Value::from)),
            ("offset", offset.map_or(Value::Null, Value::from)),
            ("sort", sort.map_or(Value::Null, Value::String)),
            ("descending", descending.map_or(Value::Null, Value::Bool)),
            ("state", state.map_or(Value::Null, Value::String)),
            ("search", search.map_or(Value::Null, Value::String)),
        ],
    );
    with_desktop_client(app, move |client| client.record_query(body)).await
}

/// One entity with its kind, identities, edges and events.
#[tauri::command]
pub async fn record_entity(
    app: tauri::AppHandle,
    company_id: Option<String>,
    entity: String,
) -> Result<Value, String> {
    let body = record_body(company_id, vec![("entity", Value::String(entity))]);
    with_desktop_client(app, move |client| client.record_entity(body)).await
}

/// Validates one change and returns its diff. The desktop acts as the owner.
#[tauri::command]
pub async fn record_propose(
    app: tauri::AppHandle,
    company_id: Option<String>,
    operation: Value,
) -> Result<Value, String> {
    let body = record_body(company_id, vec![("operation", operation)]);
    with_desktop_client(app, move |client| client.record_propose(body)).await
}

/// Applies one proposal as the owner.
#[tauri::command]
pub async fn record_commit(
    app: tauri::AppHandle,
    company_id: Option<String>,
    proposal: String,
) -> Result<Value, String> {
    let body = record_body(company_id, vec![("proposal", Value::String(proposal))]);
    with_desktop_client(app, move |client| client.record_commit(body)).await
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

/// One source object's fields and samples, so the panel maps it onto a kind.
/// `object` is the file path the dialog returned.
#[tauri::command]
pub async fn reader_describe(
    app: tauri::AppHandle,
    company_id: Option<String>,
    source: String,
    object: String,
) -> Result<Value, String> {
    let body = record_body(
        company_id,
        vec![
            ("source", Value::String(source)),
            ("object", Value::String(object)),
        ],
    );
    with_desktop_client(app, move |client| client.reader_describe(body)).await
}

/// Runs one committed mapping from `offset` for one budget. The answer's
/// `next_offset` and `done` drive the next call.
#[tauri::command]
pub async fn reader_run(
    app: tauri::AppHandle,
    company_id: Option<String>,
    mapping: String,
    offset: Option<u64>,
) -> Result<Value, String> {
    let body = record_body(
        company_id,
        vec![
            ("mapping", Value::String(mapping)),
            ("offset", offset.map_or(Value::Null, Value::from)),
        ],
    );
    with_desktop_client(app, move |client| client.reader_run(body)).await
}

/// The rows a mapping's last run could not place, with their reasons.
#[tauri::command]
pub async fn reader_queue(
    app: tauri::AppHandle,
    company_id: Option<String>,
    mapping: String,
) -> Result<Value, String> {
    let body = record_body(company_id, vec![("mapping", Value::String(mapping))]);
    with_desktop_client(app, move |client| client.reader_queue(body)).await
}

/// The objects a connected source holds, or the failure that names what to do.
#[tauri::command]
pub async fn reader_objects(
    app: tauri::AppHandle,
    company_id: Option<String>,
    source: String,
) -> Result<Value, String> {
    let body = record_body(company_id, vec![("source", Value::String(source))]);
    with_desktop_client(app, move |client| client.reader_objects(body)).await
}

/// Stores a source's secret after the runtime proves it with one read.
#[tauri::command]
pub async fn reader_connect(
    app: tauri::AppHandle,
    company_id: Option<String>,
    source: String,
    secret: String,
) -> Result<Value, String> {
    let body = record_body(
        company_id,
        vec![
            ("source", Value::String(source)),
            ("secret", Value::String(secret)),
        ],
    );
    with_desktop_client(app, move |client| client.reader_connect(body)).await
}

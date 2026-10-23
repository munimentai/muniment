//! Settings → Models, the router half: the account pools, the routes, the
//! classifier and its connections. The runtime runs the router and keeps its
//! record. Each command here is a forward to the runtime's model service, and
//! the settings it answers carry an account's source and counters, never its
//! key.

use muniment_core::attach::ModelAction;
use muniment_core::model_router::config::Route;
use serde_json::{json, Value};

use crate::model_service::{self, data};

#[tauri::command]
pub(crate) async fn model_router_settings(app: tauri::AppHandle) -> Result<Value, String> {
    model_service::request(&app, ModelAction::RouterSettings, json!({})).await
}

#[tauri::command]
pub(crate) async fn model_router_set_enabled(
    app: tauri::AppHandle,
    enabled: bool,
) -> Result<Value, String> {
    model_service::request(
        &app,
        ModelAction::RouterSetEnabled,
        data([("enabled", json!(enabled))]),
    )
    .await
}

#[tauri::command]
pub(crate) async fn model_router_add_account(
    app: tauri::AppHandle,
    family: String,
    label: String,
    key: String,
    base_url: Option<String>,
    models: Option<Vec<String>>,
) -> Result<Value, String> {
    model_service::request(
        &app,
        ModelAction::RouterAddAccount,
        data([
            ("family", json!(family)),
            ("label", json!(label)),
            ("key", json!(key)),
            ("base_url", json!(base_url)),
            ("models", json!(models)),
        ]),
    )
    .await
}

#[tauri::command]
pub(crate) async fn model_router_update_account(
    app: tauri::AppHandle,
    id: String,
    label: Option<String>,
    enabled: Option<bool>,
    weight: Option<u32>,
    models: Option<Vec<String>>,
    overflow: Option<bool>,
) -> Result<Value, String> {
    model_service::request(
        &app,
        ModelAction::RouterUpdateAccount,
        data([
            ("id", json!(id)),
            ("label", json!(label)),
            ("enabled", json!(enabled)),
            ("weight", json!(weight)),
            ("models", json!(models)),
            ("overflow", json!(overflow)),
        ]),
    )
    .await
}

#[tauri::command]
pub(crate) async fn model_router_remove_account(
    app: tauri::AppHandle,
    id: String,
) -> Result<Value, String> {
    model_service::request(
        &app,
        ModelAction::RouterRemoveAccount,
        data([("id", json!(id))]),
    )
    .await
}

/// Keeps the models the picker hides out of the running, so a routed turn
/// answers only from a model the picker offers. Each key is `family/model`.
#[tauri::command]
pub(crate) async fn model_router_set_hidden(
    app: tauri::AppHandle,
    keys: Vec<String>,
) -> Result<Value, String> {
    model_service::request(
        &app,
        ModelAction::RouterSetHidden,
        data([("keys", json!(keys))]),
    )
    .await
}

#[tauri::command]
pub(crate) async fn model_router_save_routes(
    app: tauri::AppHandle,
    routes: Vec<Route>,
    fallback: Option<String>,
    min_confidence: Option<f64>,
) -> Result<Value, String> {
    model_service::request(
        &app,
        ModelAction::RouterSaveRoutes,
        data([
            ("routes", json!(routes)),
            ("fallback", json!(fallback)),
            ("min_confidence", json!(min_confidence)),
        ]),
    )
    .await
}

#[tauri::command]
pub(crate) async fn model_router_set_classifier(
    app: tauri::AppHandle,
    kind: String,
    api_key: Option<String>,
    model: Option<String>,
    base_url: Option<String>,
    family: Option<String>,
) -> Result<Value, String> {
    model_service::request(
        &app,
        ModelAction::RouterSetClassifier,
        data([
            ("kind", json!(kind)),
            ("api_key", json!(api_key)),
            ("model", json!(model)),
            ("base_url", json!(base_url)),
            ("family", json!(family)),
        ]),
    )
    .await
}

#[tauri::command]
pub(crate) async fn model_router_test_classifier(app: tauri::AppHandle) -> Result<(), String> {
    model_service::request(&app, ModelAction::RouterTestClassifier, json!({}))
        .await
        .map(drop)
}

#[tauri::command]
pub(crate) async fn model_router_test_route(
    app: tauri::AppHandle,
    sample: String,
) -> Result<Value, String> {
    model_service::request(
        &app,
        ModelAction::RouterTestRoute,
        data([("sample", json!(sample))]),
    )
    .await
}

/// A first sign-in may install Pi, and the runtime answers once it starts.
#[tauri::command]
pub(crate) async fn model_router_subscription_start(
    app: tauri::AppHandle,
    provider: String,
    runtime: Option<String>,
) -> Result<(), String> {
    crate::account_login::start_sign_in(
        &app,
        ModelAction::RouterSubscriptionStart,
        provider,
        runtime,
    )
    .await
}

#[tauri::command]
pub(crate) async fn model_router_refresh_quota(
    app: tauri::AppHandle,
    id: Option<String>,
) -> Result<Value, String> {
    model_service::request(
        &app,
        ModelAction::RouterRefreshQuota,
        data([("id", json!(id))]),
    )
    .await
}

/// Turns decision-model assistance on or off and names the connection it
/// asks. An empty id uses the model routing decision model.
#[tauri::command]
pub(crate) async fn model_router_set_assist(
    app: tauri::AppHandle,
    enabled: bool,
    id: String,
) -> Result<Value, String> {
    model_service::request(
        &app,
        ModelAction::RouterSetAssist,
        data([("enabled", json!(enabled)), ("id", json!(id))]),
    )
    .await
}

#[tauri::command]
pub(crate) async fn model_router_connect_classifier(
    app: tauri::AppHandle,
    catalog_id: String,
    name: String,
    model: String,
    base_url: String,
    api_key: Option<String>,
    key_provider: Option<String>,
) -> Result<Value, String> {
    model_service::request(
        &app,
        ModelAction::RouterConnectClassifier,
        data([
            ("catalog_id", json!(catalog_id)),
            ("name", json!(name)),
            ("model", json!(model)),
            ("base_url", json!(base_url)),
            ("api_key", json!(api_key)),
            ("key_provider", json!(key_provider)),
        ]),
    )
    .await
}

/// Renames a saved connection or changes where it sends requests. The
/// runtime tests changed details before it saves them.
#[tauri::command]
pub(crate) async fn model_router_update_classifier(
    app: tauri::AppHandle,
    id: String,
    name: String,
    model: String,
    base_url: String,
    api_key: Option<String>,
) -> Result<Value, String> {
    model_service::request(
        &app,
        ModelAction::RouterUpdateClassifier,
        data([
            ("id", json!(id)),
            ("name", json!(name)),
            ("model", json!(model)),
            ("base_url", json!(base_url)),
            ("api_key", json!(api_key)),
        ]),
    )
    .await
}

#[tauri::command]
pub(crate) async fn model_router_select_classifier(
    app: tauri::AppHandle,
    id: String,
) -> Result<Value, String> {
    model_service::request(
        &app,
        ModelAction::RouterSelectClassifier,
        data([("id", json!(id))]),
    )
    .await
}

#[tauri::command]
pub(crate) async fn model_router_disconnect_classifier(
    app: tauri::AppHandle,
    id: String,
) -> Result<Value, String> {
    model_service::request(
        &app,
        ModelAction::RouterDisconnectClassifier,
        data([("id", json!(id))]),
    )
    .await
}

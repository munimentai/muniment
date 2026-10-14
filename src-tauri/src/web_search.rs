//! Settings → Search: the order agents try web searches in, the engines the
//! built-in search asks, and the keys of the providers that need one. The
//! runtime's model service keeps the record, and each command here forwards
//! one `search_` action to it. Every answer carries a key's last four
//! characters at most, never the key.

use std::collections::BTreeMap;

use muniment_core::attach::ModelAction;
use muniment_core::search_settings::OrderEntry;
use serde_json::{json, Value};

use crate::model_service::{self, data};

#[tauri::command]
pub(crate) async fn web_search_settings(app: tauri::AppHandle) -> Result<Value, String> {
    model_service::request(&app, ModelAction::SearchSettings, json!({})).await
}

#[tauri::command]
pub(crate) async fn web_search_save_order(
    app: tauri::AppHandle,
    order: Vec<OrderEntry>,
) -> Result<Value, String> {
    model_service::request(&app, ModelAction::SearchSaveOrder, order_data(order)).await
}

#[tauri::command]
pub(crate) async fn web_search_save_engines(
    app: tauri::AppHandle,
    engines: Vec<String>,
) -> Result<Value, String> {
    model_service::request(
        &app,
        ModelAction::SearchSaveEngines,
        data([("engines", json!(engines))]),
    )
    .await
}

#[tauri::command]
pub(crate) async fn web_search_set_key(
    app: tauri::AppHandle,
    provider: String,
    api_key: String,
    base_url: Option<String>,
    extra: Option<BTreeMap<String, String>>,
) -> Result<Value, String> {
    model_service::request(
        &app,
        ModelAction::SearchSetKey,
        key_data(provider, api_key, base_url, extra),
    )
    .await
}

#[tauri::command]
pub(crate) async fn web_search_remove_key(
    app: tauri::AppHandle,
    provider: String,
) -> Result<Value, String> {
    model_service::request(
        &app,
        ModelAction::SearchRemoveKey,
        data([("provider", json!(provider))]),
    )
    .await
}

fn order_data(order: Vec<OrderEntry>) -> Value {
    data([("order", json!(order))])
}

fn key_data(
    provider: String,
    api_key: String,
    base_url: Option<String>,
    extra: Option<BTreeMap<String, String>>,
) -> Value {
    data([
        ("provider", json!(provider)),
        ("api_key", json!(api_key)),
        ("base_url", json!(base_url)),
        ("extra", json!(extra)),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_order_carries_only_each_entry_id_and_state() {
        let order = vec![
            OrderEntry {
                id: "tavily".into(),
                enabled: true,
            },
            OrderEntry {
                id: "muniment".into(),
                enabled: false,
            },
        ];
        assert_eq!(
            order_data(order),
            json!({"order": [
                {"id": "tavily", "enabled": true},
                {"id": "muniment", "enabled": false},
            ]})
        );
    }

    #[test]
    fn a_key_leaves_out_the_base_url_and_fields_the_window_did_not_send() {
        assert_eq!(
            key_data("tavily".into(), "test-key".into(), None, None),
            json!({"provider": "tavily", "api_key": "test-key"})
        );
        let extra = BTreeMap::from([("brightdataSerpZone".to_string(), "serp".to_string())]);
        assert_eq!(
            key_data(
                "brightdata".into(),
                "test-key".into(),
                Some("https://search.example".into()),
                Some(extra),
            ),
            json!({
                "provider": "brightdata",
                "api_key": "test-key",
                "base_url": "https://search.example",
                "extra": {"brightdataSerpZone": "serp"},
            })
        );
    }
}

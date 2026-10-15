//! Settings → Profile and Memory. The runtime owns the memory files, and each
//! command here forwards one `memory_` action to its model service. A fact
//! saved here is the user's own fact.

use muniment_core::attach::ModelAction;
use muniment_core::memory_files::{Fact, RecallSettings};
use serde_json::{json, Value};

use crate::model_service;

#[tauri::command]
pub async fn memory_profile_read(app: tauri::AppHandle) -> Result<Value, String> {
    model_service::request(&app, ModelAction::MemoryProfileRead, json!({})).await
}
#[tauri::command]
pub async fn memory_profile_save(app: tauri::AppHandle, content: String) -> Result<Value, String> {
    model_service::request(
        &app,
        ModelAction::MemoryProfileSave,
        json!({"content": content}),
    )
    .await
}
#[tauri::command]
pub async fn memory_facts(app: tauri::AppHandle) -> Result<Value, String> {
    model_service::request(&app, ModelAction::MemoryFacts, json!({})).await
}
#[tauri::command]
pub async fn memory_fact_save(app: tauri::AppHandle, fact: Fact) -> Result<Value, String> {
    model_service::request(&app, ModelAction::MemoryFactSave, json!({"fact": fact})).await
}
#[tauri::command]
pub async fn memory_fact_delete(app: tauri::AppHandle, id: String) -> Result<Value, String> {
    model_service::request(&app, ModelAction::MemoryFactDelete, json!({"id": id})).await
}
#[tauri::command]
pub async fn memory_deleted_facts(app: tauri::AppHandle) -> Result<Value, String> {
    model_service::request(&app, ModelAction::MemoryDeletedFacts, json!({})).await
}
#[tauri::command]
pub async fn memory_fact_restore(app: tauri::AppHandle, id: String) -> Result<Value, String> {
    model_service::request(&app, ModelAction::MemoryFactRestore, json!({"id": id})).await
}
#[tauri::command]
pub async fn memory_recall_settings(app: tauri::AppHandle) -> Result<Value, String> {
    model_service::request(&app, ModelAction::MemoryRecallSettings, json!({})).await
}
#[tauri::command]
pub async fn memory_recall_settings_save(
    app: tauri::AppHandle,
    settings: RecallSettings,
) -> Result<Value, String> {
    model_service::request(
        &app,
        ModelAction::MemoryRecallSettingsSave,
        json!({"settings": settings}),
    )
    .await
}

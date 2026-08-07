#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod attach_service;
mod auth;
mod chat;
mod chat_coordinate;
mod chat_threads;
mod dictation;
mod home;
mod memory;
mod model_install;
mod onboarding_import;
#[cfg(test)]
mod test_support;
mod voice_capture;

use muniment_core::attach::RuntimeActivityRegistry;
use std::sync::Arc;
use tauri::Manager;

fn main() {
    let runtime_activity = RuntimeActivityRegistry::new();
    let builder = tauri::Builder::default().manage(runtime_activity.clone());
    #[cfg(feature = "e2e-webdriver")]
    let builder = builder.plugin(tauri_plugin_wdio_webdriver::init());

    builder
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_window_state::Builder::default().build())
        .manage(auth::AuthState::new())
        .manage(Arc::new(voice_capture::VoiceCaptureState::new()))
        .manage(attach_service::AttachApprovalState::default())
        .setup(move |app| {
            let app_data = app.path().app_data_dir()?;
            let app_config = app.path().app_config_dir()?;
            app.manage(memory::ApplicationMemoryRuntime::new(
                app_config,
                app_data.join("memory"),
            ));
            app.manage(chat::ChatState::new(
                app.handle(),
                runtime_activity.clone(),
            )?);
            #[cfg(target_os = "linux")]
            attach_service::start_attach_listener(app.handle().clone());
            #[cfg(not(target_os = "linux"))]
            app.manage(attach_service::AttachCompanionState::default());
            let parakeet_root = app.path().app_data_dir()?.join("models").join("parakeet");
            app.manage(model_install::ParakeetInstallState::new(
                parakeet_root.clone(),
            )?);
            app.manage(dictation::DictationState::new(parakeet_root));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            auth::auth_sign_in,
            auth::auth_status,
            auth::auth_entitlement_snapshot,
            auth::auth_devices,
            auth::auth_sign_out,
            chat::chat_submit,
            chat::chat_file_metadata,
            chat::chat_resume,
            chat::chat_cancel,
            chat::chat_answer_permission,
            chat::chat_queue,
            memory::memory_tools,
            memory::memory_tool_call,
            chat_threads::chat_current_thread,
            chat_threads::chat_thread_summaries,
            chat_threads::chat_thread_open,
            chat_threads::chat_select_thread,
            chat_threads::chat_rename_thread,
            chat_threads::chat_delete_thread,
            chat_threads::chat_new_thread,
            attach_service::attach_pairing_decide,
            attach_service::attach_companions,
            attach_service::attach_revoke_companion,
            home::home_status,
            home::home_confirm,
            home::home_confirm_import,
            onboarding_import::onboarding_import_preview,
            onboarding_import::onboarding_import_extract,
            model_install::parakeet_install_facts,
            model_install::parakeet_install_start,
            model_install::parakeet_install_status,
            model_install::parakeet_install_cancel,
            dictation::dictation_start,
            dictation::dictation_stop,
            dictation::dictation_status
        ])
        .run(tauri::generate_context!())
        .expect("error while running muniment");
}

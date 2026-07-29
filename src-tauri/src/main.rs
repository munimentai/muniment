#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod attach_service;
mod auth;
mod chat;
mod chat_threads;
mod dictation;
mod home;
mod model_install;
mod onboarding_import;
mod session_thread;
#[cfg(test)]
mod test_support;
mod voice_capture;

use std::sync::Arc;
use tauri::Manager;

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_window_state::Builder::default().build())
        .manage(auth::AuthState::new())
        .manage(Arc::new(voice_capture::VoiceCaptureState::new()))
        .manage(attach_service::AttachApprovalState::default())
        .setup(|app| {
            app.manage(chat::ChatState::new(app.handle())?);
            #[cfg(target_os = "linux")]
            attach_service::start_attach_listener(app.handle().clone());
            let model_root = app.path().app_data_dir()?.join("models").join("qwen3.5-4b");
            let required_model = model_install::ResidentModelInstallState::new(model_root)?;
            // Required acquisition is deliberately detached from onboarding: folder and
            // consent steps remain interactive while this worker downloads and activates AI.
            required_model.start();
            app.manage(required_model);
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
            auth::auth_ensure_fresh,
            auth::auth_sign_out,
            chat::chat_submit,
            chat::chat_file_metadata,
            chat::chat_resume,
            chat::chat_cancel,
            chat::chat_answer_permission,
            chat::chat_queue,
            chat_threads::chat_current_thread,
            chat_threads::chat_thread_summaries,
            chat_threads::chat_thread_open,
            chat_threads::chat_select_thread,
            chat_threads::chat_rename_thread,
            chat_threads::chat_new_thread,
            attach_service::attach_pairing_decide,
            model_install::required_model_acquisition_status,
            model_install::dictation_polish,
            model_install::dictation_transform,
            model_install::onboarding_triage,
            home::home_status,
            home::home_confirm,
            home::home_confirm_import,
            onboarding_import::onboarding_import_preview,
            onboarding_import::onboarding_import_extract,
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

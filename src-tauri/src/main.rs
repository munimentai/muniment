#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod auth;
mod chat;
mod dictation;
mod model_install;
mod onboarding;
mod voice_capture;

use std::sync::Arc;
use tauri::Manager;

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(auth::AuthState::new())
        .manage(Arc::new(voice_capture::VoiceCaptureState::new()))
        .setup(|app| {
            app.manage(chat::ChatState::new(app.handle())?);
            let model_root = app.path().app_data_dir()?.join("models").join("qwen3.5-4b");
            let required_model = model_install::GemmaInstallState::new(model_root)?;
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
            chat::chat_queue,
            chat::chat_history,
            model_install::gemma_install_start,
            model_install::gemma_install_status,
            model_install::gemma_install_cancel,
            model_install::required_model_acquisition_status,
            onboarding::onboarding_status,
            onboarding::onboarding_propose,
            onboarding::onboarding_confirm,
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

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod auth;
mod chat;
mod model_install;

use tauri::Manager;

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(auth::AuthState::new())
        .setup(|app| {
            app.manage(chat::ChatState::new(app.handle())?);
            let model_root = app.path().app_data_dir()?.join("models").join("gemma");
            app.manage(model_install::GemmaInstallState::new(model_root)?);
            let parakeet_root = app.path().app_data_dir()?.join("models").join("parakeet");
            app.manage(model_install::ParakeetInstallState::new(parakeet_root)?);
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
            chat::chat_resume,
            chat::chat_cancel,
            chat::chat_queue,
            chat::chat_history,
            model_install::gemma_install_start,
            model_install::gemma_install_status,
            model_install::gemma_install_cancel,
            model_install::parakeet_install_start,
            model_install::parakeet_install_status,
            model_install::parakeet_install_cancel
        ])
        .run(tauri::generate_context!())
        .expect("error while running muniment");
}

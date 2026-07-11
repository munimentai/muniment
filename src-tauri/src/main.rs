#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod auth;
mod chat;

use tauri::Manager;

fn main() {
    tauri::Builder::default()
        .manage(auth::AuthState::new())
        .setup(|app| {
            app.manage(chat::ChatState::new(app.handle())?);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            auth::auth_sign_in,
            auth::auth_status,
            auth::auth_ensure_fresh,
            auth::auth_sign_out,
            chat::chat_submit,
            chat::chat_cancel,
            chat::chat_history
        ])
        .run(tauri::generate_context!())
        .expect("error while running muniment");
}

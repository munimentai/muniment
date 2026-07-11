#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod auth;
mod chat;

fn main() {
    tauri::Builder::default()
        .manage(auth::AuthState::new())
        .invoke_handler(tauri::generate_handler![
            auth::auth_sign_in,
            auth::auth_status,
            auth::auth_ensure_fresh,
            auth::auth_sign_out,
            chat::chat_send
        ])
        .run(tauri::generate_context!())
        .expect("error while running muniment");
}

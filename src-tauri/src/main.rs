#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod auth;

fn main() {
    tauri::Builder::default()
        .manage(auth::AuthState::new())
        .invoke_handler(tauri::generate_handler![
            auth::auth_sign_in,
            auth::auth_status,
            auth::auth_sign_out
        ])
        .run(tauri::generate_context!())
        .expect("error while running muniment");
}

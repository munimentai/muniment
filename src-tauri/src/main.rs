#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod account_login;
mod attach_service;
mod auth;
mod chat;
mod chat_threads;
mod dictation;
#[cfg(all(target_os = "macos", feature = "e2e-webdriver"))]
mod e2e_folder_dialog;
mod home;
mod launcher;
#[cfg(target_os = "linux")]
mod linux_runtime_service;
mod local_mode;
#[cfg(any(target_os = "macos", all(test, unix)))]
mod macos_run_start_probe;
#[cfg(target_os = "macos")]
mod macos_runtime_notice_probe;
#[cfg(any(target_os = "macos", all(test, unix)))]
mod macos_runtime_service;
mod memory;
mod model_install;
mod onboarding_diagnostics;
mod onboarding_import;
mod onboarding_scan;
mod record;
mod runtime_owner;
#[cfg(test)]
mod test_support;
mod thread_retention;
mod voice_capture;
mod window_state;
#[cfg(any(target_os = "windows", all(test, unix)))]
mod windows_runtime_service;

use muniment_core::attach::{DrainState, PreparedHandoffSlot, RuntimeActivityRegistry};
use std::sync::{Arc, Mutex};
use tauri::Manager;

#[tauri::command]
fn restart_muniment(app: tauri::AppHandle) {
    app.restart();
}

fn main() {
    #[cfg(target_os = "macos")]
    {
        let mut args = std::env::args_os().skip(1);
        if args.next().as_deref() == Some(std::ffi::OsStr::new("--probe-local-run")) {
            let result = match (args.next(), args.next()) {
                (Some(endpoint), None) => {
                    macos_run_start_probe::run(std::path::Path::new(&endpoint))
                }
                _ => Err("The run-start probe requires one attach socket path.".into()),
            };
            match result {
                Ok(run_id) => println!("{run_id}"),
                Err(message) => {
                    eprintln!("{message}");
                    std::process::exit(1);
                }
            }
            return;
        }
    }
    let runtime_activity = RuntimeActivityRegistry::new();
    let builder = tauri::Builder::default()
        .manage(runtime_activity.clone())
        .manage(DrainState::new())
        .manage(Mutex::new(PreparedHandoffSlot::new()));
    #[cfg(feature = "e2e-webdriver")]
    let builder = builder.plugin(tauri_plugin_wdio_webdriver::init());

    builder
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_opener::init())
        .plugin(
            tauri_plugin_window_state::Builder::default()
                .skip_initial_state("main")
                .with_denylist(&["launcher"])
                .build(),
        )
        .manage(auth::AuthState::new(runtime_activity.clone()))
        .manage(Arc::new(voice_capture::VoiceCaptureState::new()))
        .manage(attach_service::AttachApprovalState::default())
        .on_page_load(|webview, payload| {
            #[cfg(target_os = "macos")]
            if payload.event() == tauri::webview::PageLoadEvent::Finished {
                macos_runtime_notice_probe::install(webview);
            }
            #[cfg(not(target_os = "macos"))]
            let _ = (webview, payload);
        })
        .setup(move |app| {
            // Every file the app keeps lives under one root, and an earlier
            // install's files move there before anything opens them.
            let state = muniment_runtime::adopt_state_directory()?;
            window_state::restore_main_window(app)?;
            #[cfg(target_os = "macos")]
            launcher::setup(app.handle())?;
            let memory_runtime = Arc::new(memory::ApplicationMemoryRuntime::new(
                state.clone(),
                state.join("memory"),
            ));
            app.manage(Arc::clone(&memory_runtime));
            #[cfg(unix)]
            {
                app.manage(chat::ChatState::new(runtime_activity.clone()));
            }
            #[cfg(target_os = "windows")]
            {
                app.manage(chat::ChatState::new(runtime_activity.clone()));
            }
            runtime_owner::setup(app.handle());
            app.manage(account_login::AccountLoginState::default());
            let parakeet_root = state.join("models").join("parakeet");
            app.manage(model_install::ParakeetInstallState::new(
                parakeet_root.clone(),
            )?);
            app.manage(dictation::DictationState::new(parakeet_root));
            Ok(())
        })
        .on_window_event(launcher::window_event)
        .invoke_handler(tauri::generate_handler![
            onboarding_diagnostics::onboarding_model_settings_error,
            auth::auth_sign_in,
            auth::auth_status,
            auth::auth_entitlement_snapshot,
            auth::auth_devices,
            auth::auth_sign_out,
            local_mode::local_mode_status,
            local_mode::local_mode_enter,
            local_mode::local_mode_leave,
            local_mode::local_mode_provider_status,
            local_mode::local_mode_store_provider_key,
            local_mode::local_mode_store_local_provider,
            local_mode::local_mode_provider_inventory,
            local_mode::local_mode_set_default_model,
            local_mode::local_mode_set_model_hidden,
            local_mode::local_mode_disconnect_provider,
            local_mode::local_mode_store_endpoint,
            local_mode::local_mode_claude_code_status,
            local_mode::local_mode_connect_claude_code,
            account_login::local_mode_account_login_start,
            account_login::local_mode_account_login_answer,
            account_login::local_mode_account_login_cancel,
            account_login::local_mode_open_url,
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
            chat_threads::chat_delete_thread,
            chat_threads::chat_new_thread,
            thread_retention::thread_retention_choice,
            thread_retention::record_thread_retention_choice,
            attach_service::attach_pairing_decide,
            attach_service::attach_companions,
            attach_service::attach_listener_status,
            attach_service::attach_revoke_companion,
            record::record_companies,
            record::record_company_create,
            record::record_company_select,
            record::record_company_rename,
            record::record_kinds,
            record::record_query,
            record::record_entity,
            record::record_propose,
            record::record_commit,
            restart_muniment,
            launcher::launcher_register,
            launcher::launcher_start_failed,
            launcher::launcher_open,
            launcher::launcher_close,
            launcher::launcher_is_visible,
            launcher::launcher_present_main,
            home::home_status,
            home::home_confirm,
            home::home_confirm_import,
            onboarding_import::onboarding_import_preview,
            onboarding_import::onboarding_import_extract,
            onboarding_scan::onboarding_scan,
            model_install::parakeet_install_facts,
            model_install::parakeet_install_start,
            model_install::parakeet_install_status,
            model_install::parakeet_install_cancel,
            dictation::dictation_start,
            dictation::dictation_stop,
            dictation::dictation_status,
            runtime_owner::runtime_state,
            runtime_owner::runtime_start,
            runtime_owner::runtime_stop,
            #[cfg(target_os = "macos")]
            macos_runtime_service::open_login_items,
            #[cfg(target_os = "macos")]
            macos_runtime_notice_probe::runtime_notice_observed,
            #[cfg(all(target_os = "macos", feature = "e2e-webdriver"))]
            e2e_folder_dialog::e2e_drive_folder_dialog,
            #[cfg(all(target_os = "macos", feature = "e2e-webdriver"))]
            e2e_folder_dialog::e2e_folder_dialog_snapshot
        ])
        .build(tauri::generate_context!())
        .expect("error while running muniment")
        .run(|app, event| {
            // A runtime the desktop runs as its child exits with the desktop.
            #[cfg(target_os = "macos")]
            if let tauri::RunEvent::Exit = event {
                let _ = app.state::<runtime_owner::RuntimeOwner>().stop_child();
            }
            #[cfg(not(target_os = "macos"))]
            let _ = (app, event);
        });
}

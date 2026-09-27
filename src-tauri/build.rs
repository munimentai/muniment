use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

mod asr_rpath;

use asr_rpath::ExecutableLocation;

const ROOT: &str = "third-party/sherpa-onnx-v1.13.2";

/// Every command in the desktop invoke handler. Each one gets an `allow-*`
/// permission, and a window reaches a command only through a capability in
/// `capabilities/` that names that permission.
const APP_COMMANDS: &[&str] = &[
    "extend_command",
    "creation_list",
    "creation_save",
    "creation_delete",
    "artifact_edit",
    "workspace_file_action",
    "workspace_reveal",
    "workspace_folders",
    "workspace_list",
    "workspace_open",
    "workspace_image",
    "workspace_save_link",
    "workspace_read_text",
    "workspace_save_text",
    "terminal_start",
    "terminal_read",
    "terminal_write",
    "terminal_resize",
    "terminal_close",
    "browser_command",
    "browser_view",
    "artifact_list",
    "artifact_from_file",
    "artifact_read",
    "onboarding_model_settings_error",
    "installed_fonts",
    "auth_sign_in",
    "auth_status",
    "auth_entitlement_snapshot",
    "auth_devices",
    "auth_pairing_challenge",
    "auth_pairing_status",
    "auth_pairing_revoke",
    "auth_sign_out",
    "local_mode_status",
    "local_mode_enter",
    "local_mode_leave",
    "local_mode_provider_status",
    "local_mode_store_provider_key",
    "local_mode_store_local_provider",
    "local_mode_provider_inventory",
    "local_mode_set_default_model",
    "local_mode_set_model_hidden",
    "local_mode_disconnect_provider",
    "local_mode_store_endpoint",
    "local_mode_claude_code_status",
    "local_mode_connect_claude_code",
    "model_router_settings",
    "model_router_set_enabled",
    "model_router_add_account",
    "model_router_update_account",
    "model_router_remove_account",
    "model_router_save_routes",
    "model_router_set_classifier",
    "model_router_connect_classifier",
    "model_router_select_classifier",
    "model_router_disconnect_classifier",
    "model_router_test_classifier",
    "model_router_test_route",
    "model_router_subscription_start",
    "model_router_refresh_quota",
    "local_mode_account_login_start",
    "local_mode_account_login_answer",
    "local_mode_account_login_cancel",
    "local_mode_open_url",
    "chat_submit",
    "chat_file_metadata",
    "chat_file_content",
    "chat_search_files",
    "context_settings",
    "context_settings_save",
    "chat_resume",
    "chat_cancel",
    "chat_answer_permission",
    "chat_queue",
    "chat_current_thread",
    "chat_thread_summaries",
    "chat_thread_open",
    "chat_select_thread",
    "chat_rename_thread",
    "chat_delete_thread",
    "chat_new_thread",
    "thread_retention_choice",
    "record_thread_retention_choice",
    "attach_pairing_decide",
    "attach_companions",
    "attach_listener_status",
    "attach_revoke_companion",
    "record_companies",
    "record_company_create",
    "record_company_select",
    "record_company_rename",
    "record_company_delete",
    "record_kinds",
    "record_report",
    "record_query",
    "record_entity",
    "record_propose",
    "record_commit",
    "reader_describe",
    "reader_run",
    "reader_queue",
    "reader_objects",
    "reader_connect",
    "restart_muniment",
    "chat_pick_attachments",
    "app_update_prepare",
    "app_update_install",
    "launcher_register",
    "launcher_start_failed",
    "launcher_open",
    "launcher_close",
    "launcher_is_visible",
    "launcher_present_main",
    "agent_list",
    "agent_memory",
    "agent_export_template",
    "agent_import_link",
    "agent_save",
    "agent_delete",
    "agent_open",
    "agent_run",
    "memory_profile_read",
    "memory_profile_save",
    "memory_facts",
    "memory_fact_save",
    "memory_fact_delete",
    "memory_deleted_facts",
    "memory_fact_restore",
    "project_list",
    "project_create",
    "project_rename",
    "project_open",
    "home_status",
    "home_confirm",
    "home_confirm_import",
    "onboarding_import_preview",
    "onboarding_import_extract",
    "onboarding_scan",
    "parakeet_install_facts",
    "parakeet_install_start",
    "parakeet_install_status",
    "parakeet_install_cancel",
    "dictation_start",
    "dictation_stop",
    "dictation_status",
    "runtime_state",
    "runtime_start",
    "runtime_stop",
    "open_login_items",
    "runtime_notice_observed",
    "subscription_probe_observed",
    "subscription_probe_update",
    "e2e_drive_folder_dialog",
    "e2e_folder_dialog_snapshot",
];

fn main() {
    // Bind installed diagnostics to the source that built the signed executable.
    let git = |args: &[&str]| {
        std::process::Command::new("git")
            .args(args)
            .output()
            .ok()
            .filter(|output| output.status.success())
            .and_then(|output| String::from_utf8(output.stdout).ok())
            .map(|text| text.trim().to_owned())
    };
    for name in [
        Some("HEAD".to_owned()),
        git(&["symbolic-ref", "-q", "HEAD"]),
    ]
    .into_iter()
    .flatten()
    {
        if let Some(file) = git(&["rev-parse", "--git-path", &name]) {
            println!("cargo:rerun-if-changed={file}");
        }
    }
    let source = git(&["rev-parse", "HEAD"])
        .filter(|sha| sha.len() == 40 && sha.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .unwrap_or_else(|| "unavailable".into());
    println!("cargo:rustc-env=MUNIMENT_BUILD_SOURCE_SHA={source}");
    println!("cargo:rerun-if-env-changed=MUNIMENT_UPDATER_PUBLIC_KEY");
    println!("cargo:rerun-if-env-changed=MUNIMENT_UPDATER_ENDPOINT");
    let os = std::env::var("CARGO_CFG_TARGET_OS").expect("target OS is set by Cargo");
    if os == "linux" {
        println!("cargo:rustc-link-arg=-Wl,-rpath,$ORIGIN");
        println!("cargo:rustc-link-arg=-Wl,-rpath,$ORIGIN/../lib/muniment/cef");
        // Keep bundled native libraries out of the ELF dynamic symbol table.
        // NSS and WebKit use the system SQLite ABI, not rusqlite's bundled ABI.
        println!("cargo:rustc-link-arg=-Wl,--exclude-libs,ALL");
    }
    let arch = std::env::var("CARGO_CFG_TARGET_ARCH").expect("target arch is set by Cargo");
    let runtime = match (os.as_str(), arch.as_str()) {
        ("linux", "x86_64") => "linux-x86_64",
        ("windows", "x86_64") => "windows-x86_64",
        ("macos", "x86_64" | "aarch64") => "macos-universal2",
        _ => panic!(
            "unsupported desktop ASR target: {os}-{arch}; supported targets are macOS universal2, Windows x86_64, and Linux x86_64"
        ),
    };

    let required_runtime: &[(&str, &str)] = match os.as_str() {
        "linux" => &[
            (
                "libsherpa-onnx-c-api.so",
                "33e09c24dabb94f749cae7f97d9dd3317944695dd68a6c09d316a863b81ea011",
            ),
            (
                "libonnxruntime.so",
                "88468d42d3381c18a7bd3f99a01f07293ff8c9e38f71fd5bcc5fa08c101d31bf",
            ),
        ],
        "macos" => &[
            (
                "libsherpa-onnx-c-api.dylib",
                "b7b0a34667834cb03a227d13130a08e990204fcc8dd9a5c9d532224266a18afd",
            ),
            (
                "libonnxruntime.1.24.4.dylib",
                "e9a9534fc92910d9bd6ffd155c13ce7920417c652c5c1920178520880627513e",
            ),
        ],
        "windows" => &[
            (
                "sherpa-onnx-c-api.dll",
                "ee59933bb110fe8badf886a85fe3caaee0cf0d1a28497028b67ec711375c0cca",
            ),
            (
                "sherpa-onnx-c-api.lib",
                "a42f595587731c8c83b8b48f99e94a3ebe5885399902b54e218a0cb1066e62aa",
            ),
            (
                "onnxruntime.dll",
                "8b695444d1a35ed0c8338b8c14438b3be5e0a3b222b88b1e7b4ce8753f135b50",
            ),
            (
                "onnxruntime.lib",
                "4b8482a2b5cc3b825e468a13672e65e1e2b771c669cc6f29bb90cea3a87e16b0",
            ),
            (
                "onnxruntime_providers_shared.dll",
                "ebc55b0f28e8a79cbf78e810a7f510ba70e75a2dfbcfcc6aca31ab2b8710a59a",
            ),
        ],
        _ => unreachable!(),
    };
    let link_directory = PathBuf::from(ROOT).join("link");
    let configured_link_directory = std::env::var_os("SHERPA_ONNX_LIB_DIR")
        .map(PathBuf::from)
        .expect("SHERPA_ONNX_LIB_DIR must select the checked-in ASR runtime");
    assert_eq!(
        canonical(&configured_link_directory),
        canonical(&link_directory),
        "sherpa-onnx must link the checked-in runtime that Tauri bundles"
    );
    for (filename, sha256) in required_runtime {
        let bundled = PathBuf::from(ROOT).join(runtime).join(filename);
        require_hash(&bundled, sha256);
        let linked = link_directory.join(filename);
        require_hash(&linked, sha256);
    }
    for notice in [
        "notices/THIRD-PARTY-NOTICES.md",
        "notices/sherpa-onnx-LICENSE.txt",
        "notices/onnxruntime-LICENSE.txt",
        "notices/onnxruntime-ThirdPartyNotices.txt",
    ] {
        require(&format!("{ROOT}/{notice}"));
    }
    require("../THIRD_PARTY_NOTICES.md");

    // The packaged libraries live in Tauri's resource directory. Keep loader
    // lookup relative to the executable so no machine-global install is used.
    if let Some(link_arg) = asr_rpath::link_arg(&os, ExecutableLocation::Desktop) {
        println!("cargo:rustc-link-arg={link_arg}");
    }
    #[cfg(target_os = "macos")]
    {
        println!("cargo:rustc-link-arg=-Wl,-rpath,@executable_path/../Frameworks");
        let out = PathBuf::from(std::env::var("OUT_DIR").unwrap());
        let target = out.ancestors().nth(3).unwrap();
        let library = target.join("libmuniment_cef_keychain.dylib");
        let status = cc::Build::new()
            .get_compiler()
            .to_command()
            .args([
                "-dynamiclib",
                "-compatibility_version",
                "1.0",
                "-current_version",
                "1.0",
                "-Wno-deprecated-declarations",
                "-framework",
                "CoreFoundation",
                "-Wl,-reexport_framework,Security",
                "-Wl,-install_name,@rpath/libmuniment_cef_keychain.dylib",
                "src/keychain_macos.c",
                "-o",
            ])
            .arg(&library)
            .status()
            .expect("Compile private browser key bridge");
        assert!(status.success());
        println!("cargo:rustc-link-search=native={}", target.display());
        println!("cargo:rustc-link-lib=dylib=muniment_cef_keychain");
        cc::Build::new()
            .file("src/cef_macos.m")
            .flag("-fobjc-arc")
            .compile("muniment_cef_macos");
        println!("cargo:rustc-link-lib=framework=AppKit");
        println!("cargo:rerun-if-changed=src/keychain_macos.c");
        println!("cargo:rerun-if-changed=src/cef_macos.m");
    }
    tauri_build::try_build(
        tauri_build::Attributes::new()
            .app_manifest(tauri_build::AppManifest::new().commands(APP_COMMANDS)),
    )
    .unwrap_or_else(|error| panic!("{error:#}"));
}

fn require(path: &str) {
    println!("cargo:rerun-if-changed={path}");
    assert!(
        Path::new(path).is_file(),
        "packaged ASR runtime component or corresponding notice is missing: {path}"
    );
}

fn require_hash(path: &Path, expected: &str) {
    let display = path.display();
    println!("cargo:rerun-if-changed={display}");
    let mut file = File::open(path)
        .unwrap_or_else(|_| panic!("packaged ASR runtime component is missing: {display}"));
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = file
            .read(&mut buffer)
            .unwrap_or_else(|_| panic!("packaged ASR runtime component is unreadable: {display}"));
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
    }
    assert_eq!(
        format!("{:x}", digest.finalize()),
        expected,
        "packaged ASR runtime component has the wrong pinned identity: {display}"
    );
}

fn canonical(path: &Path) -> PathBuf {
    path.canonicalize()
        .unwrap_or_else(|_| panic!("ASR runtime path is missing: {}", path.display()))
}

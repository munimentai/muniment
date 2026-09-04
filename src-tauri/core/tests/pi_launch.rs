use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use muniment_core::chat_grant::ChatGrant;
use muniment_core::pi_launch::{
    pi_launch_config, pi_launch_config_for_executable, prepare_pi_agent_directory,
    PiLaunchBoundaries, PiLaunchError, PI_BASH_TIMEOUT_PROMPT,
};
use muniment_core::sidecar::{PiRpcWiring, SidecarStatus, SidecarSupervisor};
use uuid::Uuid;

static ENVIRONMENT: Mutex<()> = Mutex::new(());

struct Boundaries {
    session_root: Result<PathBuf, PiLaunchError>,
    extension: Option<PathBuf>,
}

impl PiLaunchBoundaries for Boundaries {
    fn pi_session_root(&self) -> Result<PathBuf, PiLaunchError> {
        self.session_root.clone()
    }

    fn memory_agent_extension_path(&self) -> Option<PathBuf> {
        self.extension.clone()
    }
}

struct WorkspaceBoundaries {
    session_root: PathBuf,
    agent_workspace: Mutex<Option<PathBuf>>,
}

impl PiLaunchBoundaries for WorkspaceBoundaries {
    fn pi_session_root(&self) -> Result<PathBuf, PiLaunchError> {
        Ok(self.session_root.clone())
    }

    fn memory_agent_extension_path(&self) -> Option<PathBuf> {
        None
    }

    fn pi_agent_directory(
        &self,
        workspace: &std::path::Path,
    ) -> Result<Option<PathBuf>, PiLaunchError> {
        *self.agent_workspace.lock().unwrap() = Some(workspace.to_path_buf());
        Ok(None)
    }
}

fn grant() -> ChatGrant {
    ChatGrant {
        workspace: "/work".into(),
        gateway_url: "https://gateway.example.com".into(),
        virtual_key: "secret-key".into(),
        model: Some("model".into()),
        minimum_cacheable_prefix_characters: 8_192,
        receipt_url: "https://receipts.example.com".into(),
    }
}

fn temporary_directory() -> PathBuf {
    let path = std::env::temp_dir().join(format!("muniment-pi-launch-{}", Uuid::new_v4()));
    fs::create_dir(&path).unwrap();
    path
}

#[test]
fn rejects_a_missing_root() {
    let boundaries = Boundaries {
        session_root: Err(PiLaunchError::UnavailableSessionRoot),
        extension: None,
    };
    assert_eq!(
        pi_launch_config(&boundaries, None, &grant(), None).unwrap_err(),
        PiLaunchError::MissingRoot
    );
}

#[test]
fn rejects_an_unresolvable_executable() {
    let root = temporary_directory();
    let boundaries = Boundaries {
        session_root: Ok(root.clone()),
        extension: None,
    };
    assert_eq!(
        pi_launch_config(&boundaries, Some(&root), &grant(), None).unwrap_err(),
        PiLaunchError::UnresolvableExecutable
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn reports_a_rejected_config() {
    let root = temporary_directory();
    let missing = root.join("missing");
    let boundaries = Boundaries {
        session_root: Ok(missing),
        extension: None,
    };
    assert_eq!(
        pi_launch_config_for_executable(&boundaries, "pi".into(), &grant(), None).unwrap_err(),
        PiLaunchError::RejectedConfig
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn appends_a_present_extension_file_and_environment() {
    let root = temporary_directory();
    let extension = root.join("memory.js");
    fs::write(&extension, "").unwrap();
    let boundaries = Boundaries {
        session_root: Ok(root.clone()),
        extension: Some(extension.clone()),
    };
    let config = pi_launch_config_for_executable(&boundaries, "pi".into(), &grant(), None).unwrap();
    assert_eq!(
        config.env.get("OPENAI_API_KEY").map(String::as_str),
        Some("secret-key")
    );
    assert_eq!(
        config.env.get("OPENAI_BASE_URL").map(String::as_str),
        Some("https://gateway.example.com")
    );
    assert_eq!(
        config.env.get("PI_DEFAULT_MODEL").map(String::as_str),
        Some("model")
    );
    assert!(config
        .args
        .windows(2)
        .any(|args| args == ["--extension", extension.to_string_lossy().as_ref()]));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn bundled_pi_agent_is_copied_and_seeds_the_workspace_mcp_config_once() {
    let root = temporary_directory();
    let bundled = root.join("bundled");
    let destination = root.join("configured");
    let workspace = root.join("workspace");
    fs::create_dir(&workspace).unwrap();
    for package in [
        "pi-web-access",
        "pi-subagents",
        "pi-background-tasks",
        "pi-mcp-adapter",
    ] {
        fs::create_dir_all(bundled.join("npm/node_modules").join(package)).unwrap();
        fs::write(
            bundled
                .join("npm/node_modules")
                .join(package)
                .join("package.json"),
            "{}",
        )
        .unwrap();
    }
    fs::create_dir_all(bundled.join(".pi")).unwrap();
    fs::write(bundled.join("settings.json"), "{\"packages\":[]}").unwrap();
    fs::write(bundled.join("bundle-version"), "0.84.4\n").unwrap();
    fs::write(bundled.join(".pi/mcp.json"), "{\"mcpServers\":{}}").unwrap();

    prepare_pi_agent_directory(&bundled, &destination, &workspace).unwrap();
    assert_eq!(
        fs::read_to_string(workspace.join(".pi/mcp.json")).unwrap(),
        "{\"mcpServers\":{}}"
    );
    fs::write(
        workspace.join(".pi/mcp.json"),
        "{\"mcpServers\":{\"local\":{}}}",
    )
    .unwrap();
    prepare_pi_agent_directory(&bundled, &destination, &workspace).unwrap();

    assert_eq!(
        fs::read_to_string(destination.join("settings.json")).unwrap(),
        "{\"packages\":[]}"
    );
    assert_eq!(
        fs::read_to_string(workspace.join(".pi/mcp.json")).unwrap(),
        "{\"mcpServers\":{\"local\":{}}}"
    );
    for package in [
        "pi-web-access",
        "pi-subagents",
        "pi-background-tasks",
        "pi-mcp-adapter",
    ] {
        assert!(destination.join("npm/node_modules").join(package).is_dir());
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn launch_uses_the_grant_workspace_for_the_sidecar_and_agent_seed() {
    let root = temporary_directory();
    let workspace = root.join("workspace");
    fs::create_dir(&workspace).unwrap();
    assert_ne!(std::env::current_dir().unwrap(), workspace);
    let boundaries = WorkspaceBoundaries {
        session_root: root.clone(),
        agent_workspace: Mutex::new(None),
    };
    let mut workspace_grant = grant();
    workspace_grant.workspace = workspace.to_string_lossy().into_owned();

    let config =
        pi_launch_config_for_executable(&boundaries, "pi".into(), &workspace_grant, None).unwrap();

    assert_eq!(config.working_directory, Some(workspace.clone()));
    assert_eq!(*boundaries.agent_workspace.lock().unwrap(), Some(workspace));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn launch_appends_the_factory_bash_timeout_rule() {
    let root = temporary_directory();
    let boundaries = Boundaries {
        session_root: Ok(root.clone()),
        extension: None,
    };

    let config = pi_launch_config_for_executable(&boundaries, "pi".into(), &grant(), None).unwrap();

    assert!(config
        .args
        .windows(2)
        .any(|args| { args == ["--append-system-prompt", PI_BASH_TIMEOUT_PROMPT] }));
    assert!(PI_BASH_TIMEOUT_PROMPT.contains("SECONDS, never milliseconds"));
    assert!(PI_BASH_TIMEOUT_PROMPT.contains("60 for a quick command"));
    assert!(PI_BASH_TIMEOUT_PROMPT.contains("up to 600 for a build"));
    assert!(PI_BASH_TIMEOUT_PROMPT.contains("belongs in `bg_run`"));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn local_launch_uses_pis_credential_store_without_cloud_credentials() {
    let root = temporary_directory();
    let boundaries = Boundaries {
        session_root: Ok(root.clone()),
        extension: None,
    };
    let config =
        pi_launch_config_for_executable(&boundaries, "pi".into(), &ChatGrant::local(), None)
            .unwrap();
    assert!(!config.env.contains_key("OPENAI_API_KEY"));
    assert!(!config.env.contains_key("OPENAI_BASE_URL"));
    assert!(!config.env.contains_key("PI_DEFAULT_MODEL"));
    assert_eq!(config.args[0..2], ["--mode", "rpc"]);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn local_launch_clears_inherited_cloud_environment_in_the_child() {
    let _environment = ENVIRONMENT.lock().unwrap();
    let root = temporary_directory();
    let capture = root.join("environment.json");
    let boundaries = Boundaries {
        session_root: Ok(root.clone()),
        extension: None,
    };
    let executable_name = if cfg!(windows) {
        "sidecar-test-stub.exe"
    } else {
        "sidecar-test-stub"
    };
    let executable = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join(executable_name);
    let mut config =
        pi_launch_config_for_executable(&boundaries, executable, &ChatGrant::local(), None)
            .unwrap();
    config.env.insert(
        "PI_RESUME_STUB_ENV_CAPTURE".into(),
        capture.to_string_lossy().into_owned(),
    );
    for (name, value) in [
        ("OPENAI_API_KEY", "inherited-key"),
        ("OPENAI_BASE_URL", "https://inherited.example.com"),
        ("PI_DEFAULT_MODEL", "inherited-model"),
    ] {
        std::env::set_var(name, value);
    }

    let wiring = PiRpcWiring::new();
    let mut supervisor =
        SidecarSupervisor::spawn(config, wiring.readiness_probe(Duration::from_millis(100)))
            .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while (!capture.is_file() || supervisor.status() != SidecarStatus::Healthy)
        && Instant::now() < deadline
    {
        std::thread::sleep(Duration::from_millis(5));
    }
    for name in ["OPENAI_API_KEY", "OPENAI_BASE_URL", "PI_DEFAULT_MODEL"] {
        std::env::remove_var(name);
    }

    assert_eq!(supervisor.status(), SidecarStatus::Healthy);
    assert_eq!(fs::read_to_string(&capture).unwrap(), "{}");
    supervisor.shutdown().unwrap();
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn omits_an_absent_extension_file() {
    let root = temporary_directory();
    let boundaries = Boundaries {
        session_root: Ok(root.clone()),
        extension: Some(root.join("missing.js")),
    };
    let config = pi_launch_config_for_executable(&boundaries, "pi".into(), &grant(), None).unwrap();
    assert!(!config.args.iter().any(|arg| arg == "--extension"));
    fs::remove_dir_all(root).unwrap();
}

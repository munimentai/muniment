use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use fs2::FileExt;

use crate::chat_grant::ChatGrant;
use crate::sidecar::pi_install::{resolve_current_for, PiArtifactDescriptor, PI_ARTIFACT};
use crate::sidecar::{pi_sidecar_config, PiSessionLocator, SidecarConfig};

pub const PI_BASH_TIMEOUT_PROMPT: &str = "`bash` reads its `timeout` in SECONDS, never milliseconds, and applies NO timeout at all when you omit it. Pass one on every call: 60 for a quick command, up to 600 for a build or a test suite. Work that needs more than 600 seconds belongs in `bg_run`, not behind a bigger timeout.";

const LOCAL_MODE_ENV_REMOVE: &[&str] = &[
    "AI_GATEWAY_API_KEY",
    "ANTHROPIC_API_KEY",
    "ANTHROPIC_OAUTH_TOKEN",
    "AWS_ACCESS_KEY_ID",
    "AWS_BEARER_TOKEN_BEDROCK",
    "AWS_CONTAINER_CREDENTIALS_FULL_URI",
    "AWS_CONTAINER_CREDENTIALS_RELATIVE_URI",
    "AWS_PROFILE",
    "AWS_SECRET_ACCESS_KEY",
    "AWS_SESSION_TOKEN",
    "AWS_WEB_IDENTITY_TOKEN_FILE",
    "AZURE_OPENAI_API_KEY",
    "AZURE_OPENAI_BASE_URL",
    "CEREBRAS_API_KEY",
    "CLOUDFLARE_API_KEY",
    "COPILOT_GITHUB_TOKEN",
    "DEEPSEEK_API_KEY",
    "FIREWORKS_API_KEY",
    "GCLOUD_PROJECT",
    "GEMINI_API_KEY",
    "GH_TOKEN",
    "GITHUB_TOKEN",
    "GOOGLE_APPLICATION_CREDENTIALS",
    "GOOGLE_CLOUD_API_KEY",
    "GOOGLE_CLOUD_LOCATION",
    "GOOGLE_CLOUD_PROJECT",
    "GROQ_API_KEY",
    "HF_TOKEN",
    "KIMI_API_KEY",
    "MINIMAX_API_KEY",
    "MINIMAX_CN_API_KEY",
    "MISTRAL_API_KEY",
    "MOONSHOT_API_KEY",
    "OPENAI_API_KEY",
    "OPENAI_BASE_URL",
    "OPENCODE_API_KEY",
    "OPENROUTER_API_KEY",
    "PI_DEFAULT_MODEL",
    "XAI_API_KEY",
    "XIAOMI_API_KEY",
    "XIAOMI_TOKEN_PLAN_AMS_API_KEY",
    "XIAOMI_TOKEN_PLAN_CN_API_KEY",
    "XIAOMI_TOKEN_PLAN_SGP_API_KEY",
    "ZAI_API_KEY",
];

pub trait PiLaunchBoundaries {
    fn pi_session_root(&self) -> Result<PathBuf, PiLaunchError>;
    fn memory_agent_extension_path(&self) -> Option<PathBuf>;
    fn pi_agent_directory(&self, _workspace: &Path) -> Result<Option<PathBuf>, PiLaunchError> {
        Ok(None)
    }
    fn pi_workspace_directory(&self) -> Result<Option<PathBuf>, PiLaunchError> {
        Ok(None)
    }
    fn pi_artifact(&self) -> PiArtifactDescriptor {
        PI_ARTIFACT
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PiLaunchError {
    MissingRoot,
    UnresolvableExecutable,
    UnavailableSessionRoot,
    RejectedConfig,
    UnavailableAgentDirectory,
}

const PI_PACKAGE_NAMES: &[&str] = &[
    "pi-web-access",
    "pi-subagents",
    "pi-background-tasks",
    "pi-mcp-adapter",
];

pub fn prepare_pi_agent_directory(
    bundled: &Path,
    destination: &Path,
    workspace: &Path,
) -> Result<PathBuf, PiLaunchError> {
    for package in PI_PACKAGE_NAMES {
        if !bundled.join("npm/node_modules").join(package).is_dir() {
            return Err(PiLaunchError::UnavailableAgentDirectory);
        }
    }
    if !bundled.join("settings.json").is_file()
        || !bundled.join(".pi/mcp.json").is_file()
        || !bundled.join("bundle-version").is_file()
    {
        return Err(PiLaunchError::UnavailableAgentDirectory);
    }

    fs::create_dir_all(destination).map_err(|_| PiLaunchError::UnavailableAgentDirectory)?;
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(destination.join(".install.lock"))
        .map_err(|_| PiLaunchError::UnavailableAgentDirectory)?;
    lock.lock_exclusive()
        .map_err(|_| PiLaunchError::UnavailableAgentDirectory)?;
    let workspace_pi = workspace.join(".pi");
    fs::create_dir_all(&workspace_pi).map_err(|_| PiLaunchError::UnavailableAgentDirectory)?;
    let workspace_mcp = workspace_pi.join("mcp.json");
    let mcp_template = fs::read(bundled.join(".pi/mcp.json"))
        .map_err(|_| PiLaunchError::UnavailableAgentDirectory)?;
    match OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(workspace_mcp)
    {
        Ok(mut file) => file
            .write_all(&mcp_template)
            .map_err(|_| PiLaunchError::UnavailableAgentDirectory)?,
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
        Err(_) => return Err(PiLaunchError::UnavailableAgentDirectory),
    }

    let settings_match = fs::read(bundled.join("settings.json")).ok()
        == fs::read(destination.join("settings.json")).ok();
    let version_match = fs::read(bundled.join("bundle-version")).ok()
        == fs::read(destination.join("bundle-version")).ok();
    let packages_present = PI_PACKAGE_NAMES
        .iter()
        .all(|package| destination.join("npm/node_modules").join(package).is_dir());
    if settings_match && version_match && packages_present {
        return Ok(destination.to_owned());
    }

    let staged_npm = destination.join(format!(".npm-stage-{}", std::process::id()));
    let _ = fs::remove_dir_all(&staged_npm);
    copy_directory(&bundled.join("npm"), &staged_npm)?;
    let installed_npm = destination.join("npm");
    let previous_npm = destination.join(".npm-previous");
    let _ = fs::remove_dir_all(&previous_npm);
    if installed_npm.exists() {
        fs::rename(&installed_npm, &previous_npm)
            .map_err(|_| PiLaunchError::UnavailableAgentDirectory)?;
    }
    if fs::rename(&staged_npm, &installed_npm).is_err() {
        let _ = fs::rename(&previous_npm, &installed_npm);
        return Err(PiLaunchError::UnavailableAgentDirectory);
    }
    fs::copy(
        bundled.join("settings.json"),
        destination.join("settings.json"),
    )
    .map_err(|_| PiLaunchError::UnavailableAgentDirectory)?;
    fs::copy(
        bundled.join("bundle-version"),
        destination.join("bundle-version"),
    )
    .map_err(|_| PiLaunchError::UnavailableAgentDirectory)?;
    let _ = fs::remove_dir_all(previous_npm);
    FileExt::unlock(&lock).map_err(|_| PiLaunchError::UnavailableAgentDirectory)?;
    Ok(destination.to_owned())
}

fn copy_directory(source: &Path, destination: &Path) -> Result<(), PiLaunchError> {
    fs::create_dir_all(destination).map_err(|_| PiLaunchError::UnavailableAgentDirectory)?;
    for entry in fs::read_dir(source).map_err(|_| PiLaunchError::UnavailableAgentDirectory)? {
        let entry = entry.map_err(|_| PiLaunchError::UnavailableAgentDirectory)?;
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        if source_path.is_dir() {
            copy_directory(&source_path, &destination_path)?;
        } else if source_path.is_file() {
            fs::copy(source_path, destination_path)
                .map_err(|_| PiLaunchError::UnavailableAgentDirectory)?;
        }
    }
    Ok(())
}

pub fn pi_launch_config(
    boundaries: &impl PiLaunchBoundaries,
    root: Option<&Path>,
    grant: &ChatGrant,
    reopen: Option<&PiSessionLocator>,
) -> Result<SidecarConfig, PiLaunchError> {
    let root = root.ok_or(PiLaunchError::MissingRoot)?;
    let executable = resolve_current_for(root, boundaries.pi_artifact())
        .map_err(|_| PiLaunchError::UnresolvableExecutable)?;
    pi_launch_config_for_executable(boundaries, executable, grant, reopen)
}

pub fn pi_launch_config_for_executable(
    boundaries: &impl PiLaunchBoundaries,
    executable: PathBuf,
    grant: &ChatGrant,
    reopen: Option<&PiSessionLocator>,
) -> Result<SidecarConfig, PiLaunchError> {
    let session_root = boundaries.pi_session_root()?;
    let mut config = pi_sidecar_config(executable.to_string_lossy(), &session_root, reopen)
        .map_err(|_| PiLaunchError::RejectedConfig)?;
    config.args.extend([
        "--append-system-prompt".into(),
        PI_BASH_TIMEOUT_PROMPT.into(),
    ]);
    let workspace = if grant.is_local() {
        boundaries.pi_workspace_directory()?
    } else {
        Some(PathBuf::from(&grant.workspace))
    };
    if let Some(agent_directory) = workspace
        .as_deref()
        .map(|workspace| boundaries.pi_agent_directory(workspace))
        .transpose()?
        .flatten()
    {
        config.env.insert(
            "PI_CODING_AGENT_DIR".into(),
            agent_directory.to_string_lossy().into_owned(),
        );
    }
    config.working_directory = workspace;
    if grant.is_local() {
        config.env_remove = LOCAL_MODE_ENV_REMOVE
            .iter()
            .map(|name| (*name).to_owned())
            .collect();
    } else {
        config
            .env
            .insert("OPENAI_API_KEY".into(), grant.virtual_key.clone());
        config
            .env
            .insert("OPENAI_BASE_URL".into(), grant.gateway_url.clone());
        if let Some(model) = &grant.model {
            config.env.insert("PI_DEFAULT_MODEL".into(), model.clone());
        }
    }
    if let Some(extension) = boundaries
        .memory_agent_extension_path()
        .filter(|path| path.is_file())
    {
        config.args.extend([
            "--extension".into(),
            extension.to_string_lossy().into_owned(),
        ]);
    }
    Ok(config)
}

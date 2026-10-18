//! Endpoint keys outside Pi's `models.json`. A key an endpoint provider holds
//! lives in the host's credential store, one entry per provider, and its
//! `apiKey` in `models.json` is a command Pi runs for the key:
//! `!"<runtime executable>" secret get <provider>`. The runtime executable
//! answers that command from the same store, so the key never sits in a file
//! Pi or a tool reads. Stand-in keys, environment references and commands the
//! user wrote stay as they are.

use std::path::Path;
use std::sync::Arc;

use muniment_core::model_router::config::ROUTER_PROVIDER;
use muniment_core::models::pi_models;
use muniment_core::serde_json::Value;

use crate::host::CredentialStore;

/// The credential store service that holds endpoint keys.
pub const SERVICE: &str = "ai.muniment.endpoints";

/// The arguments the runtime executable answers a key on.
const COMMAND: [&str; 2] = ["secret", "get"];

/// The values Pi sends to a server with no key. They are not keys.
const STAND_INS: [&str; 2] = ["ollama", "local"];

/// The `apiKey` value that has Pi read `provider`'s key through `executable`.
pub fn reference(executable: &Path, provider: &str) -> Option<String> {
    let path = executable.to_str()?;
    (safe_provider(provider) && !path.contains('"') && !path.is_empty())
        .then(|| format!("!\"{path}\" {} {} {provider}", COMMAND[0], COMMAND[1]))
}

/// The provider a reference this module wrote names, with the executable it
/// runs, or none for any other value.
fn parse_reference(value: &str) -> Option<(&str, &str)> {
    let rest = value.strip_prefix("!\"")?;
    let (path, rest) = rest.split_once('"')?;
    let provider = rest.strip_prefix(&format!(" {} {} ", COMMAND[0], COMMAND[1]))?;
    safe_provider(provider).then_some((path, provider))
}

/// A provider id the shell passes through untouched.
fn safe_provider(provider: &str) -> bool {
    !provider.is_empty()
        && provider.len() <= 128
        && provider
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

/// Moves every plain endpoint key in `models.json` into `store` and writes a
/// reference through `executable` in its place, and points each reference
/// this module wrote at `executable`, so a moved runtime keeps its keys.
/// Answers how many entries changed. A key the store refuses stays in the
/// file, so no key is lost.
pub fn seal(agent: &Path, store: &dyn CredentialStore, executable: &Path) -> Result<usize, String> {
    let models_file = pi_models::models_file(agent);
    if !models_file.is_file() {
        return Ok(0);
    }
    let _lock = pi_models::lock(&models_file).map_err(|_| pi_models::SAVE_ERROR.to_string())?;
    let mut root =
        pi_models::read_json(&models_file).map_err(|_| pi_models::SAVE_ERROR.to_string())?;
    let mut changed = 0;
    if let Some(providers) = root.get_mut("providers").and_then(Value::as_object_mut) {
        for (provider, entry) in providers
            .iter_mut()
            .filter(|(provider, _)| provider.as_str() != ROUTER_PROVIDER)
        {
            let Some(key) = entry.get("apiKey").and_then(Value::as_str) else {
                continue;
            };
            let Some(next) = reference(executable, provider) else {
                continue;
            };
            if key == next {
                continue;
            }
            if let Some((_, named)) = parse_reference(key) {
                if named != provider {
                    continue;
                }
            } else {
                if key.is_empty()
                    || STAND_INS.contains(&key)
                    || key.starts_with('!')
                    || key.starts_with('$')
                {
                    continue;
                }
                if let Err(error) = store.write(SERVICE, provider, key) {
                    muniment_core::runtime_eprintln!(
                        "muniment-runtime: endpoint_key provider={provider} outcome=kept error={error}"
                    );
                    continue;
                }
            }
            entry["apiKey"] = next.into();
            changed += 1;
        }
    }
    if changed > 0 {
        pi_models::write_json(&models_file, &root)
            .map_err(|_| pi_models::SAVE_ERROR.to_string())?;
    }
    Ok(changed)
}

/// Removes `provider`'s key from `store`.
pub fn forget(store: &dyn CredentialStore, provider: &str) -> Result<(), String> {
    if !safe_provider(provider) {
        return Ok(());
    }
    store.remove(SERVICE, provider)
}

/// Answers `secret get <provider>`, the command a reference runs: the key on
/// standard output. Answers none for other arguments, so the caller handles
/// them, and the exit status otherwise: 0 with the key, 1 without one.
pub fn run_command(
    arguments: &[String],
    store: impl FnOnce() -> Arc<dyn CredentialStore>,
) -> Option<i32> {
    let [first, second, provider] = arguments else {
        return None;
    };
    if first != COMMAND[0] || second != COMMAND[1] {
        return None;
    }
    if !safe_provider(provider) {
        return Some(1);
    }
    match store().read(SERVICE, provider) {
        Some(key) => {
            println!("{key}");
            Some(0)
        }
        None => Some(1),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::FileCredentialStore;
    use muniment_core::serde_json::{self, json};
    use std::path::PathBuf;

    fn temporary(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "muniment-endpoint-secrets-{name}-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    fn models(agent: &Path) -> Value {
        serde_json::from_slice(&std::fs::read(agent.join("models.json")).unwrap()).unwrap()
    }

    #[test]
    fn a_plain_key_moves_to_the_store_and_pi_reads_it_through_the_runtime() {
        let root = temporary("seal");
        let agent = root.join("agent");
        std::fs::create_dir_all(&agent).unwrap();
        std::fs::write(
            agent.join("models.json"),
            serde_json::to_vec(&json!({"providers": {
                "custom-proxy": {"baseUrl": "https://proxy.example/v1", "apiKey": "sk-secret"},
                "ollama": {"baseUrl": "http://127.0.0.1:11434/v1", "apiKey": "ollama"},
                "lmstudio": {"baseUrl": "http://127.0.0.1:1234/v1", "apiKey": "local"},
                "custom-env": {"baseUrl": "https://env.example/v1", "apiKey": "$PROXY_KEY"},
                "custom-own": {"baseUrl": "https://own.example/v1", "apiKey": "!op read key"},
                ROUTER_PROVIDER: {"baseUrl": "http://127.0.0.1:1/v1", "apiKey": "router-token"}
            }}))
            .unwrap(),
        )
        .unwrap();
        let store = FileCredentialStore::new(root.join("credentials"));
        let executable = Path::new("/opt/muniment/muniment-runtime");

        assert_eq!(seal(&agent, &store, executable).unwrap(), 1);
        let written = models(&agent);
        let providers = &written["providers"];
        assert_eq!(
            providers["custom-proxy"]["apiKey"],
            "!\"/opt/muniment/muniment-runtime\" secret get custom-proxy"
        );
        assert!(!std::fs::read_to_string(agent.join("models.json"))
            .unwrap()
            .contains("sk-secret"));
        assert_eq!(
            store.read(SERVICE, "custom-proxy").as_deref(),
            Some("sk-secret")
        );
        for (provider, kept) in [
            ("ollama", "ollama"),
            ("lmstudio", "local"),
            ("custom-env", "$PROXY_KEY"),
            ("custom-own", "!op read key"),
            (ROUTER_PROVIDER, "router-token"),
        ] {
            assert_eq!(providers[provider]["apiKey"], kept, "{provider}");
        }

        // A second pass changes nothing, and a moved runtime takes its references along.
        assert_eq!(seal(&agent, &store, executable).unwrap(), 0);
        let moved = Path::new("/Applications/Muniment.app/Contents/MacOS/muniment-runtime");
        assert_eq!(seal(&agent, &store, moved).unwrap(), 1);
        assert_eq!(
            models(&agent)["providers"]["custom-proxy"]["apiKey"],
            "!\"/Applications/Muniment.app/Contents/MacOS/muniment-runtime\" secret get custom-proxy"
        );
        assert_eq!(
            store.read(SERVICE, "custom-proxy").as_deref(),
            Some("sk-secret")
        );

        forget(&store, "custom-proxy").unwrap();
        assert_eq!(store.read(SERVICE, "custom-proxy"), None);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_key_the_store_refuses_stays_in_the_file() {
        let root = temporary("refused");
        let agent = root.join("agent");
        std::fs::create_dir_all(&agent).unwrap();
        std::fs::write(
            agent.join("models.json"),
            r#"{"providers":{"custom-proxy":{"baseUrl":"https://proxy.example/v1","apiKey":"sk-kept"}}}"#,
        )
        .unwrap();
        let store = FileCredentialStore::environment_only();
        assert_eq!(seal(&agent, &store, Path::new("/opt/runtime")).unwrap(), 0);
        assert_eq!(
            models(&agent)["providers"]["custom-proxy"]["apiKey"],
            "sk-kept"
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn the_runtime_answers_only_the_secret_command() {
        let root = temporary("command");
        let store = FileCredentialStore::new(root.join("credentials"));
        store.write(SERVICE, "custom-proxy", "sk-secret").unwrap();
        let open = || {
            Arc::new(FileCredentialStore::new(root.join("credentials"))) as Arc<dyn CredentialStore>
        };
        let arguments = |values: &[&str]| {
            values
                .iter()
                .map(|value| value.to_string())
                .collect::<Vec<_>>()
        };
        assert_eq!(
            run_command(&arguments(&["secret", "get", "custom-proxy"]), open),
            Some(0)
        );
        assert_eq!(
            run_command(&arguments(&["secret", "get", "custom-missing"]), open),
            Some(1)
        );
        assert_eq!(
            run_command(&arguments(&["secret", "get", "../escape"]), open),
            Some(1)
        );
        assert_eq!(run_command(&arguments(&["--version"]), open), None);
        assert_eq!(
            run_command(&arguments(&["secret", "list", "x"]), open),
            None
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn a_reference_resolves_through_the_command_it_names() {
        use std::os::unix::fs::PermissionsExt;
        let root = temporary("resolve");
        let credentials = root.join("credentials");
        FileCredentialStore::new(&credentials)
            .write(SERVICE, "custom-proxy", "sk-secret")
            .unwrap();
        // A stand-in for the runtime executable that answers from the same store.
        let executable = root.join("runtime");
        std::fs::write(
            &executable,
            format!(
                "#!/bin/sh\n[ \"$1 $2\" = \"secret get\" ] && cat \"{}/{SERVICE}/$3\"\n",
                credentials.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o755)).unwrap();
        let value = reference(&executable, "custom-proxy").unwrap();
        assert_eq!(pi_models::resolve_key(&value).as_deref(), Some("sk-secret"));
        assert_eq!(
            pi_models::resolve_key("sk-plain").as_deref(),
            Some("sk-plain")
        );
        assert_eq!(pi_models::resolve_key("!exit 3"), None);
        let _ = std::fs::remove_dir_all(root);
    }
}

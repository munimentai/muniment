//! Saved classifier connections. Secrets stay in the private profile file.
use super::*;
use serde::Deserialize;

static MUTATION: Mutex<()> = Mutex::new(());

#[derive(Clone, Serialize, Deserialize)]
struct Connection {
    id: String,
    name: String,
    catalog_id: String,
    classifier: Classifier,
}

#[derive(Debug, Serialize)]
pub(crate) struct ConnectionView {
    id: String,
    name: String,
    catalog_id: String,
    active: bool,
    connection: ClassifierView,
}

fn read(agent: &Path, active: &Classifier) -> Result<Vec<Connection>, String> {
    match std::fs::read(agent.join("classifier-connections.json")) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(|_| READ_ERROR.into()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            // Migrate the existing dedicated connection without exposing its key.
            Ok(
                if active.ready() && !matches!(active, Classifier::Pooled { .. }) {
                    vec![Connection {
                        id: "existing".into(),
                        name: active.model().into(),
                        catalog_id: if matches!(active, Classifier::Typesafe { .. }) {
                            "jev"
                        } else {
                            "custom"
                        }
                        .into(),
                        classifier: active.clone(),
                    }]
                } else {
                    vec![]
                },
            )
        }
        Err(_) => Err(READ_ERROR.into()),
    }
}

fn write(agent: &Path, connections: &[Connection]) -> Result<(), String> {
    std::fs::create_dir_all(agent).map_err(|_| SAVE_ERROR.to_string())?;
    let bytes = serde_json::to_vec_pretty(connections).map_err(|_| SAVE_ERROR.to_string())?;
    config::write_private(&agent.join("classifier-connections.json"), &bytes)
        .map_err(|_| SAVE_ERROR.into())
}

pub(super) fn views(agent: &Path, active: &Classifier) -> Result<Vec<ConnectionView>, String> {
    Ok(read(agent, active)?
        .into_iter()
        .map(|entry| ConnectionView {
            active: entry.classifier == *active,
            connection: classifier_view(&entry.classifier),
            id: entry.id,
            name: entry.name,
            catalog_id: entry.catalog_id,
        })
        .collect())
}

#[tauri::command]
pub(crate) async fn model_router_connect_classifier(
    state: tauri::State<'_, RouterState>,
    catalog_id: String,
    name: String,
    model: String,
    base_url: String,
    api_key: Option<String>,
) -> Result<RouterSettings, String> {
    let url = url::Url::parse(base_url.trim()).map_err(|_| "Enter a valid classifier URL.")?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err("Use an HTTP or HTTPS URL without credentials in the address.".into());
    }
    if name.trim().is_empty() || model.trim().is_empty() {
        return Err("Enter a name and model ID.".into());
    }
    let classifier = Classifier::Endpoint {
        base_url: url.to_string(),
        api_key: api_key
            .map(|key| key.trim().to_owned())
            .filter(|key| !key.is_empty()),
        model: model.trim().into(),
    };
    let probe = classifier.clone();
    tauri::async_runtime::spawn_blocking(move || classify::check(&probe, CLASSIFIER_TIMEOUT))
        .await
        .map_err(|_| "The connection test did not finish.".to_string())??;
    let _guard = MUTATION.lock().map_err(|_| SAVE_ERROR.to_string())?;
    let agent = agent()?;
    let config = load(&agent)?;
    let mut connections = read(&agent, &config.classifier)?;
    connections.push(Connection {
        id: uuid::Uuid::now_v7().to_string(),
        name: name.trim().into(),
        catalog_id,
        classifier,
    });
    write(&agent, &connections)?;
    settings(
        &agent,
        running_endpoint(&state).as_ref(),
        &running_active(&state),
    )
}

#[tauri::command]
pub(crate) fn model_router_select_classifier(
    state: tauri::State<'_, RouterState>,
    id: String,
) -> Result<RouterSettings, String> {
    let _guard = MUTATION.lock().map_err(|_| SAVE_ERROR.to_string())?;
    let agent = agent()?;
    let mut config = load(&agent)?;
    let connections = read(&agent, &config.classifier)?;
    let selected = connections
        .iter()
        .find(|entry| entry.id == id)
        .ok_or("Choose a connected classifier.")?;
    write(&agent, &connections)?;
    config.classifier = selected.classifier.clone();
    save(&agent, &config)?;
    apply(&agent, &state, &config)?;
    settings(
        &agent,
        running_endpoint(&state).as_ref(),
        &running_active(&state),
    )
}

#[tauri::command]
pub(crate) fn model_router_disconnect_classifier(
    state: tauri::State<'_, RouterState>,
    id: String,
) -> Result<RouterSettings, String> {
    let _guard = MUTATION.lock().map_err(|_| SAVE_ERROR.to_string())?;
    let agent = agent()?;
    let mut config = load(&agent)?;
    let mut connections = read(&agent, &config.classifier)?;
    if connections
        .iter()
        .any(|entry| entry.id == id && entry.classifier == config.classifier)
    {
        let defaults: Value = std::fs::read(agent.join("settings.json"))
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        if defaults["defaultProvider"] == "muniment-router" && defaults["defaultModel"] == "auto" {
            return Err("Choose another classifier or turn routing off before disconnecting this classifier.".into());
        }
        config.classifier = Classifier::None;
        save(&agent, &config)?;
        apply(&agent, &state, &config)?;
    }
    connections.retain(|entry| entry.id != id);
    write(&agent, &connections)?;

    settings(
        &agent,
        running_endpoint(&state).as_ref(),
        &running_active(&state),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn migrates_and_redacts_existing_classifier() {
        let root = std::env::temp_dir().join(format!("classifier-test-{}", uuid::Uuid::now_v7()));
        let active = Classifier::Typesafe {
            api_key: "secret-test-key".into(),
            model: "jev-latest".into(),
            base_url: None,
        };
        let entries = read(&root, &active).unwrap();
        write(&root, &entries).unwrap();
        let output = serde_json::to_string(&views(&root, &active).unwrap()).unwrap();
        assert!(output.contains("existing"));
        assert!(!output.contains("secret-test-key"));
        assert_eq!(read(&root, &Classifier::None).unwrap().len(), 1);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(root.join("classifier-connections.json"))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn corrupt_store_does_not_silently_reset() {
        let root = std::env::temp_dir().join(format!("classifier-test-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("classifier-connections.json"), b"broken").unwrap();
        assert!(read(&root, &Classifier::None).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
}

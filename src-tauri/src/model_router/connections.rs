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

#[derive(Debug, Serialize)]
pub(crate) struct AssistView {
    enabled: bool,
    connection: String,
}

pub(super) fn assist_view(agent: &Path, active: &Classifier) -> Result<AssistView, String> {
    let assist = config::load_assist(agent).map_err(|_| READ_ERROR.to_string())?;
    let connection = match &assist.classifier {
        Some(classifier) => read(agent, active)?
            .into_iter()
            .find(|entry| entry.classifier == *classifier)
            .map(|entry| entry.id)
            .unwrap_or_default(),
        None => String::new(),
    };
    Ok(AssistView {
        enabled: assist.enabled,
        connection,
    })
}

/// Turns decision-model assistance on or off and names the connection it
/// asks. An empty id uses the model routing decision model.
#[tauri::command]
pub(crate) fn model_router_set_assist(
    state: tauri::State<'_, RouterState>,
    enabled: bool,
    id: String,
) -> Result<RouterSettings, String> {
    let _guard = MUTATION.lock().map_err(|_| SAVE_ERROR.to_string())?;
    let agent = agent()?;
    let config = load(&agent)?;
    let classifier = if id.is_empty() {
        None
    } else {
        Some(
            read(&agent, &config.classifier)?
                .into_iter()
                .find(|entry| entry.id == id)
                .ok_or("Choose a connected decision model.")?
                .classifier,
        )
    };
    let assist = config::Assist {
        enabled,
        classifier,
    };
    if enabled && assist.decision_model(&config).is_none() {
        return Err("Choose a decision model for assistance.".into());
    }
    config::save_assist(&agent, &assist).map_err(|_| SAVE_ERROR.to_string())?;
    settings(
        &agent,
        running_endpoint(&state).as_ref(),
        &running_active(&state),
    )
}

/// Assistance follows an edited connection, and turns off when the decision
/// model it asks is disconnected.
fn follow_assist(
    agent: &Path,
    from: &Classifier,
    to: Option<&Classifier>,
    routing: bool,
) -> Result<(), String> {
    let mut assist = config::load_assist(agent).map_err(|_| READ_ERROR.to_string())?;
    let uses = match &assist.classifier {
        Some(classifier) => classifier == from,
        None => routing,
    };
    if !uses {
        return Ok(());
    }
    match to {
        Some(next) if assist.classifier.is_some() => assist.classifier = Some(next.clone()),
        Some(_) => return Ok(()),
        None => assist = config::Assist::default(),
    }
    config::save_assist(agent, &assist).map_err(|_| SAVE_ERROR.to_string())
}

#[tauri::command]
pub(crate) async fn model_router_connect_classifier(
    state: tauri::State<'_, RouterState>,
    catalog_id: String,
    name: String,
    model: String,
    base_url: String,
    api_key: Option<String>,
    key_provider: Option<String>,
) -> Result<RouterSettings, String> {
    // A decision model on a connected server, such as Ollama, sends the key
    // that server holds, which Settings never reads back to show.
    let api_key = api_key.filter(|key| !key.trim().is_empty()).or_else(|| {
        crate::local_mode::endpoint_key_for(
            &crate::local_mode::pi_models_file(&agent().ok()?),
            key_provider.as_deref()?,
            base_url.trim(),
        )
    });
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

/// The saved connection with its new details. A blank key keeps the saved key,
/// because Settings never reads a key back to show it. A pooled classifier
/// spends an account and is never a saved connection, so it has no form.
fn edited(
    saved: &Classifier,
    model: &str,
    base_url: &str,
    api_key: Option<String>,
) -> Result<Classifier, String> {
    let model = model.trim();
    if model.is_empty() {
        return Err("Enter a model ID.".into());
    }
    let key = api_key
        .map(|key| key.trim().to_owned())
        .filter(|key| !key.is_empty());
    let checked = |url: &str| -> Result<String, String> {
        let parsed = url::Url::parse(url.trim()).map_err(|_| "Enter a valid classifier URL.")?;
        if !matches!(parsed.scheme(), "http" | "https")
            || parsed.host().is_none()
            || !parsed.username().is_empty()
            || parsed.password().is_some()
        {
            return Err("Use an HTTP or HTTPS URL without credentials in the address.".into());
        }
        Ok(parsed.to_string())
    };
    match saved {
        Classifier::Endpoint { api_key: saved_key, .. } => Ok(Classifier::Endpoint {
            base_url: checked(base_url)?,
            api_key: key.or_else(|| saved_key.clone()),
            model: model.into(),
        }),
        Classifier::Typesafe { api_key: saved_key, .. } => Ok(Classifier::Typesafe {
            api_key: key.unwrap_or_else(|| saved_key.clone()),
            model: model.into(),
            base_url: Some(base_url.trim())
                .filter(|url| !url.is_empty())
                .map(checked)
                .transpose()?,
        }),
        _ => Err("This classifier has no saved connection to edit.".into()),
    }
}

/// Renames a saved connection or changes where it sends requests. Changed
/// details pass the connection test before they are saved, and a connection
/// routing uses switches the router to them at once.
#[tauri::command]
pub(crate) async fn model_router_update_classifier(
    state: tauri::State<'_, RouterState>,
    id: String,
    name: String,
    model: String,
    base_url: String,
    api_key: Option<String>,
) -> Result<RouterSettings, String> {
    let name = name.trim().to_owned();
    if name.is_empty() {
        return Err("Enter a name and model ID.".into());
    }
    let agent = agent()?;
    let saved = {
        let config = load(&agent)?;
        read(&agent, &config.classifier)?
            .into_iter()
            .find(|entry| entry.id == id)
            .ok_or("Choose a connected classifier.")?
    };
    let classifier = edited(&saved.classifier, &model, &base_url, api_key)?;
    if classifier != saved.classifier {
        let probe = classifier.clone();
        tauri::async_runtime::spawn_blocking(move || classify::check(&probe, CLASSIFIER_TIMEOUT))
            .await
            .map_err(|_| "The connection test did not finish.".to_string())??;
    }
    let _guard = MUTATION.lock().map_err(|_| SAVE_ERROR.to_string())?;
    let mut config = load(&agent)?;
    let mut connections = read(&agent, &config.classifier)?;
    let entry = connections
        .iter_mut()
        .find(|entry| entry.id == id)
        .ok_or("Choose a connected classifier.")?;
    let routing = entry.classifier == config.classifier;
    let previous = std::mem::replace(&mut entry.classifier, classifier.clone());
    entry.name = name;
    write(&agent, &connections)?;
    follow_assist(&agent, &previous, Some(&classifier), routing)?;
    if routing {
        config.classifier = classifier;
        save(&agent, &config)?;
        apply(&agent, &state, &config)?;
    }
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
    let removed = connections
        .iter()
        .find(|entry| entry.id == id)
        .map(|entry| (entry.classifier.clone(), entry.classifier == config.classifier));
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
    if let Some((classifier, routing)) = removed {
        follow_assist(&agent, &classifier, None, routing)?;
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
    fn assistance_follows_an_edit_and_turns_off_when_its_model_is_disconnected() {
        let root = std::env::temp_dir().join(format!("assist-test-{}", uuid::Uuid::now_v7()));
        let endpoint = |model: &str| Classifier::Endpoint {
            base_url: "http://127.0.0.1:1/v1/systemone".into(),
            api_key: None,
            model: model.into(),
        };
        let assist = |classifier| config::Assist {
            enabled: true,
            classifier,
        };
        config::save_assist(&root, &assist(Some(endpoint("clef-flash")))).unwrap();
        follow_assist(&root, &endpoint("other"), None, false).unwrap();
        assert_eq!(config::load_assist(&root).unwrap(), assist(Some(endpoint("clef-flash"))));
        follow_assist(&root, &endpoint("clef-flash"), Some(&endpoint("clef")), false).unwrap();
        assert_eq!(config::load_assist(&root).unwrap(), assist(Some(endpoint("clef"))));
        follow_assist(&root, &endpoint("clef"), None, false).unwrap();
        assert_eq!(config::load_assist(&root).unwrap(), config::Assist::default());
        // Assistance that uses the model routing decision model turns off with it.
        config::save_assist(&root, &assist(None)).unwrap();
        follow_assist(&root, &endpoint("jev"), Some(&endpoint("jev-2")), true).unwrap();
        assert_eq!(config::load_assist(&root).unwrap(), assist(None));
        follow_assist(&root, &endpoint("jev-2"), None, true).unwrap();
        assert!(!config::load_assist(&root).unwrap().enabled);
        let _ = std::fs::remove_dir_all(root);
    }

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
    fn an_edit_keeps_the_saved_key_when_the_key_is_blank() {
        let saved = Classifier::Endpoint {
            base_url: "http://127.0.0.1:8000/v1/systemone".into(),
            api_key: Some("saved-key".into()),
            model: "kev-latest".into(),
        };
        let renamed = edited(&saved, " kev-4b ", "http://127.0.0.1:8009/v1/systemone", Some(" ".into())).unwrap();
        assert_eq!(
            renamed,
            Classifier::Endpoint {
                base_url: "http://127.0.0.1:8009/v1/systemone".into(),
                api_key: Some("saved-key".into()),
                model: "kev-4b".into(),
            }
        );
        // The same details compare equal, so a new name alone skips the test.
        assert_eq!(edited(&saved, "kev-latest", "http://127.0.0.1:8000/v1/systemone", None).unwrap(), saved);
        let hosted = Classifier::Typesafe { api_key: "k".into(), model: "jev-latest".into(), base_url: None };
        assert_eq!(
            edited(&hosted, "jev-2", "", Some("new".into())).unwrap(),
            Classifier::Typesafe { api_key: "new".into(), model: "jev-2".into(), base_url: None }
        );
        assert!(edited(&saved, "", "http://127.0.0.1:8000/v1/systemone", None).is_err());
        assert!(edited(&saved, "m", "http://user:pass@127.0.0.1/", None).is_err());
        assert!(edited(&Classifier::Pooled { family: "openai".into(), model: "m".into() }, "m", "", None).is_err());
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

use muniment_core::agents::{self, Agent, AgentList};
use tauri_plugin_opener::OpenerExt;
fn profile() -> Result<std::path::PathBuf, String> {
    muniment_runtime::profile_directory().map_err(|_| "Agents are unavailable.".into())
}
#[tauri::command]
pub fn agent_list() -> Result<AgentList, String> {
    agents::list(&profile()?)
}
#[tauri::command]
pub fn agent_save(agent: Agent) -> Result<Agent, String> {
    agents::save(&profile()?, agent)
}
#[tauri::command]
pub fn agent_delete(id: String) -> Result<(), String> {
    agents::delete(&profile()?, &id)
}
#[tauri::command]
pub fn agent_open(app: tauri::AppHandle, id: String) -> Result<(), String> {
    let folder = agents::folder(&profile()?, &id)?;
    app.opener()
        .open_path(folder.to_string_lossy(), None::<String>)
        .map_err(|_| "The agent folder could not be opened.".into())
}

#[tauri::command]
pub fn agent_run(id: String) -> Result<(), String> {
    agents::queue_run(&profile()?, &id)
}

#[tauri::command]
pub async fn agent_export_template(
    app: tauri::AppHandle,
    content: String,
    name: String,
    format: Option<String>,
) -> Result<bool, String> {
    use tauri_plugin_dialog::DialogExt;
    if content.len() > 131072 {
        return Err("The template exceeds 128 KB.".into());
    }
    let extension = if format.as_deref() == Some("markdown") {
        "md"
    } else {
        "json"
    };
    let file_name: String = name
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '-' || *c == ' ')
        .take(80)
        .collect();
    tauri::async_runtime::spawn_blocking(move || {
        let selected = app
            .dialog()
            .file()
            .set_file_name(format!(
                "{}.agent.{}",
                if file_name.is_empty() {
                    "agent"
                } else {
                    &file_name
                },
                extension
            ))
            .add_filter("Agent template", &[extension])
            .blocking_save_file();
        let Some(selected) = selected else {
            return Ok(false);
        };
        let path = selected.into_path().map_err(|_| "Choose a local file.")?;
        std::fs::write(path, content).map_err(|_| "The template could not be exported.")?;
        Ok(true)
    })
    .await
    .map_err(|_| "The template export could not finish.".to_string())?
}

#[tauri::command]
pub async fn agent_import_link(url: String) -> Result<serde_json::Value, String> {
    tauri::async_runtime::spawn_blocking(move || {
        muniment_core::agent_templates::fetch_public_template(&url)
    })
    .await
    .map_err(|_| "The template could not be loaded.".to_string())?
}

#[tauri::command]
pub fn agent_memory(
    id: String,
    action: String,
    fact: Option<muniment_core::memory_files::Fact>,
    fact_id: Option<String>,
) -> Result<serde_json::Value, String> {
    use muniment_core::memory_files as memory;
    let (home, private) = agents::memory_paths(&profile()?, &id)?;
    match action.as_str() {
        "memory_facts" => Ok(serde_json::json!(memory::facts_in(&home)?)),
        "memory_deleted_facts" => Ok(serde_json::json!(memory::deleted_facts(&private)?)),
        "memory_fact_save" => Ok(serde_json::json!(memory::fact_save_in(
            &private,
            &home,
            fact.ok_or("Give the memory a title and fact.")?
        )?)),
        "memory_fact_delete" => {
            memory::fact_delete_in(&private, &home, &fact_id.ok_or("Choose a memory.")?)?;
            Ok(serde_json::Value::Null)
        }
        "memory_fact_restore" => {
            memory::fact_restore_in(&private, &home, &fact_id.ok_or("Choose a memory.")?)?;
            Ok(serde_json::Value::Null)
        }
        _ => Err("The memory action is unknown.".into()),
    }
}

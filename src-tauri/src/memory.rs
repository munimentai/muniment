#[cfg(test)]
use muniment_core::memory_index::RecallRecord;
use muniment_core::memory_index::{
    MemoryIndexError, MemoryRuntimeSession, MemorySearchResult, ModelMemoryCapability,
};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tauri::State;

pub(crate) struct ApplicationMemoryRuntime {
    config: PathBuf,
    database_root: PathBuf,
    sessions: Mutex<BTreeMap<String, MemoryRuntimeSession>>,
}

impl ApplicationMemoryRuntime {
    pub(crate) fn new(config: PathBuf, database_root: PathBuf) -> Self {
        Self {
            config,
            database_root,
            sessions: Mutex::new(BTreeMap::new()),
        }
    }

    pub(crate) fn open_session(
        &self,
        session: &str,
        thread: &str,
        capability: ModelMemoryCapability,
    ) -> Result<(), MemoryIndexError> {
        let home = muniment_core::home::configured_home(&self.config)
            .map_err(|error| MemoryIndexError::Io(std::io::Error::other(error.to_string())))?
            .ok_or(MemoryIndexError::InvalidPath)?;
        let database = self.database_root.join("memory-index.sqlite3");
        self.open_session_for_home(session, thread, capability, &home, database);
        Ok(())
    }

    fn open_session_for_home(
        &self,
        session: &str,
        thread: &str,
        capability: ModelMemoryCapability,
        home: &Path,
        database: PathBuf,
    ) {
        self.sessions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(
                session.to_owned(),
                MemoryRuntimeSession::open(home, database, thread, capability),
            );
    }

    pub(crate) fn tool_definition_for_turn(&self, session: &str) -> Option<Vec<u8>> {
        self.sessions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(session)
            .map(|session| session.tool_definition_for_turn().to_vec())
    }

    pub(crate) fn dispatch_tool_call(
        &self,
        session: &str,
        name: &str,
        arguments: &[u8],
    ) -> Result<MemorySearchResult, MemoryIndexError> {
        if name != "memory-search" {
            return Err(MemoryIndexError::InvalidToolArguments);
        }
        self.sessions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get_mut(session)
            .ok_or(MemoryIndexError::InvalidToolArguments)?
            .call(arguments)
    }

    #[cfg(test)]
    fn recall_records(&self, session: &str) -> Vec<RecallRecord> {
        self.sessions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(session)
            .map(|session| session.recall_records().to_vec())
            .unwrap_or_default()
    }

    pub(crate) fn close_session(&self, session: &str) {
        self.sessions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(session);
    }
}

#[tauri::command]
pub(crate) fn memory_tools(
    state: State<'_, ApplicationMemoryRuntime>,
    run_id: String,
) -> Result<serde_json::Value, String> {
    let definition = state
        .tool_definition_for_turn(&run_id)
        .ok_or_else(|| "That reply is no longer active.".to_string())?;
    serde_json::from_slice(&definition).map_err(|_| "The memory tool is unavailable.".to_string())
}

#[tauri::command]
pub(crate) fn memory_tool_call(
    state: State<'_, ApplicationMemoryRuntime>,
    run_id: String,
    tool_name: String,
    arguments: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let arguments = serde_json::to_vec(&arguments)
        .map_err(|_| "The memory search arguments are invalid.".to_string())?;
    let result = state
        .dispatch_tool_call(&run_id, &tool_name, &arguments)
        .map_err(|_| "The memory search failed.".to_string())?;
    serde_json::to_value(result).map_err(|_| "The memory search failed.".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tauri::Manager;
    use uuid::Uuid;

    #[test]
    fn application_session_declares_dispatches_and_records_memory_across_two_turns() {
        let root = std::env::temp_dir().join(format!("muniment-app-memory-{}", Uuid::now_v7()));
        let home = root.join("home");
        fs::create_dir_all(home.join("memory")).unwrap();
        fs::write(home.join("memory/fact.md"), "saffron belongs in the pantry").unwrap();
        let runtime = ApplicationMemoryRuntime::new(root.join("config"), root.join("cache"));
        runtime.open_session_for_home(
            "session-1",
            "thread-1",
            ModelMemoryCapability {
                minimum_cacheable_prefix_characters: 100,
            },
            &home,
            root.join("cache/index.sqlite3"),
        );

        let app = tauri::test::mock_app();
        app.manage(runtime);
        let first_definition = memory_tools(app.state(), "session-1".into()).unwrap();
        let first: MemorySearchResult = serde_json::from_value(
            memory_tool_call(
                app.state(),
                "session-1".into(),
                "memory-search".into(),
                serde_json::json!({"query": "saffron"}),
            )
            .unwrap(),
        )
        .unwrap();
        let second_definition = memory_tools(app.state(), "session-1".into()).unwrap();
        let second: MemorySearchResult = serde_json::from_value(
            memory_tool_call(
                app.state(),
                "session-1".into(),
                "memory-search".into(),
                serde_json::json!({"query": "saffron"}),
            )
            .unwrap(),
        )
        .unwrap();

        assert_eq!(first_definition, second_definition);
        assert_eq!(first.items, second.items);
        assert_eq!(first.items.len(), 1);
        assert_eq!(first.items[0].path, "memory/fact.md");
        assert_eq!(
            app.state::<ApplicationMemoryRuntime>()
                .recall_records("session-1")
                .len(),
            2
        );
        app.state::<ApplicationMemoryRuntime>()
            .close_session("session-1");
        assert!(memory_tools(app.state(), "session-1".into()).is_err());
        fs::remove_dir_all(root).unwrap();
    }
}

use keyring::Entry;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

const PROMPT_SERVICE: &str = "ai.muniment.desktop.chat";
const PROMPT_USER: &str = "protected-prompts";

#[derive(Debug)]
pub enum ChatPromptError {
    CachePoisoned,
    Entry(keyring::Error),
    Store(keyring::Error),
    Load(keyring::Error),
}

fn entries() -> &'static Mutex<HashMap<String, Entry>> {
    static ENTRIES: OnceLock<Mutex<HashMap<String, Entry>>> = OnceLock::new();
    ENTRIES.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn prompt_user(subject: Option<&str>, run_id: &str) -> String {
    subject.filter(|value| !value.is_empty()).map_or_else(
        || format!("{PROMPT_USER}:{run_id}"),
        |value| format!("{PROMPT_USER}:{value}:{run_id}"),
    )
}

pub fn store_prompt(
    run_id: &str,
    prompt: &str,
    subject: Option<&str>,
) -> Result<(), ChatPromptError> {
    let user = prompt_user(subject, run_id);
    let mut entries = entries()
        .lock()
        .map_err(|_| ChatPromptError::CachePoisoned)?;
    let entry = match entries.entry(user) {
        std::collections::hash_map::Entry::Occupied(entry) => entry.into_mut(),
        std::collections::hash_map::Entry::Vacant(entry) => {
            let credential =
                Entry::new(PROMPT_SERVICE, entry.key()).map_err(ChatPromptError::Entry)?;
            entry.insert(credential)
        }
    };
    entry.set_password(prompt).map_err(ChatPromptError::Store)
}

pub fn load_prompt(run_id: &str, subject: Option<&str>) -> Result<Option<String>, ChatPromptError> {
    let user = prompt_user(subject, run_id);
    let mut entries = entries()
        .lock()
        .map_err(|_| ChatPromptError::CachePoisoned)?;
    let entry = match entries.entry(user) {
        std::collections::hash_map::Entry::Occupied(entry) => entry.into_mut(),
        std::collections::hash_map::Entry::Vacant(entry) => {
            let credential =
                Entry::new(PROMPT_SERVICE, entry.key()).map_err(ChatPromptError::Entry)?;
            entry.insert(credential)
        }
    };
    match entry.get_password() {
        Ok(prompt) => Ok(Some(prompt)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(error) => Err(ChatPromptError::Load(error)),
    }
}

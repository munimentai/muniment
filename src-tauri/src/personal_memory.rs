use muniment_core::memory_files::{self, Fact};
fn profile() -> Result<std::path::PathBuf, String> {
    muniment_runtime::profile_directory().map_err(|_| "Memory is unavailable.".into())
}
#[tauri::command]
pub fn memory_profile_read() -> Result<String, String> {
    memory_files::profile_read(&profile()?)
}
#[tauri::command]
pub fn memory_profile_save(content: String) -> Result<(), String> {
    memory_files::profile_save(&profile()?, &content)
}
#[tauri::command]
pub fn memory_facts() -> Result<Vec<Fact>, String> {
    memory_files::facts(&profile()?)
}
#[tauri::command]
pub fn memory_fact_save(fact: Fact) -> Result<Fact, String> {
    memory_files::fact_save(&profile()?, fact)
}
#[tauri::command]
pub fn memory_fact_delete(id: String) -> Result<(), String> {
    memory_files::fact_delete(&profile()?, &id)
}

#[tauri::command]
pub fn memory_deleted_facts() -> Result<Vec<Fact>, String> {
    memory_files::deleted_facts(&profile()?)
}
#[tauri::command]
pub fn memory_fact_restore(id: String) -> Result<(), String> {
    memory_files::fact_restore(&profile()?, &id)
}

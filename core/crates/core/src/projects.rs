//! Local project folders and stable thread membership.
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::Mutex,
};

static WRITE: Mutex<()> = Mutex::new(());

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Catalog {
    pub projects: BTreeMap<String, String>,
    pub threads: BTreeMap<String, String>,
    /// Projects another swarm member owns, by id. Their files stay on the
    /// owner.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub remote: BTreeMap<String, RemoteProject>,
    /// Each project's defaults for its new threads, by id.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub settings: BTreeMap<String, Defaults>,
}

/// What a new thread in a project starts with. Every run gets every tool its
/// machine offers; the `tools` list an older catalog holds is ignored, and a
/// save drops it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Defaults {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<DefaultModel>,
    /// A permission mode id from `permission.modes`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub permission_mode: Option<String>,
    /// The runtime id of the machine a new thread runs on. None is this computer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub machine: Option<String>,
}

/// A provider and model id.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DefaultModel {
    pub provider: String,
    pub model: String,
}

/// A project's settings: its defaults and the instructions every run in its
/// folder reads from the folder's `AGENTS.md`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    #[serde(flatten)]
    pub defaults: Defaults,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
}

/// A project whose folder is on another swarm member.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteProject {
    pub name: String,
    pub owner_runtime: String,
    /// The folder on the owner.
    #[serde(default)]
    pub path: String,
}

/// The id the retired built-in Chats project used. A thread assigned to it,
/// or a seed from it, belongs to no project.
pub const RETIRED_CHATS: &str = "chats";

fn read(profile: &Path) -> Result<Catalog, String> {
    match fs::read(profile.join("projects.json")) {
        Ok(bytes) => {
            let mut catalog: Catalog = serde_json::from_slice(&bytes)
                .map_err(|_| "The project catalog cannot be read.".to_string())?;
            if !catalog.projects.contains_key(RETIRED_CHATS) {
                catalog
                    .threads
                    .retain(|_, project| project != RETIRED_CHATS);
                catalog.settings.remove(RETIRED_CHATS);
            }
            Ok(catalog)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Catalog::default()),
        Err(_) => Err("The project catalog cannot be read.".into()),
    }
}
fn save(profile: &Path, catalog: &Catalog) -> Result<(), String> {
    fs::create_dir_all(profile).map_err(|_| "The project catalog cannot be saved.")?;
    let temporary = profile.join(format!("projects.{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| {
        fs::write(
            &temporary,
            serde_json::to_vec(catalog).map_err(|_| "The project catalog cannot be saved.")?,
        )
        .map_err(|_| "The project catalog cannot be saved.")?;
        fs::rename(&temporary, profile.join("projects.json"))
            .map_err(|_| "The project catalog cannot be saved.")
    })();
    let _ = fs::remove_file(temporary);
    result.map_err(str::to_owned)
}
pub fn root(profile: &Path) -> Result<PathBuf, String> {
    let home = crate::home::configured_home(profile)
        .map_err(|e| e.to_string())?
        .ok_or("Choose a Home folder before creating projects.")?;
    let root = home.join("projects");
    if root.is_symlink() {
        return Err("The projects folder must not be a symbolic link.".into());
    }
    fs::create_dir_all(&root).map_err(|_| "The projects folder cannot be opened.")?;
    Ok(root)
}
fn name(value: &str) -> Result<&str, String> {
    let value = value.trim();
    let stem = value.split('.').next().unwrap_or("").to_ascii_uppercase();
    if value.is_empty()
        || value.chars().count() > 80
        || value.starts_with('.')
        || value.ends_with('.')
        || value
            .chars()
            .any(|c| c.is_control() || "/\\<>:\"|?*".contains(c))
        || matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (stem.len() == 4
            && (stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem.as_bytes()[3].is_ascii_digit())
    {
        return Err("Use a project name without path separators or reserved characters.".into());
    }
    Ok(value)
}
pub fn list(profile: &Path) -> Result<Catalog, String> {
    let _guard = WRITE.lock().map_err(|_| "The project catalog is busy.")?;
    let mut catalog = read(profile)?;
    let root = root(profile)?;
    // A project whose folder is gone, such as one on a drive that is not
    // mounted, drops out of the list and stays in the catalog, so it returns
    // with its folder. One missing folder never hides the other projects.
    let mut known = Vec::new();
    let mut missing = Vec::new();
    for (id, title) in &catalog.projects {
        match crate::workspace_names::resolve(
            profile,
            &root,
            id,
            Some(title),
            Some(&root.join(title)),
        ) {
            Ok(folder) => known.push(folder),
            Err(_) => missing.push(id.clone()),
        }
    }
    // A folder the name index records and no project holds is a deleted
    // project's folder that kept the user's files, so it stays out.
    let recorded = crate::workspace_names::recorded_names(profile, &root)?;
    let mut changed = false;
    for entry in fs::read_dir(&root).map_err(|_| "The projects folder cannot be read.")? {
        let entry = entry.map_err(|_| "A project folder cannot be read.")?;
        if !entry
            .file_type()
            .map_err(|_| "A project folder cannot be read.")?
            .is_dir()
        {
            continue;
        }
        let folder = entry.file_name().to_string_lossy().into_owned();
        if name(&folder).is_ok()
            && !known.contains(&entry.path())
            && !recorded.contains(&folder)
            && !catalog.projects.values().any(|n| n == &folder)
        {
            catalog
                .projects
                .insert(uuid::Uuid::new_v4().to_string(), folder);
            changed = true;
        }
    }
    if changed {
        save(profile, &catalog)?;
    }
    catalog.projects.retain(|id, _| !missing.contains(id));
    catalog
        .threads
        .retain(|_, project| !missing.contains(project));
    Ok(catalog)
}
pub fn create(profile: &Path, value: &str) -> Result<String, String> {
    let value = name(value)?;
    let _guard = WRITE.lock().map_err(|_| "The project catalog is busy.")?;
    let mut catalog = read(profile)?;
    let folder = root(profile)?.join(value);
    if catalog
        .projects
        .values()
        .any(|n| n.eq_ignore_ascii_case(value))
        || folder.exists()
    {
        return Err("A project with that name already exists.".into());
    }
    fs::create_dir(&folder).map_err(|_| "The project folder could not be created.")?;
    let id = uuid::Uuid::new_v4().to_string();
    catalog.projects.insert(id.clone(), value.into());
    if let Err(error) = save(profile, &catalog) {
        let _ = fs::remove_dir(folder);
        return Err(error);
    }
    Ok(id)
}
pub fn folder(profile: &Path, id: &str) -> Result<PathBuf, String> {
    folder_with_config(profile, profile, id)
}

fn folder_with_config(profile: &Path, config: &Path, id: &str) -> Result<PathBuf, String> {
    let catalog = read(profile)?;
    let value = catalog
        .projects
        .get(id)
        .ok_or("The project does not exist.")?;
    let root = root(config)?;
    let folder = crate::workspace_names::resolve(
        config,
        &root,
        id,
        Some(name(value)?),
        Some(&root.join(value)),
    )?;
    if folder.is_symlink() || !folder.is_dir() {
        return Err("The project folder is unavailable.".into());
    }
    Ok(folder)
}
pub fn rename(profile: &Path, id: &str, value: &str) -> Result<(), String> {
    let value = name(value)?;
    let _guard = WRITE.lock().map_err(|_| "The project catalog is busy.")?;
    let mut catalog = read(profile)?;
    let old = catalog
        .projects
        .get(id)
        .ok_or("The project does not exist.")?
        .clone();
    if old == value {
        return Ok(());
    }
    if catalog
        .projects
        .iter()
        .any(|(key, n)| key != id && n.eq_ignore_ascii_case(value))
    {
        return Err("A project with that name already exists.".into());
    }
    let source = folder(profile, id)?;
    let target = root(profile)?.join(crate::workspace_names::basename(value, id));
    if target.exists() && !old.eq_ignore_ascii_case(value) {
        return Err("A folder with that name already exists.".into());
    }
    crate::workspace_names::resolve(profile, &root(profile)?, id, Some(value), Some(&source))?;
    catalog.projects.insert(id.into(), value.into());
    if let Err(error) = save(profile, &catalog) {
        crate::workspace_names::resolve(profile, &root(profile)?, id, Some(&old), Some(&target))?;
        return Err(error);
    }
    Ok(())
}
/// What a project delete says when the project's folder stays.
pub const PROJECT_FOLDER_KEPT: &str = "The project's folder holds your files, so it stays.";

/// Deletes a project of this computer. Its threads stay and leave it, each
/// a project of its own, and its settings go. Its folder goes when it holds
/// only Muniment's own files. A folder with the user's files stays out of
/// the list, and the answer names it. A project an agent works in stays.
pub fn delete(profile: &Path, id: &str) -> Result<Option<PathBuf>, String> {
    if crate::agents::list(profile)?
        .agents
        .iter()
        .any(|agent| agent.project_id.as_deref() == Some(id))
    {
        return Err("An agent works in this project. Move or delete the agent first.".into());
    }
    let root = root(profile)?;
    {
        let _guard = WRITE.lock().map_err(|_| "The project catalog is busy.")?;
        let mut catalog = read(profile)?;
        if !catalog.projects.contains_key(id) {
            return Err("The project does not exist.".into());
        }
        // Resolving the folder records its name, so the removal finds it.
        let _ = folder(profile, id);
        catalog.projects.remove(id);
        catalog.settings.remove(id);
        catalog.threads.retain(|_, project| project != id);
        save(profile, &catalog)?;
    }
    crate::workspace_names::remove_recorded(profile, &root, id, crate::agents::remove_folder)
}
/// Drops a deleted thread from its project.
pub fn forget_thread(profile: &Path, thread: &str) -> Result<(), String> {
    update(profile, |catalog| catalog.threads.remove(thread).is_some())
}
/// The pass at start over the catalog: it drops each thread the journal
/// deleted from its project. Answers the count.
pub fn forget_deleted_threads(profile: &Path) -> Result<usize, String> {
    let journal_path = profile.join("runs.sqlite3");
    if !journal_path.exists() || saved(profile)?.threads.is_empty() {
        return Ok(0);
    }
    let mut journal = crate::journal::RunJournal::open(journal_path).map_err(|e| e.to_string())?;
    let mut deleted = Vec::new();
    for thread in saved(profile)?.threads.keys() {
        if journal.thread_deleted(thread).map_err(|e| e.to_string())? {
            deleted.push(thread.clone());
        }
    }
    if !deleted.is_empty() {
        update(profile, |catalog| {
            let before = catalog.threads.len();
            catalog
                .threads
                .retain(|thread, _| !deleted.contains(thread));
            catalog.threads.len() != before
        })?;
    }
    Ok(deleted.len())
}
pub fn assign(profile: &Path, thread: &str, project: &str) -> Result<(), String> {
    folder(profile, project)?;
    let _guard = WRITE.lock().map_err(|_| "The project catalog is busy.")?;
    let mut catalog = read(profile)?;
    catalog.threads.insert(thread.into(), project.into());
    save(profile, &catalog)
}
pub fn unassign(profile: &Path, thread: &str) -> Result<(), String> {
    let _guard = WRITE.lock().map_err(|_| "The project catalog is busy.")?;
    let mut catalog = read(profile)?;
    catalog.threads.remove(thread);
    save(profile, &catalog)
}
/// Moves a thread into a catalog project, or out of every project with
/// None, which makes it a project of its own. Its conversation keeps its id,
/// so its history and session go with it, and its next run works in the new
/// project's folder. Files it wrote stay where it wrote them. An agent's
/// thread stays in its agent's project.
pub fn move_thread(profile: &Path, thread: &str, project: Option<&str>) -> Result<(), String> {
    if crate::agents::state(profile)?.threads.contains_key(thread) {
        return Err("An agent's thread stays in its agent's project.".into());
    }
    let Some(project) = project else {
        return unassign(profile, thread);
    };
    if saved(profile)?.remote.contains_key(project) {
        return Err("Move threads into that project on the machine that holds it.".into());
    }
    assign(profile, thread, project)
}
/// The project a thread belongs to: its agent's project or its catalog
/// project. None for a thread in no project.
pub fn thread_project(profile: &Path, thread: &str) -> Result<Option<String>, String> {
    if let Some(agent) = crate::agents::thread_agent(profile, thread)? {
        return Ok(Some(agent.project_id.unwrap_or_else(|| {
            format!("{}{}", crate::sidebar::AGENT_PROJECT_PREFIX, agent.id)
        })));
    }
    Ok(read(profile)?.threads.get(thread).cloned())
}
pub fn thread_folder(profile: &Path, thread: &str) -> Result<Option<PathBuf>, String> {
    let catalog = read(profile)?;
    catalog
        .threads
        .get(thread)
        .filter(|id| !catalog.remote.contains_key(*id))
        .map(|id| folder(profile, id))
        .transpose()
}

const INSTRUCTIONS: &str = "AGENTS.md";
const MAX_INSTRUCTIONS: usize = 64 * 1024;

/// A catalog project's settings.
pub fn settings(profile: &Path, id: &str) -> Result<Settings, String> {
    let defaults = read(profile)?.settings.get(id).cloned().unwrap_or_default();
    let file = folder(profile, id)?.join(INSTRUCTIONS);
    let instructions = match fs::read_to_string(&file) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(_) => return Err("The project instructions cannot be read.".into()),
    };
    Ok(Settings {
        defaults,
        instructions: Some(instructions),
    })
}

/// Saves a project's settings. Empty instructions remove the folder's
/// `AGENTS.md`, and None leaves the file as it is.
pub fn save_settings(profile: &Path, id: &str, settings: &Settings) -> Result<(), String> {
    let folder = folder(profile, id)?;
    if let Some(text) = &settings.instructions {
        if text.len() > MAX_INSTRUCTIONS {
            return Err("Project instructions must be under 64 KB.".into());
        }
        let file = folder.join(INSTRUCTIONS);
        if file.is_symlink() {
            return Err("The project instructions file must not be a symbolic link.".into());
        }
        let result = if text.trim().is_empty() {
            match fs::remove_file(&file) {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
                other => other,
            }
        } else {
            fs::write(&file, text)
        };
        result.map_err(|_| "The project instructions could not be saved.")?;
    }
    let defaults = settings.defaults.clone();
    update(profile, |catalog| {
        if defaults == Defaults::default() {
            catalog.settings.remove(id).is_some()
        } else {
            catalog
                .settings
                .insert(id.into(), defaults.clone())
                .as_ref()
                != Some(&defaults)
        }
    })
}

/// The catalog as saved, with projects whose folder is missing.
pub(crate) fn saved(profile: &Path) -> Result<Catalog, String> {
    let _guard = WRITE.lock().map_err(|_| "The project catalog is busy.")?;
    read(profile)
}

/// Changes the saved catalog under the catalog lock.
pub(crate) fn update(
    profile: &Path,
    change: impl FnOnce(&mut Catalog) -> bool,
) -> Result<(), String> {
    let _guard = WRITE.lock().map_err(|_| "The project catalog is busy.")?;
    let mut catalog = read(profile)?;
    if change(&mut catalog) {
        save(profile, &catalog)?;
    }
    Ok(())
}

/// Resolves every thread to its project or its own visible output folder.
pub fn workspace(profile: &Path, thread: &str) -> Result<PathBuf, String> {
    workspace_with_config(profile, profile, thread)
}

pub fn workspace_with_config(
    profile: &Path,
    config: &Path,
    thread: &str,
) -> Result<PathBuf, String> {
    if let Some(id) = crate::agents::state(profile)?.threads.get(thread) {
        let agent = crate::agents::get(config, id)?;
        if let Some(project) = agent.project_id {
            return folder_with_config(profile, config, &project);
        }
        return crate::agents::folder(config, id);
    }
    let catalog = read(profile)?;
    // A thread in another member's project works in its own folder here.
    if let Some(project) = catalog
        .threads
        .get(thread)
        .filter(|project| !catalog.remote.contains_key(*project))
    {
        let folder = folder_with_config(profile, config, project)?;
        migrate_workspace_metadata(&folder)?;
        return Ok(folder);
    }
    if thread.is_empty()
        || thread.len() > 128
        || !thread
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
    {
        return Err("The thread identifier is invalid.".into());
    }
    let home = crate::home::configured_home(config)
        .map_err(|e| e.to_string())?
        .ok_or("The Home folder is unavailable.")?;
    let sessions = home.join("sessions");
    let folder = crate::workspace_names::resolve(config, &sessions, thread, None, None)?;
    if sessions.is_symlink() || folder.is_symlink() {
        return Err("The session folder must not be a symbolic link.".into());
    }
    fs::create_dir_all(&folder).map_err(|_| "The session folder cannot be opened.")?;
    migrate_workspace_metadata(&folder)?;
    Ok(folder)
}

/// Names a thread's own session folder after its title and answers the
/// folder. A thread that works in an agent's or a project's folder keeps
/// that folder and answers `None`. The name index lives under `config`.
pub fn name_thread_workspace(
    profile: &Path,
    config: &Path,
    thread: &str,
    title: &str,
) -> Result<Option<PathBuf>, String> {
    if title.trim().is_empty() || crate::agents::state(profile)?.threads.contains_key(thread) {
        return Ok(None);
    }
    let catalog = read(profile)?;
    if catalog
        .threads
        .get(thread)
        .is_some_and(|project| !catalog.remote.contains_key(project))
    {
        return Ok(None);
    }
    let home = crate::home::configured_home(config)
        .map_err(|e| e.to_string())?
        .ok_or("The Home folder is unavailable.")?;
    let sessions = home.join("sessions");
    if sessions.is_symlink() {
        return Err("The session folder must not be a symbolic link.".into());
    }
    crate::workspace_names::resolve(config, &sessions, thread, Some(title), None).map(Some)
}

/// The Home's session folder, when Home is set and the folder reads as one.
fn sessions_root(profile: &Path) -> Result<Option<PathBuf>, String> {
    let Some(home) = crate::home::configured_home(profile).map_err(|e| e.to_string())? else {
        return Ok(None);
    };
    let root = home.join("sessions");
    Ok((root.is_dir() && !root.is_symlink()).then_some(root))
}

/// What a thread delete says when the thread's folder stays.
pub const THREAD_FOLDER_KEPT: &str = "The thread's folder holds your files, so it stays.";

/// Removes a deleted thread's own session folder when it holds only
/// Muniment's own files, and its name record with it. A folder with the
/// user's files stays, and the answer names it.
pub fn remove_thread_folder(profile: &Path, thread: &str) -> Result<Option<PathBuf>, String> {
    let Some(root) = sessions_root(profile)? else {
        return Ok(None);
    };
    crate::workspace_names::remove_recorded(profile, &root, thread, crate::agents::remove_folder)
}

/// The pass at start over the session folders: it removes each folder of a
/// thread the journal deleted that holds only Muniment's own files, and
/// forgets each name record whose folder is gone. It never removes a folder
/// that holds the user's files. Answers the names of the removed folders.
pub fn remove_deleted_thread_folders(profile: &Path) -> Result<Vec<String>, String> {
    let mut removed = Vec::new();
    if let (Some(root), true) = (
        sessions_root(profile)?,
        profile.join("runs.sqlite3").exists(),
    ) {
        let mut journal = crate::journal::RunJournal::open(profile.join("runs.sqlite3"))
            .map_err(|e| e.to_string())?;
        for thread in crate::workspace_names::recorded_ids(profile, &root)? {
            if !journal.thread_deleted(&thread).map_err(|e| e.to_string())? {
                continue;
            }
            let Some(name) = crate::workspace_names::recorded(profile, &root, &thread)? else {
                continue;
            };
            if remove_thread_folder(profile, &thread)?.is_none() {
                removed.push(name);
            }
        }
    }
    let forgotten = crate::workspace_names::forget_missing(profile)?;
    if forgotten > 0 {
        crate::runtime_eprintln!("muniment-runtime: workspace_names forgot_missing={forgotten}");
    }
    Ok(removed)
}

/// Converts legacy session directories using the journal's visible thread titles.
pub fn migrate_session_names(profile: &Path) -> Result<(), String> {
    let Some(home) = crate::home::configured_home(profile).map_err(|e| e.to_string())? else {
        return Ok(());
    };
    let root = home.join("sessions");
    if !root.is_dir() || root.is_symlink() {
        return Ok(());
    }
    let removed = crate::workspace_names::remove_unused_untitled(profile, &root)?;
    if removed > 0 {
        crate::runtime_eprintln!(
            "muniment-runtime: session_names removed_empty_untitled={removed}"
        );
    }
    let ids = fs::read_dir(&root)
        .map_err(|e| e.to_string())?
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let id = entry.file_name().to_string_lossy().into_owned();
            (uuid::Uuid::parse_str(&id).is_ok()
                && entry.path().is_dir()
                && !entry.path().is_symlink())
            .then_some(id)
        })
        .collect::<Vec<_>>();
    if ids.is_empty() {
        return Ok(());
    }
    let mut titles = BTreeMap::new();
    if profile.join("runs.sqlite3").exists() {
        let mut journal = crate::journal::RunJournal::open(profile.join("runs.sqlite3"))
            .map_err(|e| e.to_string())?;
        let mut cursor = None;
        loop {
            let page = journal
                .thread_summaries(100, cursor.as_deref())
                .map_err(|e| e.to_string())?;
            for summary in page.summaries {
                titles.insert(summary.thread_id, summary.title);
            }
            cursor = page.next_cursor;
            if cursor.is_none() {
                break;
            }
        }
    }
    for id in ids {
        crate::workspace_names::resolve(
            profile,
            &root,
            &id,
            Some(titles.get(&id).map(String::as_str).unwrap_or("Thread")),
            None,
        )?;
    }
    Ok(())
}

fn migrate_workspace_metadata(folder: &Path) -> Result<(), String> {
    let old = folder.join(".pi");
    let new = folder.join(".muniment");
    if !old.exists() && !old.is_symlink() {
        return Ok(());
    }
    if old.is_symlink() || !old.is_dir() || new.exists() || new.is_symlink() {
        return Err(
            "The session metadata folder cannot be renamed without overwriting existing files."
                .into(),
        );
    }
    fs::rename(old, new).map_err(|_| "The session metadata folder could not be renamed.".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let root =
                std::env::temp_dir().join(format!("muniment-project-{}", uuid::Uuid::new_v4()));
            fs::create_dir_all(&root).unwrap();
            crate::home::confirm_home(&root.join("profile"), &root.join("muniment")).unwrap();
            Self(root)
        }
        fn profile(&self) -> PathBuf {
            self.0.join("profile")
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    #[test]
    fn unassigned_threads_use_separate_session_folders() {
        let fixture = Fixture::new();
        let profile = fixture.profile();
        let first = workspace(&profile, "thread-one").unwrap();
        assert!(first
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("Untitled-"));
        fs::write(first.join("result.txt"), "kept").unwrap();
        fs::create_dir_all(first.join(".pi/tasks")).unwrap();
        fs::write(first.join(".pi/tasks/result.txt"), "task output").unwrap();
        assert_ne!(first, workspace(&profile, "thread-two").unwrap());
        assert_eq!(first, workspace(&profile, "thread-one").unwrap());
        assert!(!first.join(".pi").exists());
        assert_eq!(
            fs::read_to_string(first.join(".muniment/tasks/result.txt")).unwrap(),
            "task output"
        );
        assert!(workspace(&profile, "../outside").is_err());
        let project = create(&profile, "Research").unwrap();
        assign(&profile, "thread-two", &project).unwrap();
        assert_eq!(
            workspace(&profile, "thread-two").unwrap(),
            folder(&profile, &project).unwrap()
        );
        assert_eq!(
            fs::read_to_string(first.join("result.txt")).unwrap(),
            "kept"
        );
    }
    #[test]
    fn a_titled_thread_names_its_own_folder_and_a_project_thread_keeps_the_project() {
        let fixture = Fixture::new();
        let profile = fixture.profile();
        let untitled = workspace(&profile, "db6632f9-thread").unwrap();
        fs::write(untitled.join("notes.txt"), "kept").unwrap();
        let named =
            name_thread_workspace(&profile, &profile, "db6632f9-thread", "Bubbles in water")
                .unwrap()
                .unwrap();
        assert!(named
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("Bubbles_in_water-"));
        assert!(!untitled.exists());
        assert_eq!(fs::read_to_string(named.join("notes.txt")).unwrap(), "kept");
        // The thread's workspace is the named folder from here on.
        assert_eq!(workspace(&profile, "db6632f9-thread").unwrap(), named);
        // A second pass with the same title leaves the folder in place.
        assert_eq!(
            name_thread_workspace(&profile, &profile, "db6632f9-thread", "Bubbles in water")
                .unwrap(),
            Some(named.clone())
        );
        assert_eq!(
            name_thread_workspace(&profile, &profile, "db6632f9-thread", " ").unwrap(),
            None
        );
        let project = create(&profile, "Research").unwrap();
        assign(&profile, "thread-two", &project).unwrap();
        assert_eq!(
            name_thread_workspace(&profile, &profile, "thread-two", "Other").unwrap(),
            None
        );
        assert_eq!(
            workspace(&profile, "thread-two").unwrap(),
            folder(&profile, &project).unwrap()
        );
    }
    #[test]
    fn a_deleted_threads_folder_goes_unless_it_holds_the_users_files() {
        let fixture = Fixture::new();
        let profile = fixture.profile();
        let only_tasks = workspace(&profile, "thread-one").unwrap();
        fs::create_dir_all(only_tasks.join(".muniment/tasks/run-1")).unwrap();
        fs::write(only_tasks.join(".muniment/tasks/run-1/output.txt"), "done").unwrap();
        fs::write(only_tasks.join(".DS_Store"), "finder").unwrap();
        assert_eq!(remove_thread_folder(&profile, "thread-one").unwrap(), None);
        assert!(!only_tasks.exists());
        let root = only_tasks.parent().unwrap().to_path_buf();
        assert_eq!(
            crate::workspace_names::recorded(&profile, &root, "thread-one").unwrap(),
            None
        );

        let mine = workspace(&profile, "thread-two").unwrap();
        fs::create_dir_all(mine.join(".muniment/tasks")).unwrap();
        fs::write(mine.join("report.md"), "mine").unwrap();
        assert_eq!(
            remove_thread_folder(&profile, "thread-two").unwrap(),
            Some(mine.clone())
        );
        assert_eq!(fs::read_to_string(mine.join("report.md")).unwrap(), "mine");
        assert!(
            crate::workspace_names::recorded(&profile, &root, "thread-two")
                .unwrap()
                .is_some()
        );
        // A thread with no folder of its own leaves the rest alone.
        assert_eq!(
            remove_thread_folder(&profile, "thread-three").unwrap(),
            None
        );
    }

    #[test]
    fn the_startup_pass_removes_empty_untitled_folders_no_thread_names() {
        let fixture = Fixture::new();
        let profile = fixture.profile();
        let kept = workspace(&profile, "thread-one").unwrap();
        let sessions = kept.parent().unwrap().to_path_buf();
        fs::create_dir_all(sessions.join("Untitled-orphan01")).unwrap();
        migrate_session_names(&profile).unwrap();
        assert!(kept.is_dir());
        assert!(!sessions.join("Untitled-orphan01").exists());
    }
    #[test]
    fn metadata_migration_preserves_conflicting_directories() {
        let fixture = Fixture::new();
        let folder = workspace(&fixture.profile(), "thread-one").unwrap();
        fs::create_dir(folder.join(".pi")).unwrap();
        fs::create_dir(folder.join(".muniment")).unwrap();
        fs::write(folder.join(".pi/old.txt"), "old").unwrap();
        fs::write(folder.join(".muniment/new.txt"), "new").unwrap();
        assert!(workspace(&fixture.profile(), "thread-one").is_err());
        assert_eq!(
            fs::read_to_string(folder.join(".pi/old.txt")).unwrap(),
            "old"
        );
        assert_eq!(
            fs::read_to_string(folder.join(".muniment/new.txt")).unwrap(),
            "new"
        );
    }

    #[test]
    fn rename_preserves_files_and_thread_membership() {
        let fixture = Fixture::new();
        let profile = fixture.profile();
        let id = create(&profile, "Contracts").unwrap();
        assign(&profile, "thread-one", &id).unwrap();
        fs::write(folder(&profile, &id).unwrap().join("draft.txt"), "draft").unwrap();
        rename(&profile, &id, "Agreements").unwrap();
        let renamed = thread_folder(&profile, "thread-one").unwrap().unwrap();
        assert!(renamed
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("Agreements-"));
        assert_eq!(
            fs::read_to_string(renamed.join("draft.txt")).unwrap(),
            "draft"
        );
        assert!(!root(&profile).unwrap().join("Contracts").exists());
        assert_eq!(list(&profile).unwrap().threads["thread-one"], id);
        rename(&profile, &id, "agreements").unwrap();
        assert!(thread_folder(&profile, "thread-one")
            .unwrap()
            .unwrap()
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("agreements-"));
    }
    #[test]
    fn a_missing_project_folder_drops_out_of_the_list_and_keeps_its_place() {
        let fixture = Fixture::new();
        let profile = fixture.profile();
        let kept = create(&profile, "Contracts").unwrap();
        let gone = create(&profile, "Audit").unwrap();
        assign(&profile, "thread-kept", &kept).unwrap();
        assign(&profile, "thread-gone", &gone).unwrap();
        // Listing records each folder's name, so the missing one is a mapped folder.
        list(&profile).unwrap();
        let folder_of_gone = folder(&profile, &gone).unwrap();
        fs::rename(&folder_of_gone, fixture.0.join("away")).unwrap();
        let listed = list(&profile).unwrap();
        assert_eq!(listed.projects.keys().collect::<Vec<_>>(), vec![&kept]);
        assert_eq!(
            listed.threads.keys().collect::<Vec<_>>(),
            vec!["thread-kept"]
        );
        assert!(read(&profile).unwrap().projects.contains_key(&gone));
        fs::rename(fixture.0.join("away"), &folder_of_gone).unwrap();
        assert!(list(&profile).unwrap().projects.contains_key(&gone));
    }
    #[test]
    fn a_deleted_project_leaves_its_threads_and_keeps_a_folder_with_the_users_files() {
        let fixture = Fixture::new();
        let profile = fixture.profile();
        let empty = create(&profile, "Audit").unwrap();
        let mine = create(&profile, "Contracts").unwrap();
        assign(&profile, "thread-audit", &empty).unwrap();
        assign(&profile, "thread-mine", &mine).unwrap();
        save_settings(
            &profile,
            &empty,
            &Settings {
                defaults: Defaults {
                    permission_mode: Some("plan".into()),
                    ..Defaults::default()
                },
                instructions: None,
            },
        )
        .unwrap();
        let empty_folder = folder(&profile, &empty).unwrap();
        fs::create_dir_all(empty_folder.join(".muniment/tasks")).unwrap();
        let mine_folder = folder(&profile, &mine).unwrap();
        fs::write(mine_folder.join("draft.txt"), "draft").unwrap();

        assert_eq!(delete(&profile, &empty).unwrap(), None);
        assert!(!empty_folder.exists());
        let catalog = read(&profile).unwrap();
        assert!(!catalog.projects.contains_key(&empty));
        assert!(!catalog.settings.contains_key(&empty));
        assert!(!catalog.threads.contains_key("thread-audit"));
        assert_eq!(thread_project(&profile, "thread-audit").unwrap(), None);
        assert_eq!(
            thread_project(&profile, "thread-mine").unwrap(),
            Some(mine.clone())
        );

        assert_eq!(delete(&profile, &mine).unwrap(), Some(mine_folder.clone()));
        assert_eq!(
            fs::read_to_string(mine_folder.join("draft.txt")).unwrap(),
            "draft"
        );
        // The kept folder stays out of the list, and no project returns.
        let listed = list(&profile).unwrap();
        assert!(listed.projects.is_empty());
        assert!(listed.threads.is_empty());
        assert!(delete(&profile, &mine).is_err());

        let homelab = create(&profile, "Homelab").unwrap();
        crate::agents::save(
            &profile,
            crate::agents::Agent {
                name: "Deploy bot".into(),
                instructions: "Deploy the stack.".into(),
                project_id: Some(homelab.clone()),
                ..crate::agents::Agent::default()
            },
        )
        .unwrap();
        assert!(delete(&profile, &homelab).unwrap_err().contains("agent"));
        assert!(list(&profile).unwrap().projects.contains_key(&homelab));
    }
    #[test]
    fn settings_keep_defaults_in_the_catalog_and_instructions_in_the_folder() {
        let fixture = Fixture::new();
        let profile = fixture.profile();
        let id = create(&profile, "Homelab").unwrap();
        assert_eq!(
            settings(&profile, &id).unwrap().instructions.as_deref(),
            Some("")
        );
        let saved = Settings {
            defaults: Defaults {
                model: Some(DefaultModel {
                    provider: "openai".into(),
                    model: "gpt-6".into(),
                }),
                permission_mode: Some("plan".into()),
                machine: Some("studio".into()),
            },
            instructions: Some("Answer in short lines.".into()),
        };
        save_settings(&profile, &id, &saved).unwrap();
        assert_eq!(settings(&profile, &id).unwrap(), saved);
        assert_eq!(
            fs::read_to_string(folder(&profile, &id).unwrap().join("AGENTS.md")).unwrap(),
            "Answer in short lines."
        );
        save_settings(
            &profile,
            &id,
            &Settings {
                instructions: Some(" ".into()),
                ..Settings::default()
            },
        )
        .unwrap();
        assert!(!folder(&profile, &id).unwrap().join("AGENTS.md").exists());
        assert!(read(&profile).unwrap().settings.is_empty());

        assert!(
            settings(&profile, RETIRED_CHATS).is_err(),
            "no built-in project keeps settings"
        );
        assert!(settings(&profile, "missing").is_err());
        // The tool list of an older catalog loads and a save drops it.
        let older: Defaults = serde_json::from_value(serde_json::json!({
            "permissionMode": "plan",
            "tools": {"disabledTools": ["bash"], "disabledExtensions": ["mcp-crm"]}
        }))
        .unwrap();
        assert_eq!(
            serde_json::to_value(&older).unwrap(),
            serde_json::json!({"permissionMode": "plan"})
        );
    }
    #[test]
    fn a_thread_moves_between_projects() {
        let fixture = Fixture::new();
        let profile = fixture.profile();
        let homelab = create(&profile, "Homelab").unwrap();
        assert_eq!(thread_project(&profile, "t-1").unwrap(), None);
        move_thread(&profile, "t-1", Some(&homelab)).unwrap();
        assert_eq!(
            thread_project(&profile, "t-1").unwrap(),
            Some(homelab.clone())
        );
        assert_eq!(
            workspace(&profile, "t-1").unwrap(),
            folder(&profile, &homelab).unwrap()
        );
        move_thread(&profile, "t-1", None).unwrap();
        assert_eq!(thread_project(&profile, "t-1").unwrap(), None);
        assert!(move_thread(&profile, "t-1", Some("missing")).is_err());

        let agent = crate::agents::save(
            &profile,
            crate::agents::Agent {
                name: "Deploy bot".into(),
                instructions: "Deploy the stack.".into(),
                project_id: Some(homelab.clone()),
                ..crate::agents::Agent::default()
            },
        )
        .unwrap();
        let mut state = crate::agents::state(&profile).unwrap();
        state.threads.insert("t-agent".into(), agent.id.clone());
        crate::agents::persist_state(&profile, &state).unwrap();
        assert_eq!(thread_project(&profile, "t-agent").unwrap(), Some(homelab));
        assert!(move_thread(&profile, "t-agent", None)
            .unwrap_err()
            .contains("agent"));
    }
    #[test]
    fn threads_of_the_retired_chats_project_belong_to_no_project() {
        let fixture = Fixture::new();
        let profile = fixture.profile();
        let homelab = create(&profile, "Homelab").unwrap();
        let mut catalog = read(&profile).unwrap();
        catalog.threads.insert("t-old".into(), RETIRED_CHATS.into());
        catalog.threads.insert("t-lab".into(), homelab.clone());
        catalog
            .settings
            .insert(RETIRED_CHATS.into(), Defaults::default());
        save(&profile, &catalog).unwrap();
        assert_eq!(thread_project(&profile, "t-old").unwrap(), None);
        assert_eq!(thread_project(&profile, "t-lab").unwrap(), Some(homelab));
        let catalog = read(&profile).unwrap();
        assert!(!catalog.settings.contains_key(RETIRED_CHATS));
        assert!(!catalog.threads.contains_key("t-old"));
    }
    #[test]
    fn discovers_existing_folders_and_rejects_collisions_and_escapes() {
        let fixture = Fixture::new();
        let profile = fixture.profile();
        fs::create_dir(root(&profile).unwrap().join("Research")).unwrap();
        assert!(list(&profile)
            .unwrap()
            .projects
            .values()
            .any(|name| name == "Research"));
        let id = create(&profile, "Reports").unwrap();
        assert!(rename(&profile, &id, "Research").is_err());
        for name in ["../outside", "one/two", "one\\two", ".hidden", "CON", ""] {
            assert!(create(&profile, name).is_err());
        }
        assign(&profile, "thread-two", &id).unwrap();
        fs::remove_dir(folder(&profile, &id).unwrap()).unwrap();
        assert!(thread_folder(&profile, "thread-two").is_err());
        assert_eq!(thread_folder(&profile, "unassigned").unwrap(), None);
    }
}

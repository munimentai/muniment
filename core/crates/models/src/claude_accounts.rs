//! The Anthropic subscription accounts that Claude Code signs in to. Each
//! account has its own Claude Code config folder, which Claude Code fills
//! with its own login, so the account's token stays in Claude Code's store.
//! This record holds what Claude Code reports about each account, its email,
//! organization and plan, and its last usage reading, never a token. It also
//! keeps the account each thread runs on.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

const STORE_FILE: &str = "claude-accounts.json";
/// The folder under the agent directory that holds one config folder per account.
pub const FOLDERS: &str = "claude-accounts";
/// The Pi provider the bridge registers, which runs on these accounts.
pub const PROVIDER: &str = "claude-bridge";

/// One account Claude Code signed in to.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Account {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub org_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub org_name: Option<String>,
    /// The plan Claude Code names, such as `pro` or `max`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan: Option<String>,
    pub added_ms: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<Usage>,
}

impl Account {
    /// The name a row shows: the email, else the organization, else a
    /// placeholder.
    pub fn label(&self) -> String {
        self.email
            .clone()
            .or_else(|| self.org_name.clone())
            .unwrap_or_else(|| "Claude account".to_owned())
    }

    /// Whether Claude Code reports a spent window on this account.
    pub fn limited(&self) -> bool {
        self.usage
            .as_ref()
            .is_some_and(|usage| usage.windows.iter().any(|window| window.limit_reached))
    }

    /// Whether two sign-ins reached the same account of the same organization.
    fn same_as(&self, other: &Account) -> bool {
        self.email.is_some() && self.email == other.email && self.org_id == other.org_id
    }
}

/// The plan limits Claude Code last reported for an account.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Usage {
    pub observed_at_ms: i64,
    /// False when Claude Code reports that plan limits do not apply.
    pub readable: bool,
    #[serde(default)]
    pub windows: Vec<Window>,
}

/// One plan window: `five_hour` or `weekly`, with the model it covers when
/// it covers one.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Window {
    pub kind: String,
    #[serde(default)]
    pub scope: String,
    pub remaining_percent: f64,
    pub limit_reached: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resets_at_ms: Option<i64>,
}

/// Every account, and the account each thread runs on.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Store {
    #[serde(default)]
    pub accounts: Vec<Account>,
    #[serde(default)]
    pub threads: BTreeMap<String, String>,
}

impl Store {
    pub fn account(&self, id: &str) -> Option<&Account> {
        self.accounts.iter().find(|account| account.id == id)
    }

    /// Adds `account`. A sign-in that reached an account the record already
    /// holds replaces it, and its threads move to the new one. Answers the
    /// ids it replaced, whose folders the caller removes.
    pub fn add(&mut self, account: Account) -> Vec<String> {
        let replaced: Vec<String> = self
            .accounts
            .iter()
            .filter(|held| held.id != account.id && held.same_as(&account))
            .map(|held| held.id.clone())
            .collect();
        self.accounts.retain(|held| !replaced.contains(&held.id));
        for thread in self.threads.values_mut() {
            if replaced.contains(thread) {
                *thread = account.id.clone();
            }
        }
        self.accounts.retain(|held| held.id != account.id);
        self.accounts.push(account);
        replaced
    }

    /// Removes one account and the threads that ran on it.
    pub fn remove(&mut self, id: &str) -> bool {
        let before = self.accounts.len();
        self.accounts.retain(|account| account.id != id);
        self.threads.retain(|_, account| account != id);
        self.accounts.len() != before
    }

    /// The account a thread runs on. A thread keeps the account it started
    /// on while that account exists. A new thread takes the account that
    /// serves the fewest threads, preferring one with no spent window, then
    /// the earliest added.
    pub fn pick(&mut self, thread: Option<&str>) -> Option<String> {
        if let Some(held) = thread.and_then(|thread| self.threads.get(thread)) {
            if self.account(held).is_some() {
                return Some(held.clone());
            }
        }
        let load = |id: &str| self.threads.values().filter(|held| *held == id).count();
        let chosen = self
            .accounts
            .iter()
            .min_by_key(|account| (account.limited(), load(&account.id), account.added_ms))?
            .id
            .clone();
        if let Some(thread) = thread {
            self.threads.insert(thread.to_owned(), chosen.clone());
        }
        Some(chosen)
    }
}

/// Serializes the record's read-modify-write within one process.
static WRITE: Mutex<()> = Mutex::new(());

pub fn load(agent: &Path) -> Store {
    fs::read(agent.join(STORE_FILE))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

fn save(agent: &Path, store: &Store) -> io::Result<()> {
    fs::create_dir_all(agent)?;
    let temporary = agent.join(format!("claude-accounts-{}.tmp", uuid::Uuid::new_v4()));
    fs::write(&temporary, serde_json::to_vec_pretty(store)?)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&temporary, fs::Permissions::from_mode(0o600))?;
    }
    muniment_atomic_file::replace(&temporary, &agent.join(STORE_FILE))
}

/// Changes the record under the process lock and answers what `change`
/// answered.
pub fn update<T>(agent: &Path, change: impl FnOnce(&mut Store) -> T) -> io::Result<T> {
    let _held = WRITE.lock().unwrap_or_else(|error| error.into_inner());
    let mut store = load(agent);
    let answer = change(&mut store);
    save(agent, &store)?;
    Ok(answer)
}

/// The Claude Code config folder of one account.
pub fn folder(agent: &Path, id: &str) -> PathBuf {
    agent.join(FOLDERS).join(id)
}

/// The config folder the thread's chat hands Claude Code, and records the
/// pick, or none when no account is signed in.
pub fn folder_for_thread(agent: &Path, thread: Option<&str>) -> Option<PathBuf> {
    if load(agent).accounts.is_empty() {
        return None;
    }
    let id = update(agent, |store| store.pick(thread)).ok().flatten()?;
    Some(folder(agent, &id))
}

/// The folders of the Claude Code binary the Agent SDK installs beside the
/// bridge, for this platform, the glibc build before the musl one.
fn bundled_packages() -> Vec<&'static str> {
    let os = match std::env::consts::OS {
        "macos" => "darwin",
        "windows" => "win32",
        other => other,
    };
    let arch = match std::env::consts::ARCH {
        "x86_64" => "x64",
        "aarch64" => "arm64",
        other => other,
    };
    let base = match (os, arch) {
        ("darwin", "arm64") => "claude-agent-sdk-darwin-arm64",
        ("darwin", "x64") => "claude-agent-sdk-darwin-x64",
        ("linux", "arm64") => "claude-agent-sdk-linux-arm64",
        ("linux", "x64") => "claude-agent-sdk-linux-x64",
        ("win32", "arm64") => "claude-agent-sdk-win32-arm64",
        ("win32", "x64") => "claude-agent-sdk-win32-x64",
        _ => return Vec::new(),
    };
    match os {
        "linux" if arch == "arm64" => vec![base, "claude-agent-sdk-linux-arm64-musl"],
        "linux" => vec![base, "claude-agent-sdk-linux-x64-musl"],
        _ => vec![base],
    }
}

/// Where the Agent SDK puts Claude Code under the agent directory on this
/// platform, in the order to try them.
pub fn bundled_paths(agent: &Path) -> Vec<PathBuf> {
    let name = if cfg!(windows) {
        "claude.exe"
    } else {
        "claude"
    };
    bundled_packages()
        .into_iter()
        .map(|package| {
            agent
                .join("npm")
                .join("node_modules")
                .join("@anthropic-ai")
                .join(package)
                .join(name)
        })
        .collect()
}

/// The bundled Claude Code binaries that exist, in the order to try them.
pub fn bundled_executables(agent: &Path) -> Vec<PathBuf> {
    bundled_paths(agent)
        .into_iter()
        .filter(|path| path.is_file())
        .collect()
}

/// Whether a `claude --version` line names the pinned Claude Code version.
pub fn pinned_version(line: &str) -> bool {
    line.split_whitespace().next() == Some(muniment_pins::CLAUDE_CODE.version)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn account(id: &str, email: &str, added_ms: i64) -> Account {
        Account {
            id: id.into(),
            email: Some(email.into()),
            org_id: Some("org".into()),
            added_ms,
            ..Account::default()
        }
    }

    #[test]
    fn a_new_thread_takes_the_least_used_account_and_keeps_it() {
        let mut store = Store::default();
        assert_eq!(store.pick(Some("t0")), None);
        store.add(account("a", "a@example.test", 1));
        store.add(account("b", "b@example.test", 2));
        assert_eq!(store.pick(Some("t1")).as_deref(), Some("a"));
        assert_eq!(store.pick(Some("t2")).as_deref(), Some("b"));
        assert_eq!(store.pick(Some("t1")).as_deref(), Some("a"));
        assert_eq!(store.pick(None).as_deref(), Some("a"));
        assert_eq!(store.threads.len(), 2);
    }

    #[test]
    fn a_spent_account_takes_new_threads_last() {
        let mut store = Store::default();
        let mut spent = account("a", "a@example.test", 1);
        spent.usage = Some(Usage {
            observed_at_ms: 1,
            readable: true,
            windows: vec![Window {
                kind: "five_hour".into(),
                limit_reached: true,
                ..Window::default()
            }],
        });
        store.add(spent);
        store.add(account("b", "b@example.test", 2));
        store.threads.insert("busy".into(), "b".into());
        assert_eq!(store.pick(Some("t1")).as_deref(), Some("b"));
    }

    #[test]
    fn signing_in_to_the_same_account_replaces_it_and_moves_its_threads() {
        let mut store = Store::default();
        store.add(account("old", "a@example.test", 1));
        store.add(account("other", "b@example.test", 2));
        store.threads.insert("t1".into(), "old".into());
        let replaced = store.add(account("new", "a@example.test", 3));
        assert_eq!(replaced, vec!["old".to_owned()]);
        assert_eq!(store.threads["t1"], "new");
        assert_eq!(store.accounts.len(), 2);
        let mut work = account("work", "a@example.test", 4);
        work.org_id = Some("work-org".into());
        assert!(store.add(work).is_empty());
        assert_eq!(store.accounts.len(), 3);
    }

    #[test]
    fn removing_an_account_frees_its_threads_and_the_record_round_trips() {
        let agent = std::env::temp_dir().join(format!("claude-accounts-{}", uuid::Uuid::new_v4()));
        update(&agent, |store| {
            store.add(account("a", "a@example.test", 1));
        })
        .unwrap();
        assert_eq!(
            folder_for_thread(&agent, Some("t1")),
            Some(folder(&agent, "a"))
        );
        assert_eq!(load(&agent).threads["t1"], "a");
        assert!(update(&agent, |store| store.remove("a")).unwrap());
        let store = load(&agent);
        assert!(store.accounts.is_empty() && store.threads.is_empty());
        assert_eq!(folder_for_thread(&agent, Some("t1")), None);
        let _ = fs::remove_dir_all(&agent);
    }

    #[test]
    fn the_bundled_binary_is_found_under_the_agent_npm_folder_only() {
        let agent = std::env::temp_dir().join(format!("claude-bundled-{}", uuid::Uuid::new_v4()));
        assert!(bundled_executables(&agent).is_empty());
        if let Some(path) = bundled_paths(&agent).first() {
            assert!(path.starts_with(agent.join("npm/node_modules/@anthropic-ai")));
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, "").unwrap();
            assert_eq!(bundled_executables(&agent), vec![path.clone()]);
        }
        let _ = fs::remove_dir_all(&agent);
    }

    #[test]
    fn the_version_line_matches_the_pin() {
        let pinned = format!("{} (Claude Code)", muniment_pins::CLAUDE_CODE.version);
        assert!(pinned_version(&pinned));
        assert!(!pinned_version("0.0.1 (Claude Code)"));
        assert!(!pinned_version(""));
    }
}

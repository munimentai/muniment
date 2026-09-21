use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use serde::Serialize;
use std::{
    collections::{HashMap, VecDeque},
    io::{Read, Write},
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
};
use tauri::{Manager, State};

struct Session {
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    child: Box<dyn Child + Send + Sync>,
    output: Arc<Mutex<VecDeque<u8>>>,
    ended: Arc<AtomicBool>,
}
impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
#[derive(Default)]
pub struct TerminalState(Mutex<HashMap<String, Session>>);
fn size(cols: u16, rows: u16) -> PtySize {
    PtySize {
        cols: cols.clamp(2, 500),
        rows: rows.clamp(2, 300),
        pixel_width: 0,
        pixel_height: 0,
    }
}
#[tauri::command]
pub async fn terminal_start(
    app: tauri::AppHandle,
    webview: tauri::Webview,
    path: PathBuf,
    cols: u16,
    rows: u16,
) -> Result<String, String> {
    crate::workspace_tools::shell(&webview)?;
    tauri::async_runtime::spawn_blocking(move || {
        let path = crate::workspace_tools::directory(&path)?;
        let state = app.state::<TerminalState>();
        let mut held = state.0.lock().map_err(|e| e.to_string())?;
        let pair = native_pty_system()
            .openpty(size(cols, rows))
            .map_err(|e| e.to_string())?;
        let mut command = CommandBuilder::new_default_prog();
        #[cfg(unix)]
        {
            let shell = command.get_shell();
            let shell_name = std::path::Path::new(&shell).file_name().and_then(|s| s.to_str());
            let config = app.path().app_cache_dir().map_err(|e| e.to_string())?.join("terminal-shell");
            std::fs::create_dir_all(&config).map_err(|e| e.to_string())?;
            if shell_name == Some("zsh") {
                let original = std::env::var_os("ZDOTDIR").or_else(|| std::env::var_os("HOME")).unwrap_or_default();
                for file in [".zshenv", ".zprofile", ".zshrc", ".zlogin"] {
                    let mut script = format!("[[ -f \"$MUNIMENT_USER_ZDOTDIR/{file}\" ]] && source \"$MUNIMENT_USER_ZDOTDIR/{file}\"\n");
                    if file == ".zshrc" {
                        script.push_str("_muniment_prompt() { PROMPT='$ '; RPROMPT=''; }\nprecmd_functions+=(_muniment_prompt)\n_muniment_prompt\n");
                    }
                    std::fs::write(config.join(file), script).map_err(|e| e.to_string())?;
                }
                command = CommandBuilder::new(shell);
                command.arg("-l");
                command.env("MUNIMENT_USER_ZDOTDIR", original);
                command.env("ZDOTDIR", &config);
            } else if shell_name == Some("bash") {
                let rc = config.join("bashrc");
                std::fs::write(&rc, "[[ -f ~/.bashrc ]] && source ~/.bashrc\nPROMPT_COMMAND+=(\"PS1='$ '\")\nPS1='$ '\n").map_err(|e| e.to_string())?;
                command = CommandBuilder::new(shell);
                command.arg("--rcfile");
                command.arg(rc);
            }
        }
        command.cwd(path);
        command.env("TERM", "xterm-256color");
        // The header shows the folder. Keep the shell prompt short and local to
        // this process, without changing the user's shell configuration.
        command.env("PS1", "$ ");
        #[cfg(windows)]
        command.env("PROMPT", "$G ");
        #[cfg(not(windows))]
        command.env("PROMPT", "$ ");
        let mut reader = pair.master.try_clone_reader().map_err(|e| e.to_string())?;
        let writer = pair.master.take_writer().map_err(|e| e.to_string())?;
        let child = pair
            .slave
            .spawn_command(command)
            .map_err(|e| e.to_string())?;
        drop(pair.slave);
        let output = Arc::new(Mutex::new(VecDeque::new()));
        let target = output.clone();
        let ended = Arc::new(AtomicBool::new(false));
        let reader_ended = ended.clone();
        std::thread::spawn(move || {
            let mut buffer = [0u8; 8192];
            while let Ok(n) = reader.read(&mut buffer) {
                if n == 0 {
                    break;
                }
                let mut data = target.lock().unwrap();
                data.extend(&buffer[..n]);
                while data.len() > 1024 * 1024 {
                    data.pop_front();
                }
            }
            reader_ended.store(true, Ordering::Release);
        });
        let id = uuid::Uuid::new_v4().to_string();
        held.insert(id.clone(), Session {
            master: pair.master,
            writer,
            child,
            output,
            ended,
        });
        Ok(id)
    })
    .await
    .map_err(|e| e.to_string())?
}
#[derive(Serialize)]
pub struct Output {
    bytes: Vec<u8>,
    exited: bool,
}
#[tauri::command]
pub fn terminal_read(
    webview: tauri::Webview,
    state: State<TerminalState>,
    id: String,
) -> Result<Output, String> {
    crate::workspace_tools::shell(&webview)?;
    let mut held = state.0.lock().map_err(|e| e.to_string())?;
    let session = held.get_mut(&id).ok_or("The terminal is closed.")?;
    let bytes = session
        .output
        .lock()
        .map_err(|e| e.to_string())?
        .drain(..)
        .collect();
    Ok(Output {
        bytes,
        exited: session
            .child
            .try_wait()
            .map_err(|e| e.to_string())?
            .is_some()
            && session.ended.load(Ordering::Acquire),
    })
}
#[tauri::command]
pub fn terminal_write(
    webview: tauri::Webview,
    state: State<TerminalState>,
    id: String,
    data: String,
) -> Result<(), String> {
    crate::workspace_tools::shell(&webview)?;
    if data.len() > 65536 {
        return Err("Terminal input is too large.".into());
    }
    let mut held = state.0.lock().map_err(|e| e.to_string())?;
    let session = held.get_mut(&id).ok_or("The terminal is closed.")?;
    session
        .writer
        .write_all(data.as_bytes())
        .and_then(|_| session.writer.flush())
        .map_err(|e| e.to_string())
}
#[tauri::command]
pub fn terminal_resize(
    webview: tauri::Webview,
    state: State<TerminalState>,
    id: String,
    cols: u16,
    rows: u16,
) -> Result<(), String> {
    crate::workspace_tools::shell(&webview)?;
    let held = state.0.lock().map_err(|e| e.to_string())?;
    held.get(&id)
        .ok_or("The terminal is closed.")?
        .master
        .resize(size(cols, rows))
        .map_err(|e| e.to_string())
}
#[tauri::command]
pub fn terminal_close(
    webview: tauri::Webview,
    state: State<TerminalState>,
    id: String,
) -> Result<(), String> {
    crate::workspace_tools::shell(&webview)?;
    let session = {
        let mut held = state.0.lock().map_err(|e| e.to_string())?;
        held.remove(&id)
    };
    drop(session);
    Ok(())
}

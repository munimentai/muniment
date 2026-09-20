use crate::cef_native;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicI32, Ordering};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{mpsc, Arc, Mutex},
    time::Duration,
};
use tauri::{Emitter, Manager};
static NEXT_ID: AtomicI32 = AtomicI32::new(1);

type Reply = mpsc::Sender<Result<Value, String>>;
#[derive(Default)]
struct Control {
    pending: Mutex<HashMap<i32, Reply>>,
    grants: Mutex<HashMap<String, String>>,
    serial: Mutex<()>,
    active: Mutex<String>,
}
#[derive(Deserialize, Serialize, Clone)]
pub struct Request {
    view: String,
    action: String,
    #[serde(default)]
    value: String,
    #[serde(default)]
    selector: String,
}
fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}
fn allowed_url(value: &str) -> Result<tauri::Url, String> {
    let url = tauri::Url::parse(value).map_err(err)?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err("Use an HTTP or HTTPS address without credentials.".into());
    }
    Ok(url)
}
fn origin(value: &str) -> Result<String, String> {
    Ok(allowed_url(value)?.origin().ascii_serialization())
}
fn target(app: &tauri::AppHandle, label: &str) -> Result<cef_native::View, String> {
    cef_native::view(app, label)
}
fn cdp(app: &tauri::AppHandle, view: &str, method: &str, params: Value) -> Result<Value, String> {
    let webview = target(app, view)?;
    let control = app.state::<Arc<Control>>();
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let (tx, rx) = mpsc::channel();
    control.pending.lock().unwrap().insert(id, tx);
    let result = webview.send_dev_tools_message(
        json!({"id":id,"method":method,"params":params})
            .to_string()
            .as_bytes(),
    );
    let reply = match result {
        Ok(()) => rx
            .recv_timeout(Duration::from_secs(10))
            .map_err(err)
            .and_then(|r| r),
        Err(e) => Err(err(e)),
    };
    control.pending.lock().unwrap().remove(&id);
    reply
}
fn evaluate(app: &tauri::AppHandle, view: &str, expression: String) -> Result<Value, String> {
    let result = cdp(
        app,
        view,
        "Runtime.evaluate",
        json!({"expression":expression,"returnByValue":true,"awaitPromise":true}),
    )?;
    if result.get("exceptionDetails").is_some() {
        return Err("The page could not complete this action.".into());
    }
    Ok(result["result"]["value"].clone())
}
fn operate(app: &tauri::AppHandle, req: Request, agent: bool) -> Result<Value, String> {
    let control = app.state::<Arc<Control>>();
    if req.action == "stop" {
        control.grants.lock().unwrap().clear();
        return Ok(json!({"stopped":true}));
    }
    let _serial = control.serial.lock().unwrap();
    let webview = target(app, &req.view)?;
    let url = webview.url().map_err(err)?.to_string();
    if agent {
        if *control.active.lock().unwrap() != req.view {
            return Err("Select this view before allowing agent control.".into());
        }
        let grant = control.grants.lock().unwrap().get(&req.view).cloned();
        if grant.as_deref() != Some(origin(&url)?.as_str()) {
            return Err("Allow agent control for this page first.".into());
        }
    }
    match req.action.as_str() {
        "grant" if !agent => {
            let site = origin(&url)?;
            control.grants.lock().unwrap().insert(req.view, site.clone());
            Ok(json!({"origin":site}))
        }
        "navigate" => {
            let next = allowed_url(&req.value)?;
            if req.view == "artifact" && next.origin().ascii_serialization() != app.state::<BrowserStorage>().origin { return Err("Artifact previews stay local.".into()); }
            if agent && origin(next.as_str())? != origin(&url)? { return Err("Open the new site and allow agent control there.".into()); }
            webview.navigate(next).map_err(err)?;
            Ok(json!({"navigating":true}))
        }
        "snapshot" => evaluate(app, &req.view, "JSON.stringify({title:document.title,url:location.href,text:document.body.innerText.slice(0,12000),controls:[...document.querySelectorAll('button,input,select,a')].slice(0,80).map(e=>({tag:e.tagName,id:e.id,text:(e.innerText||e.getAttribute('aria-label')||'').slice(0,120),type:e.type}))})".into()),
        "click" | "type" => {
            if req.selector.is_empty() || req.selector.len()>512 || req.value.len()>4096 { return Err("Invalid element or text.".into()); }
            let selector = serde_json::to_string(&req.selector).unwrap();
            let check = format!("(()=>{{const e=document.querySelector({selector});if(!e)throw Error('Element not found');if(e.type==='password')throw Error('Enter passwords yourself');e.scrollIntoView({{block:'center'}});e.focus();const r=e.getBoundingClientRect();return {{x:r.x+r.width/2,y:r.y+r.height/2}}}})()");
            let point = evaluate(app,&req.view,check)?;
            // Recheck consent after page inspection, before an input event.
            if agent && !control.grants.lock().unwrap().contains_key(&req.view) { return Err("Agent control stopped.".into()); }
            if req.action == "type" {
                cdp(app,&req.view,"Input.insertText",json!({"text":req.value}))
            } else {
                let p=json!({"type":"mousePressed","x":point["x"],"y":point["y"],"button":"left","clickCount":1});
                cdp(app,&req.view,"Input.dispatchMouseEvent",p)?;
                cdp(app,&req.view,"Input.dispatchMouseEvent",json!({"type":"mouseReleased","x":point["x"],"y":point["y"],"button":"left","clickCount":1}))
            }
        }
        "screenshot" => cdp(app,&req.view,"Page.captureScreenshot",json!({"format":"png"})),
        "back" | "forward" if !agent => {
            let history=cdp(app,&req.view,"Page.getNavigationHistory",json!({}))?;
            let index=history["currentIndex"].as_i64().ok_or("No navigation history.")? + if req.action=="back" {-1} else {1};
            let entry=history["entries"].as_array().and_then(|entries|usize::try_from(index).ok().and_then(|i|entries.get(i))).ok_or("No page in that direction.")?;
            cdp(app,&req.view,"Page.navigateToHistoryEntry",json!({"entryId":entry["id"]}))
        }
        "reload" if !agent => cdp(app,&req.view,"Page.reload",json!({})),
        "status" if !agent => Ok(json!({"url":url,"allowed":control.grants.lock().unwrap().contains_key(&req.view),"chromium":"152.0.6"})),
        _ => Err("Unsupported browser action.".into()),
    }
}
#[tauri::command]
pub async fn browser_command(
    app: tauri::AppHandle,
    webview: tauri::Webview,
    request: Request,
) -> Result<Value, String> {
    if webview.label() != "main" {
        return Err("Only the control panel can call this command.".into());
    }
    tauri::async_runtime::spawn_blocking(move || operate(&app, request, false))
        .await
        .map_err(err)?
}
#[cfg(unix)]
fn serve_agent(app: tauri::AppHandle, root: &std::path::Path) -> Result<(), String> {
    use std::{
        io::{BufRead, BufReader, Read, Write},
        os::unix::{fs::PermissionsExt, net::UnixListener},
    };
    let socket = root.join("agent.sock");
    if socket.exists() {
        std::fs::remove_file(&socket).map_err(err)?;
    }
    let listener = UnixListener::bind(&socket).map_err(err)?;
    std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o600)).map_err(err)?;
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let app = app.clone();
            std::thread::spawn(move || {
                let mut stream = stream;
                let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
                let _ = stream.set_write_timeout(Some(Duration::from_secs(10)));
                let mut line = String::new();
                let read = BufReader::new((&mut stream).take(16385)).read_line(&mut line);
                let result = if read.is_err() || line.len() > 16384 || !line.ends_with('\n') {
                    Err("Invalid request size.".into())
                } else {
                    serde_json::from_str::<Request>(&line)
                        .map_err(err)
                        .and_then(|req| {
                            let action = req.action.clone();
                            let view = req.view.clone();
                            let result = operate(&app, req, true);
                            let _ = app.emit_to(
                                "main",
                                "agent-action",
                                json!({"action":action,"view":view,"ok":result.is_ok()}),
                            );
                            result
                        })
                };
                let reply = match result {
                    Ok(value) => json!({"ok":true,"result":value}),
                    Err(error) => json!({"ok":false,"error":error}),
                };
                let _ = writeln!(stream, "{reply}");
            });
        }
    });
    Ok(())
}

struct BrowserStorage {
    root: PathBuf,
    origin: String,
    token: String,
}
#[derive(Serialize, Deserialize)]
pub struct Artifact {
    id: String,
    name: String,
    html: String,
}
fn shell(webview: &tauri::Webview) -> Result<(), String> {
    if webview.label() != "main" {
        return Err("Only the app can change browser settings.".into());
    }
    Ok(())
}
fn artifact_path(root: &std::path::Path, id: &str) -> Result<PathBuf, String> {
    uuid::Uuid::parse_str(id).map_err(|_| "Invalid artifact.".to_string())?;
    Ok(root.join("artifacts").join(format!("{id}.json")))
}
#[tauri::command]
pub fn artifact_list(app: tauri::AppHandle, webview: tauri::Webview) -> Result<Vec<Value>, String> {
    shell(&webview)?;
    let storage = app.state::<BrowserStorage>();
    let mut items = Vec::new();
    for entry in std::fs::read_dir(storage.root.join("artifacts")).map_err(err)? {
        let path = entry.map_err(err)?.path();
        if path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        let artifact: Artifact =
            serde_json::from_slice(&std::fs::read(path).map_err(err)?).map_err(err)?;
        items.push(json!({"id":artifact.id,"name":artifact.name}));
    }
    Ok(items)
}
#[tauri::command]
pub fn artifact_read(
    app: tauri::AppHandle,
    webview: tauri::Webview,
    id: String,
) -> Result<Artifact, String> {
    shell(&webview)?;
    let path = artifact_path(&app.state::<BrowserStorage>().root, &id)?;
    serde_json::from_slice(&std::fs::read(path).map_err(err)?).map_err(err)
}
#[tauri::command]
pub fn artifact_save(
    app: tauri::AppHandle,
    webview: tauri::Webview,
    id: Option<String>,
    name: String,
    html: String,
) -> Result<Artifact, String> {
    shell(&webview)?;
    if name.trim().is_empty() || name.len() > 120 || html.len() > 2_000_000 {
        return Err("Use a name under 120 characters and HTML under 2 MB.".into());
    }
    let item = Artifact {
        id: id.unwrap_or_else(|| uuid::Uuid::now_v7().to_string()),
        name: name.trim().into(),
        html,
    };
    let path = artifact_path(&app.state::<BrowserStorage>().root, &item.id)?;
    let temp = path.with_extension("tmp");
    std::fs::write(&temp, serde_json::to_vec(&item).map_err(err)?).map_err(err)?;
    std::fs::rename(temp, path).map_err(err)?;
    Ok(item)
}
#[derive(Clone, Deserialize)]
pub struct Bounds {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}
#[tauri::command]
pub async fn browser_view(
    app: tauri::AppHandle,
    webview: tauri::Webview,
    label: Option<String>,
    bounds: Option<Bounds>,
    artifact_id: Option<String>,
) -> Result<(), String> {
    shell(&webview)?;
    tauri::async_runtime::spawn_blocking(move || {
        let control = app.state::<Arc<Control>>();
        let _serial = control.serial.lock().unwrap();
        let label = label.unwrap_or_default();
        if !label.is_empty() && label != "browser" && label != "artifact" {
            return Err("Unknown browser view.".into());
        }
        if *control.active.lock().unwrap() != label {
            control.grants.lock().unwrap().clear();
        }
        *control.active.lock().unwrap() = label.clone();
        cef_native::hide_all(&app)?;
        if label.is_empty() {
            return Ok(());
        }
        let b = bounds.ok_or("The browser needs a visible panel.")?;
        if [b.x, b.y, b.width, b.height]
            .iter()
            .any(|v| !v.is_finite() || *v < 0.0)
            || b.width < 1.0
            || b.height < 1.0
        {
            return Err("Invalid browser panel size.".into());
        }
        let storage = app.state::<BrowserStorage>();
        let pending = control.inner().clone();
        let grants = control.inner().clone();
        let events = app.clone();
        let name = label.clone();
        let origin = storage.origin.clone();
        let url = if label == "artifact" {
            artifact_id
                .as_deref()
                .map(|id| {
                    artifact_path(&storage.root, id)
                        .map(|_| format!("{}/{}/{}", storage.origin, storage.token, id))
                })
                .transpose()?
        } else {
            None
        };
        cef_native::layout(
            &app,
            &label,
            &storage.root,
            b,
            url,
            move |id, success, result| {
                if let Some(tx) = pending.pending.lock().unwrap().remove(&id) {
                    let _ = tx.send(if success {
                        serde_json::from_slice(&result).map_err(err)
                    } else {
                        Err("Browser action failed.".into())
                    });
                }
            },
            move |url| {
                grants.grants.lock().unwrap().remove(&name);
                let _ = events.emit_to(
                    "main",
                    "browser-page-change",
                    json!({"view":name,"url":url}),
                );
                allowed_url(&url).is_ok()
                    && (name != "artifact"
                        || crate::cef_browser::origin(&url).ok().as_deref() == Some(&origin))
            },
        )?;
        Ok(())
    })
    .await
    .map_err(err)?
}
pub fn prepare() -> PathBuf {
    let root = muniment_runtime::adopt_state_directory()
        .expect("Open app data")
        .join("browser");
    std::fs::create_dir_all(root.join("artifacts")).expect("Create browser data");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    #[cfg(target_os = "macos")]
    {
        unsafe extern "C" {
            fn muniment_keychain_prepare(audit: *const std::ffi::c_char, allow_create: bool)
                -> i32;
        }
        let audit = std::ffi::CString::new(
            root.join("keychain-audit.log")
                .as_os_str()
                .as_encoded_bytes(),
        )
        .unwrap();
        let status =
            unsafe { muniment_keychain_prepare(audit.as_ptr(), !root.join("profiles").exists()) };
        if status != 0 {
            eprintln!("Saved browser sessions are unavailable. Keychain status: {status}. No profile was opened.");
            std::process::exit(78);
        }
    }
    root
}
pub fn setup(app: &mut tauri::App, root: &std::path::Path) -> Result<(), String> {
    cef_native::initialize(Some(app.handle()), root)?;
    let server = tiny_http::Server::http("127.0.0.1:0").map_err(err)?;
    let origin = format!("http://{}", server.server_addr());
    let token = uuid::Uuid::now_v7().to_string();
    let prefix = format!("/{token}/");
    let files = root.to_path_buf();
    std::thread::spawn(move || {
        for request in server.incoming_requests() {
            let body = request
                .url()
                .strip_prefix(&prefix)
                .and_then(|id| artifact_path(&files, id).ok())
                .and_then(|path| std::fs::read(path).ok())
                .and_then(|bytes| serde_json::from_slice::<Artifact>(&bytes).ok())
                .map(|a| a.html);
            let status = if body.is_some() { 200 } else { 404 };
            let mut response =
                tiny_http::Response::from_string(body.unwrap_or_default()).with_status_code(status);
            for (key,value) in [("Content-Type","text/html; charset=utf-8"),("Cache-Control","no-store"),("Referrer-Policy","no-referrer"),("Content-Security-Policy","default-src 'none'; script-src 'unsafe-inline'; style-src 'unsafe-inline'; img-src data:; connect-src 'none'; frame-src 'none'; form-action 'none'; sandbox allow-scripts")] {
                response.add_header(tiny_http::Header::from_bytes(key,value).unwrap());
            }
            let _ = request.respond(response);
        }
    });
    app.manage(BrowserStorage {
        root: root.into(),
        origin,
        token,
    });
    app.manage(Arc::new(Control::default()));
    #[cfg(unix)]
    serve_agent(app.handle().clone(), root)?;
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn navigation_rejects_privileged_schemes_and_credentials() {
        for url in [
            "file:///etc/passwd",
            "javascript:alert(1)",
            "tauri://localhost",
            "https://user:pass@example.org",
            "data:text/html,hello",
        ] {
            assert!(allowed_url(url).is_err(), "{url}");
        }
        assert!(allowed_url("https://example.org/path").is_ok());
    }
    #[test]
    fn grants_use_exact_origins() {
        assert_eq!(
            origin("https://example.org/a").unwrap(),
            origin("https://example.org:443/b").unwrap()
        );
        assert_ne!(
            origin("https://example.org").unwrap(),
            origin("http://example.org").unwrap()
        );
        assert_ne!(
            origin("https://example.org").unwrap(),
            origin("https://example.org.evil.test").unwrap()
        );
    }
}

#[cfg(feature = "cef-smoke")]
#[path = "cef_smoke.rs"]
mod smoke;
#[cfg(feature = "cef-smoke")]
pub fn start_smoke(app: tauri::AppHandle) {
    smoke::start(app);
}

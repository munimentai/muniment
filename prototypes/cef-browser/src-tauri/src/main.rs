use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex, mpsc},
    time::Duration,
};
use tauri::webview::{NewWindowResponse, PermissionResponse, WebviewBuilder};
use tauri::{Emitter, LogicalPosition, LogicalSize, Manager, WebviewUrl};
use tauri_runtime_cef::{
    Cef, DevToolsProtocol, SandboxPolicy, SecretStorage, WebviewCefExt,
    allocate_devtools_message_id,
};

const FIXTURE: &str = "http://127.0.0.1:48763";
type Reply = mpsc::Sender<Result<Value, String>>;
#[derive(Default)]
struct Control {
    pending: Mutex<HashMap<i32, Reply>>,
    grants: Mutex<HashMap<String, String>>,
    serial: Mutex<()>,
    active: Mutex<String>,
}
#[derive(Deserialize, Serialize, Clone)]
struct Request {
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
fn target(app: &tauri::AppHandle, label: &str) -> Result<tauri::Webview, String> {
    if !matches!(label, "browser" | "artifact") {
        return Err("Unknown browser view.".into());
    }
    app.get_webview(label)
        .ok_or("Browser view is closed.".into())
}
fn cdp(app: &tauri::AppHandle, view: &str, method: &str, params: Value) -> Result<Value, String> {
    let webview = target(app, view)?;
    let control = app.state::<Arc<Control>>();
    let id = allocate_devtools_message_id().map_err(err)?;
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
            if req.view == "artifact" && next.origin().ascii_serialization() != FIXTURE { return Err("Artifact previews stay local.".into()); }
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
        "status" if !agent => Ok(json!({"url":url,"allowed":control.grants.lock().unwrap().contains_key(&req.view),"chromium":app.webview_version().map_err(err)?})),
        _ => Err("Unsupported browser action.".into()),
    }
}
#[tauri::command]
async fn browser_command(
    app: tauri::AppHandle,
    webview: tauri::Webview,
    request: Request,
) -> Result<Value, String> {
    if webview.label() != "shell" {
        return Err("Only the control panel can call this command.".into());
    }
    tauri::async_runtime::spawn_blocking(move || operate(&app, request, false))
        .await
        .map_err(err)?
}
#[tauri::command]
async fn select_view(
    app: tauri::AppHandle,
    webview: tauri::Webview,
    label: String,
    width: f64,
    height: f64,
) -> Result<(), String> {
    if webview.label() != "shell" {
        return Err("Only the control panel can change views.".into());
    }
    tauri::async_runtime::spawn_blocking(move || {
        let control = app.state::<Arc<Control>>();
        let _serial = control.serial.lock().unwrap();
        target(&app, &label)?;
        {
            let mut active = control.active.lock().unwrap();
            if *active != label {
                control.grants.lock().unwrap().clear();
                *active = label.clone();
            }
        }
        for name in ["browser", "artifact"] {
            let view = target(&app, name)?;
            if name == label {
                view.set_position(LogicalPosition::new(320., 100.))
                    .map_err(err)?;
                view.set_size(LogicalSize::new(
                    (width - 320.).max(100.),
                    (height - 100.).max(100.),
                ))
                .map_err(err)?;
                view.show().map_err(err)?;
            } else {
                view.hide().map_err(err)?;
            }
        }
        Ok(())
    })
    .await
    .map_err(err)?
}
fn serve_fixture() -> Result<(), String> {
    let server = tiny_http::Server::http("127.0.0.1:48763").map_err(err)?;
    std::thread::spawn(move || {
        for request in server.incoming_requests() {
            let path = request.url().split('?').next().unwrap_or("/");
            let body = match path {
                "/artifact" => include_str!("../../ui/artifact.html"),
                "/popup" => include_str!("../../ui/popup.html"),
                _ => include_str!("../../ui/fixture.html"),
            };
            let response = tiny_http::Response::from_string(body)
                .with_header(
                    tiny_http::Header::from_bytes("Content-Type", "text/html; charset=utf-8")
                        .unwrap(),
                )
                .with_header(tiny_http::Header::from_bytes("Cache-Control", "no-store").unwrap());
            let _ = request.respond(response);
        }
    });
    Ok(())
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
                                "shell",
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
fn root() -> PathBuf {
    dirs::data_local_dir()
        .expect("Application data directory")
        .join("com.muniment.cef-prototype")
}
#[tauri_runtime_cef::cef_entry_point]
fn main() {
    let root = root();
    std::fs::create_dir_all(&root).expect("Create prototype profile");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    // A dedicated encryption key must never open the old shared-key profile.
    // Keep that profile intact. Production migration needs a separate design.
    #[cfg(target_os = "macos")]
    let cache = root.join("profiles-private-key-v2");
    #[cfg(not(target_os = "macos"))]
    let cache = root.join("profiles");
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
        .expect("Audit path");
        let status = unsafe { muniment_keychain_prepare(audit.as_ptr(), !cache.exists()) };
        if status != 0 {
            eprintln!(
                "Saved browser sessions are unavailable. Keychain status: {status}. No profile was opened."
            );
            std::process::exit(78);
        }
    }
    tauri::Builder::default()
        .runtime(
            Cef::default()
                .sandbox(SandboxPolicy::Required)
                .secret_storage(SecretStorage::System)
                .root_cache_path(&cache)
                .persist_session_cookies(true)
                // Custom browser/artifact profiles need their own restore policy.
                // CEF's global persistence setting only covers its global profile.
                .profile_preference_value("session.restore_on_startup", 1),
        )
        .manage(Arc::new(Control::default()))
        .invoke_handler(tauri::generate_handler![browser_command, select_view])
        .setup(move |app| {
            serve_fixture()?;
            let window = tauri::window::WindowBuilder::new(app, "main")
                .title("Muniment Browser Prototype")
                .inner_size(1240., 820.)
                .min_inner_size(900., 600.)
                .build()?;
            window.add_child(
                WebviewBuilder::new("shell", WebviewUrl::App("index.html".into()))
                    .auto_resize()
                    .on_navigation(|url| {
                        url.scheme() == "tauri" || url.host_str() == Some("tauri.localhost")
                    }),
                LogicalPosition::new(0., 0.),
                LogicalSize::new(1240., 820.),
            )?;
            for (label, path) in [("browser", "/"), ("artifact", "/artifact")] {
                let control = app.state::<Arc<Control>>().inner().clone();
                let state = control.clone();
                let app_handle = app.handle().clone();
                let builder = WebviewBuilder::new(
                    label,
                    WebviewUrl::External(format!("{FIXTURE}{path}").parse()?),
                )
                .data_directory(cache.join(label))
                .on_navigation(move |url| {
                    state.grants.lock().unwrap().remove(label);
                    let _ = app_handle.emit_to(
                        "shell",
                        "page-change",
                        json!({"view":label,"url":url.as_str()}),
                    );
                    matches!(url.scheme(), "http" | "https")
                        && (label != "artifact" || url.origin().ascii_serialization() == FIXTURE)
                })
                .on_permission_request(|_, _| PermissionResponse::Deny)
                .on_new_window(move |url, _| {
                    if label == "browser" && matches!(url.scheme(), "http" | "https") {
                        NewWindowResponse::Allow
                    } else {
                        NewWindowResponse::Deny
                    }
                });
                let view = window.add_child(
                    builder,
                    LogicalPosition::new(320., 100.),
                    LogicalSize::new(920., 720.),
                )?;
                view.on_dev_tools_protocol(move |protocol| {
                    if let DevToolsProtocol::MethodResult {
                        message_id,
                        success,
                        result,
                    } = protocol
                    {
                        if let Some(tx) = control.pending.lock().unwrap().remove(&message_id) {
                            let value = serde_json::from_slice(&result).map_err(err);
                            let _ = tx.send(if success {
                                value
                            } else {
                                Err(format!(
                                    "Browser command failed: {}",
                                    String::from_utf8_lossy(&result)
                                ))
                            });
                        }
                    }
                })?;
                if label == "artifact" {
                    view.hide()?;
                }
            }
            #[cfg(unix)]
            serve_agent(app.handle().clone(), &root)?;
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("Start CEF prototype");
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

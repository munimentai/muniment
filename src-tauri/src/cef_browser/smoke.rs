use super::*;
use std::time::Instant;

fn request(view: &str, action: &str, value: &str, selector: &str) -> Request {
    Request {
        view: view.into(),
        action: action.into(),
        value: value.into(),
        selector: selector.into(),
    }
}
fn check(app: &tauri::AppHandle, phase: &str) -> Result<(), String> {
    let storage = app.state::<BrowserStorage>();
    let address_file = storage.root.join("smoke-address");
    let bind = if phase == "read" {
        std::fs::read_to_string(&address_file).map_err(err)?
    } else {
        "127.0.0.1:0".into()
    };
    let server = tiny_http::Server::http(&bind).map_err(err)?;
    std::fs::write(&address_file, server.server_addr().to_string()).map_err(err)?;
    let site = format!("http://{}", server.server_addr());
    std::thread::spawn(move || {
        for req in server.incoming_requests() {
            let authenticated = req.headers().iter().any(|h| {
                h.field.equiv("Cookie") && h.value.as_str().contains("muniment-cef-test=1")
            });
            let html = if req.url() == "/login" {
                "<!doctype html><title>Login complete</title><a id='protected' href='/secure'>Continue</a>"
            } else if req.url() == "/secure" && authenticated {
                "<!doctype html><title>Protected page</title><button id='counter' onclick='this.textContent=Number(this.textContent)+1'>0</button><input id='entry'><p id='bridge'></p><script>document.querySelector('#bridge').textContent=typeof window.__TAURI__</script>"
            } else {
                "<!doctype html><title>Login required</title><a id='login' href='/login'>Log in</a>"
            };
            let mut response = tiny_http::Response::from_string(html)
                .with_header(tiny_http::Header::from_bytes("Content-Type", "text/html").unwrap());
            if req.url() == "/login" {
                response.add_header(
                    tiny_http::Header::from_bytes(
                        "Set-Cookie",
                        "muniment-cef-test=1; Path=/; HttpOnly; SameSite=Strict",
                    )
                    .unwrap(),
                );
            }
            let _ = req.respond(response);
        }
    });
    let control = app.state::<Arc<Control>>().inner().clone();
    *control.active.lock().unwrap() = "browser".into();
    let pending = control.clone();
    let grants = control.clone();
    let allowed_origin = site.clone();
    cef_native::layout(
        app,
        "browser",
        &storage.root,
        Bounds {
            x: 20.0,
            y: 100.0,
            width: 720.0,
            height: 480.0,
        },
        Some(format!("{site}/secure")),
        move |id, success, bytes| {
            if let Some(tx) = pending.pending.lock().unwrap().remove(&id) {
                let _ = tx.send(if success {
                    serde_json::from_slice(&bytes).map_err(err)
                } else {
                    Err("CDP failed".into())
                });
            }
        },
        move |url| {
            grants.grants.lock().unwrap().clear();
            origin(&url).ok().as_deref() == Some(&allowed_origin)
        },
    )?;
    let wait = |expression: &str, expected: &str| -> Result<(), String> {
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            let actual = evaluate(app, "browser", expression.into());
            if actual.as_ref().ok().and_then(Value::as_str) == Some(expected) {
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err(format!(
                    "Timed out: {expected}. Browser answered {actual:?}"
                ));
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    };
    if phase == "write" {
        wait("document.title", "Login required")?;
        operate(app, request("browser", "grant", "", ""), false)?;
        operate(app, request("browser", "click", "", "#login"), true)?;
        wait("document.title", "Login complete")?;
        operate(app, request("browser", "grant", "", ""), false)?;
        operate(app, request("browser", "click", "", "#protected"), true)?;
    }
    wait("document.title", "Protected page")?;
    wait(
        "document.querySelector('#bridge')?.textContent",
        "undefined",
    )?;
    operate(app, request("browser", "grant", "", ""), false)?;
    operate(app, request("browser", "click", "", "#counter"), true)?;
    wait("document.querySelector('#counter')?.textContent", "1")?;
    operate(
        app,
        request("browser", "type", "CEF input verified", "#entry"),
        true,
    )?;
    wait(
        "document.querySelector('#entry')?.value",
        "CEF input verified",
    )?;
    let snapshot = operate(app, request("browser", "snapshot", "", ""), true)?;
    if !snapshot
        .as_str()
        .unwrap_or_default()
        .contains("Protected page")
    {
        return Err("Snapshot did not read the page".into());
    }
    let screenshot = cdp(
        app,
        "browser",
        "Page.captureScreenshot",
        json!({"format":"png"}),
    )?;
    std::fs::write(
        storage.root.join(format!("smoke-{phase}.png.base64")),
        screenshot["data"].as_str().ok_or("Missing screenshot")?,
    )
    .map_err(err)?;
    operate(app, request("browser", "stop", "", ""), true)?;
    if operate(app, request("browser", "snapshot", "", ""), true).is_ok() {
        return Err("Stopped control remained active".into());
    }
    Ok(())
}
pub fn start(app: tauri::AppHandle) {
    std::thread::spawn(move || {
        let phase = std::env::var("MUNIMENT_CEF_SMOKE_PHASE").unwrap_or_default();
        let result = if !matches!(phase.as_str(), "write" | "read")
            || std::env::var_os("MUNIMENT_STATE_DIR").is_none()
        {
            Err("The browser test requires its own profile and phase".into())
        } else {
            check(&app, &phase)
        };
        let code = if result.is_ok() { 0 } else { 1 };
        eprintln!("CEF_SMOKE {phase}: {result:?}");
        let root = app.state::<BrowserStorage>().root.clone();
        let _ = std::fs::write(
            root.join(format!("smoke-{phase}.json")),
            json!({"ok":result.is_ok(),"error":result.err()}).to_string(),
        );
        app.exit(code);
    });
}

use tauri::{Emitter, Manager, PhysicalPosition};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};

pub const SHORTCUT: &str = "Control+Alt+Space";

fn position(x: i32, y: i32, width: u32, height: u32, scale: f64) -> PhysicalPosition<i32> {
    PhysicalPosition::new(
        x + ((f64::from(width) - 600.0 * scale) / 2.0).max(0.0).round() as i32,
        y + (f64::from(height) / 3.0 - 40.0 * scale).max(0.0).round() as i32,
    )
}

#[tauri::command]
pub fn launcher_open(app: tauri::AppHandle) -> Result<(), String> {
    let window = app
        .get_webview_window("launcher")
        .ok_or("The launcher is unavailable.")?;
    let monitor = app
        .cursor_position()
        .ok()
        .and_then(|cursor| app.monitor_from_point(cursor.x, cursor.y).ok().flatten())
        .or(window
            .primary_monitor()
            .map_err(|error| error.to_string())?);
    if let Some(monitor) = monitor {
        let area = monitor.work_area();
        window
            .set_position(position(
                area.position.x,
                area.position.y,
                area.size.width,
                area.size.height,
                monitor.scale_factor(),
            ))
            .map_err(|error| error.to_string())?;
    }
    window.show().map_err(|error| error.to_string())?;
    window.set_focus().map_err(|error| error.to_string())?;
    window
        .emit("launcher-opened", ())
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn launcher_close(app: tauri::AppHandle) -> Result<(), String> {
    app.get_webview_window("launcher")
        .ok_or("The launcher is unavailable.")?
        .hide()
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn launcher_present_main(app: tauri::AppHandle) -> Result<(), String> {
    let main = app
        .get_webview_window("main")
        .ok_or("The main window is unavailable.")?;
    main.unminimize().map_err(|error| error.to_string())?;
    main.show().map_err(|error| error.to_string())?;
    main.set_focus().map_err(|error| error.to_string())?;
    launcher_close(app)
}

pub fn window_event(window: &tauri::Window, event: &tauri::WindowEvent) {
    match (window.label(), event) {
        ("launcher", tauri::WindowEvent::CloseRequested { api, .. }) => {
            api.prevent_close();
            if let Err(error) = window.hide() {
                eprintln!("The launcher could not close. Press Escape again: {error}");
            }
        }
        // The hidden launcher must not keep the app alive without its main window.
        ("main", tauri::WindowEvent::Destroyed) => window.app_handle().exit(0),
        _ => {}
    }
}

#[tauri::command]
pub fn launcher_register(app: tauri::AppHandle) -> Result<(), String> {
    // Option maps to Alt on macOS. Control stays Control on every platform.
    let shortcut = Shortcut::new(Some(Modifiers::CONTROL | Modifiers::ALT), Code::Space);
    app.global_shortcut()
        .on_shortcut(shortcut, |app, _, event| {
            if event.state == ShortcutState::Pressed {
                if let Err(error) = launcher_open(app.clone()) {
                    eprintln!("The launcher could not open. Try the shortcut again: {error}");
                }
            }
        })
        .map_err(|error| format!("The launcher shortcut could not register. Close conflicting apps and restart: {error}"))?;
    eprintln!("The launcher registered {SHORTCUT}.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn centers_the_launcher_in_the_upper_third_of_the_work_area() {
        assert_eq!(
            position(0, 0, 1920, 1080, 1.0),
            PhysicalPosition::new(660, 320)
        );
        assert_eq!(
            position(-3840, 40, 3840, 2160, 2.0),
            PhysicalPosition::new(-2520, 680)
        );
        assert_eq!(position(0, 0, 500, 100, 1.0), PhysicalPosition::new(0, 0));
    }

    #[test]
    fn config_keeps_the_launcher_hidden_fixed_and_above_other_apps() {
        let config: tauri::utils::config::Config =
            serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        let launcher = config
            .app
            .windows
            .iter()
            .find(|window| window.label == "launcher")
            .unwrap();
        assert_eq!((launcher.width, launcher.height), (600.0, 80.0));
        assert!(!launcher.visible && !launcher.decorations && !launcher.resizable);
        assert!(launcher.always_on_top && launcher.skip_taskbar);
    }
}

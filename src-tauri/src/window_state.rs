use serde::Deserialize;
use std::collections::HashMap;
use tauri::{utils::config::WindowConfig, Manager, PhysicalSize, Runtime};
use tauri_plugin_window_state::{AppHandleExt, StateFlags, WindowExt};

#[cfg(target_os = "macos")]
mod macos;

#[derive(Deserialize)]
struct SavedSize {
    width: u32,
    height: u32,
}

fn initial_size(config: &WindowConfig, saved: &[u8], scale_factor: f64) -> PhysicalSize<u32> {
    let saved = serde_json::from_slice::<HashMap<String, SavedSize>>(saved)
        .ok()
        .and_then(|mut states| states.remove(&config.label));
    let size = saved.map_or_else(
        || tauri::LogicalSize::new(config.width, config.height).to_physical(scale_factor),
        |size| PhysicalSize::new(size.width, size.height),
    );
    clamp_size(size, config, scale_factor)
}

fn clamp_size(
    size: PhysicalSize<u32>,
    config: &WindowConfig,
    scale_factor: f64,
) -> PhysicalSize<u32> {
    // Configured sizes use logical points. The plugin saves physical pixels.
    PhysicalSize::new(
        size.width
            .max((config.min_width.unwrap_or(0.0) * scale_factor).ceil() as u32),
        size.height
            .max((config.min_height.unwrap_or(0.0) * scale_factor).ceil() as u32),
    )
}

pub fn restore_main_window<R: Runtime>(app: &tauri::App<R>) -> tauri::Result<()> {
    let config = app
        .config()
        .app
        .windows
        .iter()
        .find(|config| config.label == "main")
        .expect("The main window requires a window configuration.");
    let window = app
        .get_webview_window(&config.label)
        .expect("The main window must exist before setup.");
    let state = muniment_runtime::profile_directory()
        .map_err(|error| tauri::Error::Io(std::io::Error::other(error)))?;
    let saved = std::fs::read(state.join(app.handle().filename())).unwrap_or_default();

    let resized_window = window.clone();
    let resized_config = config.clone();
    window.on_window_event(move |event| {
        // A restored position can move the window to a display with a different scale.
        if let tauri::WindowEvent::ScaleFactorChanged {
            scale_factor,
            new_inner_size,
            ..
        } = event
        {
            let size = clamp_size(*new_inner_size, &resized_config, *scale_factor);
            if size != *new_inner_size {
                if let Err(error) = resized_window.set_size(size) {
                    eprintln!("Could not apply the minimum window size. Restart the app: {error}");
                }
            }
        }
    });

    // Do not query inner_size after restore_state. Native resize requests can run later.
    // Read only the saved size and leave the plugin's file and other state intact.
    window.restore_state(StateFlags::POSITION | StateFlags::DECORATIONS)?;
    window.set_size(initial_size(config, &saved, window.scale_factor()?))?;
    window.restore_state(StateFlags::MAXIMIZED | StateFlags::FULLSCREEN)?;
    #[cfg(target_os = "macos")]
    macos::install(&window, config)?;
    window.show()?;
    // Apply the configured inset after show. Native frame observers keep later layouts aligned.
    #[cfg(target_os = "macos")]
    macos::apply();
    window.set_focus()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> WindowConfig {
        let config: tauri::utils::config::Config =
            serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        config.app.windows.into_iter().next().unwrap()
    }

    #[test]
    fn clamps_a_saved_window_below_the_configured_minimum() {
        let config = config();
        assert_eq!(config.label, "main");
        assert!(!config.visible);
        let saved = br#"{"main":{"width":574,"height":418,"x":0,"y":0,"prev_x":0,"prev_y":0,"maximized":false,"visible":true,"decorated":true,"fullscreen":false}}"#;
        for scale_factor in [1.0, 1.25, 2.0] {
            assert_eq!(
                initial_size(&config, saved, scale_factor),
                PhysicalSize::new(
                    (config.min_width.unwrap() * scale_factor).ceil() as u32,
                    (config.min_height.unwrap() * scale_factor).ceil() as u32,
                )
            );
        }
    }

    #[test]
    fn preserves_valid_sizes_and_clamps_each_axis() {
        let config = config();
        for (saved, expected) in [
            ((960, 640), (960, 640)),
            ((1400, 900), (1400, 900)),
            ((574, 900), (960, 900)),
            ((1400, 418), (1400, 640)),
            ((0, 0), (960, 640)),
        ] {
            let bytes = serde_json::to_vec(&serde_json::json!({
                "main": { "width": saved.0, "height": saved.1 }
            }))
            .unwrap();
            assert_eq!(initial_size(&config, &bytes, 1.0), expected.into());
        }
        assert_eq!(
            initial_size(&config, br#"{"main":{"width":2200,"height":1440}}"#, 2.0),
            PhysicalSize::new(2200, 1440)
        );
    }

    #[test]
    fn uses_configured_defaults_for_missing_or_invalid_saved_sizes() {
        let config = config();
        for saved in [
            "",
            "not json",
            "{}",
            r#"{"other":{"width":574,"height":418}}"#,
            r#"{"main":{"width":574}}"#,
            r#"{"main":{"width":-1,"height":418}}"#,
            r#"{"main":{"width":"574","height":418}}"#,
            r#"{"main":{"width":4294967296,"height":418}}"#,
        ] {
            assert_eq!(
                initial_size(&config, saved.as_bytes(), 2.0),
                tauri::LogicalSize::new(config.width, config.height).to_physical(2.0)
            );
        }
    }

    #[test]
    fn clamps_configured_defaults_and_rounds_minimums_up() {
        let mut config = config();
        config.width = 574.0;
        config.height = 418.0;
        config.min_width = Some(960.1);
        config.min_height = Some(640.1);
        assert_eq!(initial_size(&config, b"", 1.25), (1201, 801).into());
        config.min_width = None;
        config.min_height = None;
        assert_eq!(initial_size(&config, b"", 1.0), (574, 418).into());
    }
}

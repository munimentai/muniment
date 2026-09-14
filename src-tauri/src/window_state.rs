use serde::Deserialize;
use std::collections::HashMap;
use tauri::{
    utils::config::WindowConfig, Manager, PhysicalPosition, PhysicalRect, PhysicalSize, Runtime,
};
use tauri_plugin_window_state::{AppHandleExt, StateFlags, WindowExt};

#[cfg(target_os = "macos")]
mod macos;

#[derive(Deserialize)]
struct SavedState {
    width: u32,
    height: u32,
    #[serde(default)]
    x: Option<i32>,
    #[serde(default)]
    y: Option<i32>,
    #[serde(default)]
    maximized: bool,
    #[serde(default)]
    fullscreen: bool,
}

fn saved_state(config: &WindowConfig, saved: &[u8]) -> Option<SavedState> {
    serde_json::from_slice::<HashMap<String, SavedState>>(saved)
        .ok()
        .and_then(|mut states| states.remove(&config.label))
}

fn initial_size(config: &WindowConfig, saved: &[u8], scale_factor: f64) -> PhysicalSize<u32> {
    let size = saved_state(config, saved).map_or_else(
        || tauri::LogicalSize::new(config.width, config.height).to_physical(scale_factor),
        |state| PhysicalSize::new(state.width, state.height),
    );
    clamp_size(size, config, scale_factor)
}

// The plugin saves the outer position in physical pixels beside the size.
fn saved_position(config: &WindowConfig, saved: &[u8]) -> Option<PhysicalPosition<i32>> {
    let state = saved_state(config, saved)?;
    Some(PhysicalPosition::new(state.x?, state.y?))
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

// A restored frame can come from a larger display or a display that is gone.
// The outer frame shrinks to the work area, then moves until it sits inside it.
fn fit_to_work_area(
    outer: PhysicalSize<u32>,
    position: PhysicalPosition<i32>,
    work_area: &PhysicalRect<i32, u32>,
) -> (PhysicalSize<u32>, PhysicalPosition<i32>) {
    let size = PhysicalSize::new(
        outer.width.min(work_area.size.width),
        outer.height.min(work_area.size.height),
    );
    let far = |origin: i32, extent: u32, length: u32| {
        origin.saturating_add(i32::try_from(extent - length).unwrap_or(i32::MAX))
    };
    let position = PhysicalPosition::new(
        position.x.clamp(
            work_area.position.x,
            far(work_area.position.x, work_area.size.width, size.width),
        ),
        position.y.clamp(
            work_area.position.y,
            far(work_area.position.y, work_area.size.height, size.height),
        ),
    );
    (size, position)
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

    // The decorations around the content keep their size across a restore, so measure
    // them before the restore and fit the outer frame to the display.
    let chrome = {
        let outer = window.outer_size()?;
        let inner = window.inner_size()?;
        PhysicalSize::new(
            outer.width.saturating_sub(inner.width),
            outer.height.saturating_sub(inner.height),
        )
    };
    // Do not query inner_size after restore_state. Native resize requests can run later.
    // Read only the saved size and leave the plugin's file and other state intact.
    window.restore_state(StateFlags::POSITION | StateFlags::DECORATIONS)?;
    let scale_factor = window.scale_factor()?;
    let mut size = initial_size(config, &saved, scale_factor);
    let monitor = match window.current_monitor()? {
        Some(monitor) => Some(monitor),
        None => window.primary_monitor()?,
    };
    if let Some(monitor) = monitor {
        let outer = PhysicalSize::new(size.width + chrome.width, size.height + chrome.height);
        let position = match saved_position(config, &saved) {
            Some(position) => position,
            None => window.outer_position()?,
        };
        let (fitted, position) = fit_to_work_area(outer, position, monitor.work_area());
        size = clamp_size(
            PhysicalSize::new(
                fitted.width.saturating_sub(chrome.width),
                fitted.height.saturating_sub(chrome.height),
            ),
            config,
            scale_factor,
        );
        window.set_position(position)?;
    }
    window.set_size(size)?;
    // Restoring a false maximized and fullscreen pair through the plugin zooms the
    // window to the whole display on macOS. Restore the two flags only when the
    // saved state holds one of them.
    if saved_state(config, &saved).is_some_and(|state| state.maximized || state.fullscreen) {
        window.restore_state(StateFlags::MAXIMIZED | StateFlags::FULLSCREEN)?;
    }
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
    fn reads_the_saved_position_only_when_both_axes_are_present() {
        let config = config();
        let saved = br#"{"main":{"width":1720,"height":1409,"x":0,"y":31,"prev_x":0,"prev_y":31,"maximized":false,"visible":true,"decorated":true,"fullscreen":false}}"#;
        assert_eq!(saved_position(&config, saved), Some((0, 31).into()));
        assert_eq!(initial_size(&config, saved, 1.0), (1720, 1409).into());
        let state = saved_state(&config, saved).unwrap();
        assert!(!state.maximized && !state.fullscreen);
        let zoomed = br#"{"main":{"width":1720,"height":1409,"x":0,"y":31,"maximized":true}}"#;
        assert!(saved_state(&config, zoomed).unwrap().maximized);
        for saved in [
            "",
            r#"{"main":{"width":1720,"height":1409}}"#,
            r#"{"main":{"width":1720,"height":1409,"x":5}}"#,
            r#"{"other":{"width":1720,"height":1409,"x":5,"y":6}}"#,
        ] {
            assert_eq!(saved_position(&config, saved.as_bytes()), None);
        }
    }

    #[test]
    fn the_default_window_is_1280_by_800() {
        let config = config();
        assert_eq!((config.width, config.height), (1280.0, 800.0));
        assert_eq!(initial_size(&config, b"", 2.0), (2560, 1600).into());
    }

    #[test]
    fn fits_an_oversized_frame_to_the_work_area_and_keeps_it_on_screen() {
        let work_area = PhysicalRect {
            position: PhysicalPosition::new(0, 50),
            size: PhysicalSize::new(2560, 1550),
        };
        // The saved frame from a larger display.
        assert_eq!(
            fit_to_work_area((3440, 2818).into(), (0, 62).into(), &work_area),
            ((2560, 1550).into(), (0, 50).into())
        );
        // A frame that fits stays where it was saved.
        assert_eq!(
            fit_to_work_area((1280, 800).into(), (300, 200).into(), &work_area),
            ((1280, 800).into(), (300, 200).into())
        );
        // A frame past the right and bottom edges moves back inside.
        assert_eq!(
            fit_to_work_area((1280, 800).into(), (2000, 1400).into(), &work_area),
            ((1280, 800).into(), (1280, 800).into())
        );
        // A frame above or left of the work area moves to its origin.
        assert_eq!(
            fit_to_work_area((1280, 800).into(), (-400, -10).into(), &work_area),
            ((1280, 800).into(), (0, 50).into())
        );
        // A second display to the left has a negative origin.
        let left = PhysicalRect {
            position: PhysicalPosition::new(-1920, 0),
            size: PhysicalSize::new(1920, 1080),
        };
        assert_eq!(
            fit_to_work_area((2000, 1200).into(), (-1900, 100).into(), &left),
            ((1920, 1080).into(), (-1920, 0).into())
        );
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

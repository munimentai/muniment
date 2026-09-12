// AppKit dispatch stays inside the E2E app. It needs no Apple Events or
// Accessibility grant and never posts events to another process.
use objc2::{rc::Retained, MainThreadMarker};
use objc2_app_kit::{NSApplication, NSEvent, NSEventModifierFlags, NSEventType, NSOpenPanel};
use objc2_foundation::{NSPoint, NSString};
use std::{
    cell::RefCell,
    path::Path,
    time::{Duration, Instant},
};

struct Drive {
    panel: Retained<NSOpenPanel>,
    home: String,
    step: u8,
    next: Instant,
}

thread_local! {
    static DRIVE: RefCell<Option<Drive>> = const { RefCell::new(None) };
}

fn key(
    app: &NSApplication,
    text: &str,
    code: u16,
    flags: NSEventModifierFlags,
) -> Result<(), String> {
    let window = app
        .keyWindow()
        .ok_or("The Home picker has no key window.")?;
    let text = NSString::from_str(text);
    for kind in [NSEventType::KeyDown, NSEventType::KeyUp] {
        let event = NSEvent::keyEventWithType_location_modifierFlags_timestamp_windowNumber_context_characters_charactersIgnoringModifiers_isARepeat_keyCode(
            kind, NSPoint::new(0.0, 0.0), flags, 0.0, window.windowNumber(), None, &text, &text, false, code,
        ).ok_or("The Home picker could not make a key event.")?;
        app.sendEvent(&event);
    }
    Ok(())
}

fn poll(home: String) -> Result<bool, String> {
    let mtm = MainThreadMarker::new().ok_or("The Home picker needs the AppKit thread.")?;
    let app = NSApplication::sharedApplication(mtm);
    DRIVE.with(|slot| {
        let mut slot = slot.borrow_mut();
        if slot.is_none() {
            let windows = app.windows();
            let panels: Vec<_> = (0..windows.len())
                .filter_map(|index| windows.objectAtIndex(index).downcast::<NSOpenPanel>().ok())
                .filter(|panel| panel.isVisible())
                .collect();
            if panels.len() > 1 {
                return Err("Muniment has more than one visible NSOpenPanel.".into());
            }
            let Some(panel) = panels.into_iter().next() else {
                return Ok(false);
            };
            if !panel.canChooseDirectories()
                || panel.canChooseFiles()
                || panel.allowsMultipleSelection()
            {
                return Err("The NSOpenPanel is not a single-folder picker.".into());
            }
            *slot = Some(Drive {
                panel,
                home: home.clone(),
                step: 0,
                next: Instant::now(),
            });
        }
        let drive = slot
            .as_mut()
            .ok_or("The Home picker drive is unavailable.")?;
        if drive.home != home {
            return Err("The Home path changed during the picker drive.".into());
        }
        if Instant::now() < drive.next {
            return Ok(false);
        }
        if drive.step == 4 {
            if drive.panel.isVisible() {
                return Ok(false);
            }
            let urls = drive.panel.URLs();
            if urls.len() != 1
                || urls
                    .objectAtIndex(0)
                    .path()
                    .map(|path| path.to_string())
                    .as_deref()
                    != Some(home.as_str())
            {
                return Err("The NSOpenPanel did not select the isolated Home.".into());
            }
            *slot = None;
            return Ok(true);
        }
        if !drive.panel.isVisible() {
            return Err("The NSOpenPanel closed before the picker drive finished.".into());
        }
        let window = app
            .keyWindow()
            .ok_or("The Home picker has no key window.")?;
        // Accept only this panel or its sheet. Never type into the composer.
        if window.windowNumber() != drive.panel.windowNumber()
            && window
                .sheetParent()
                .is_none_or(|parent| parent.windowNumber() != drive.panel.windowNumber())
        {
            return Err("The NSOpenPanel lost keyboard focus.".into());
        }
        match drive.step {
            0 => key(
                &app,
                "G",
                5,
                NSEventModifierFlags::Command | NSEventModifierFlags::Shift,
            )?,
            1 => {
                key(&app, "a", 0, NSEventModifierFlags::Command)?;
                key(&app, &home, 0, NSEventModifierFlags::empty())?;
            }
            2 | 3 => key(&app, "\r", 36, NSEventModifierFlags::empty())?,
            _ => return Err("The Home picker drive has an invalid step.".into()),
        }
        drive.step += 1;
        // Give the native Go to Folder sheet time to show, navigate, and close.
        drive.next = Instant::now() + Duration::from_millis(500);
        Ok(false)
    })
}

#[derive(serde::Serialize)]
pub(crate) struct WindowSnapshot {
    class: String,
    title: String,
    visible: bool,
}

#[derive(serde::Serialize)]
pub(crate) struct FolderDialogSnapshot {
    windows: Vec<WindowSnapshot>,
    step: Option<u8>,
}

#[tauri::command]
pub(crate) async fn e2e_folder_dialog_snapshot(
    app: tauri::AppHandle,
) -> Result<FolderDialogSnapshot, String> {
    if std::env::var("MUNIMENT_E2E_ONBOARDING_ONLY").as_deref() != Ok("1") {
        return Err("The Home picker snapshot needs the E2E onboarding runner.".into());
    }
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    app.run_on_main_thread(move || {
        let result = (|| {
            let mtm = MainThreadMarker::new().ok_or("The Home picker needs the AppKit thread.")?;
            let app = NSApplication::sharedApplication(mtm);
            let windows = app.windows();
            Ok(FolderDialogSnapshot {
                windows: (0..windows.len())
                    .map(|index| {
                        let window = windows.objectAtIndex(index);
                        WindowSnapshot {
                            class: window.class().name().to_string_lossy().into_owned(),
                            title: window.title().to_string(),
                            visible: window.isVisible(),
                        }
                    })
                    .collect(),
                step: DRIVE.with(|slot| slot.borrow().as_ref().map(|drive| drive.step)),
            })
        })();
        let _ = sender.send(result);
    })
    .map_err(|error| error.to_string())?;
    receiver.recv().map_err(|error| error.to_string())?
}

#[tauri::command]
pub(crate) async fn e2e_drive_folder_dialog(app: tauri::AppHandle) -> Result<bool, String> {
    if std::env::var("MUNIMENT_E2E_ONBOARDING_ONLY").as_deref() != Ok("1") {
        return Err("The Home picker drive needs the E2E onboarding runner.".into());
    }
    let home =
        std::env::var("MUNIMENT_E2E_HOME_PATH").map_err(|_| "The isolated Home is unavailable.")?;
    if !Path::new(&home).is_absolute()
        || !Path::new(&home).is_dir()
        || home.chars().any(char::is_control)
    {
        return Err(
            "The Home picker needs an absolute folder path without control characters.".into(),
        );
    }
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    app.run_on_main_thread(move || {
        let _ = sender.send(poll(home));
    })
    .map_err(|error| error.to_string())?;
    receiver.recv().map_err(|error| error.to_string())?
}

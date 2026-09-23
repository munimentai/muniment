// The E2E app posts its key events from its own process to the HID event tap.
// It needs no Apple Events, no second process and no UI automation from outside.
// The CI harness session already trusts the app process for accessibility, and
// the drive checks that trust before its first key so a template change reads
// as a cause and not a timeout.
use core_graphics::event::{CGEvent, CGEventFlags, CGEventTapLocation};
use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};
use objc2::{rc::Retained, MainThreadMarker};
use objc2_app_kit::{NSApplication, NSOpenPanel};
use std::{
    cell::RefCell,
    path::Path,
    time::{Duration, Instant},
};

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXIsProcessTrusted() -> bool;
}

fn process_trusted() -> bool {
    // SAFETY: AXIsProcessTrusted takes no arguments and reads process state alone.
    unsafe { AXIsProcessTrusted() }
}

struct Drive {
    panel: Retained<NSOpenPanel>,
    home: String,
    step: u8,
    next: Instant,
    focus_deadline: Instant,
}

thread_local! {
    static DRIVE: RefCell<Option<Drive>> = const { RefCell::new(None) };
}

fn key(text: &str, code: u16, flags: CGEventFlags) -> Result<(), String> {
    let source = CGEventSource::new(CGEventSourceStateID::HIDSystemState)
        .map_err(|_| "The Home picker could not make an event source.")?;
    for down in [true, false] {
        let event = CGEvent::new_keyboard_event(source.clone(), code, down)
            .map_err(|_| "The Home picker could not make a key event.")?;
        event.set_flags(flags);
        if !text.is_empty() {
            event.set_string(text);
        }
        event.post(CGEventTapLocation::HID);
    }
    Ok(())
}

fn type_text(text: &str) -> Result<(), String> {
    let mut buffer = [0u8; 4];
    for character in text.chars() {
        key(character.encode_utf8(&mut buffer), 0, CGEventFlags::empty())?;
    }
    Ok(())
}

fn panel_directory(panel: &NSOpenPanel) -> Option<String> {
    panel.directoryURL()?.path().map(|path| path.to_string())
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
            if !process_trusted() {
                return Err(
                    "The Home picker needs Accessibility trust for the app process, and AXIsProcessTrusted reads false.".into(),
                );
            }
            app.activate();
            *slot = Some(Drive {
                panel,
                home: home.clone(),
                step: 0,
                next: Instant::now() + Duration::from_millis(100),
                focus_deadline: Instant::now() + Duration::from_secs(5),
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
            let selected = (urls.len() == 1)
                .then(|| urls.objectAtIndex(0).path().map(|path| path.to_string()))
                .flatten();
            // The panel answers a resolved path, so compare canonical forms.
            let canonical = |path: &str| std::fs::canonicalize(path).ok();
            if selected.as_deref().and_then(canonical) != canonical(&home)
                || canonical(&home).is_none()
            {
                return Err(format!(
                    "The NSOpenPanel did not select the isolated Home. The panel selected {selected:?} and the drive expected {home}."
                ));
            }
            *slot = None;
            return Ok(true);
        }
        if !drive.panel.isVisible() {
            return Err("The NSOpenPanel closed before the picker drive finished.".into());
        }
        let Some(window) = app.keyWindow() else {
            // Activation is asynchronous. Never send keys until AppKit assigns focus.
            if drive.step == 0 && Instant::now() < drive.focus_deadline {
                return Ok(false);
            }
            return Err("The Home picker has no key window.".into());
        };
        // Accept only this panel or its sheet. Never type into the composer.
        if window.windowNumber() != drive.panel.windowNumber()
            && window
                .sheetParent()
                .is_none_or(|parent| parent.windowNumber() != drive.panel.windowNumber())
        {
            return Err(format!(
                "The NSOpenPanel lost keyboard focus. The drive stopped at step {} with the panel directory {:?}.",
                drive.step,
                panel_directory(&drive.panel)
            ));
        }
        match drive.step {
            0 => key("", 5, CGEventFlags::CGEventFlagCommand | CGEventFlags::CGEventFlagShift)?,
            1 => {
                key("", 0, CGEventFlags::CGEventFlagCommand)?;
                type_text(&home)?;
            }
            2 | 3 => key("", 36, CGEventFlags::empty())?,
            _ => return Err("The Home picker drive has an invalid step.".into()),
        }
        drive.step += 1;
        // Give the native Go to Folder sheet time to show, navigate, and close.
        drive.next = Instant::now() + Duration::from_millis(700);
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
    directory: Option<String>,
    trusted: bool,
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
                directory: DRIVE.with(|slot| {
                    slot.borrow()
                        .as_ref()
                        .and_then(|drive| panel_directory(&drive.panel))
                }),
                trusted: process_trusted(),
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

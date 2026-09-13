// The E2E app posts CGEvent keys to the NSOpenPanel view service through the HID event tap.
use core_graphics::{
    event::{CGEvent, CGEventFlags, CGEventTapLocation},
    event_source::{CGEventSource, CGEventSourceStateID},
};
use objc2::{rc::Retained, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{NSApplication, NSOpenPanel};
use std::{cell::RefCell, path::Path, time::Instant};

mod drive;
use drive::{Drive, FolderPanel};

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXIsProcessTrusted() -> bool;
}

pub(crate) fn is_trusted() -> bool {
    // This query never requests a grant or opens a privacy prompt.
    unsafe { AXIsProcessTrusted() }
}

impl FolderPanel for Retained<NSOpenPanel> {
    fn directory(&self) -> Option<String> {
        self.directoryURL()?.path().map(|path| path.to_string())
    }

    fn is_visible(&self) -> bool {
        self.isVisible()
    }

    fn is_focused(&self) -> bool {
        let app = NSApplication::sharedApplication(self.mtm());
        // HID events follow global focus. A stale app key window alone does not prove focus.
        app.isActive()
            && app.keyWindow().is_some_and(|window| {
                window.windowNumber() == self.windowNumber()
                    || window
                        .sheetParent()
                        .is_some_and(|parent| parent.windowNumber() == self.windowNumber())
            })
    }

    fn is_trusted(&self) -> bool {
        is_trusted()
    }

    fn send_step(&self, step: u8, home: &str) -> Result<(), String> {
        match step {
            0 => key(
                None,
                5,
                CGEventFlags::CGEventFlagCommand | CGEventFlags::CGEventFlagShift,
            ),
            1 => {
                key(None, 0, CGEventFlags::CGEventFlagCommand)?;
                // Each scalar gets its own event pair, including both UTF-16 units of a surrogate pair.
                for character in home.chars() {
                    if !self.is_focused() {
                        return Err("The NSOpenPanel lost keyboard focus.".into());
                    }
                    key(Some(character), 0, CGEventFlags::empty())?;
                }
                Ok(())
            }
            2 | 3 => key(None, 36, CGEventFlags::empty()),
            _ => Err("The Home picker drive has an invalid step.".into()),
        }
    }

    fn selected_paths(&self) -> Vec<String> {
        let urls = self.URLs();
        (0..urls.len())
            .map(|index| {
                urls.objectAtIndex(index)
                    .path()
                    .map(|path| path.to_string())
                    .unwrap_or_default()
            })
            .collect()
    }
}

thread_local! {
    static DRIVE: RefCell<Option<Drive<Retained<NSOpenPanel>>>> = const { RefCell::new(None) };
}

fn key(character: Option<char>, code: u16, flags: CGEventFlags) -> Result<(), String> {
    let source = CGEventSource::new(CGEventSourceStateID::Private)
        .map_err(|_| "The Home picker could not make a CGEvent source.")?;
    // Allocate both events before posting, so allocation failure cannot leave a key down.
    let down = CGEvent::new_keyboard_event(source.clone(), code, true)
        .map_err(|_| "The Home picker could not make a CGEvent key-down event.")?;
    let up = CGEvent::new_keyboard_event(source, code, false)
        .map_err(|_| "The Home picker could not make a CGEvent key-up event.")?;
    for event in [down, up] {
        event.set_flags(flags);
        if let Some(character) = character {
            let mut units = [0; 2];
            event.set_string_from_utf16_unchecked(character.encode_utf16(&mut units));
        }
        event.post(CGEventTapLocation::HID);
    }
    Ok(())
}

pub(crate) fn poll(home: String) -> Result<bool, String> {
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
            if is_trusted() {
                #[allow(deprecated)]
                app.activateIgnoringOtherApps(true);
                panel.makeKeyAndOrderFront(None);
            }
            *slot = Some(Drive::new(panel, home.clone(), Instant::now()));
        }
        let closed = slot
            .as_mut()
            .ok_or("The Home picker drive is unavailable.")?
            .poll(&home, Instant::now())?;
        if closed {
            *slot = None;
        }
        Ok(closed)
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

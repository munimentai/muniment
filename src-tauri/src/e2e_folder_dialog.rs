// The E2E app uses its own NSOpenPanel API without Apple Events or Accessibility grants.
use objc2::{rc::Retained, MainThreadMarker};
use objc2_app_kit::{NSApplication, NSOpenPanel};
use objc2_foundation::{NSString, NSURL};
use std::{cell::RefCell, path::Path, time::Instant};

mod drive;
use drive::{Drive, FolderPanel};

impl FolderPanel for Retained<NSOpenPanel> {
    fn directory(&self) -> Option<String> {
        self.directoryURL()?.path().map(|path| path.to_string())
    }

    fn set_directory(&self, home: &str) {
        let url = NSURL::fileURLWithPath_isDirectory(&NSString::from_str(home), true);
        self.setDirectoryURL(Some(&url));
    }

    fn is_visible(&self) -> bool {
        self.isVisible()
    }

    fn accept(&self) {
        // A nil sender carries no object type requirements.
        unsafe { self.ok(None) };
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

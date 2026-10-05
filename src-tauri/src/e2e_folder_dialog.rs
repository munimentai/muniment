// The E2E drive navigates the validated panel and presses its Accessibility button.
// It sends no keyboard events and does not need app activation.
use core_foundation::{
    array::CFArray,
    base::{CFType, CFTypeRef, TCFType},
    boolean::CFBoolean,
    string::{CFString, CFStringRef},
};
use objc2::{rc::Retained, MainThreadMarker};
use objc2_app_kit::{NSAccessibility, NSApplication, NSOpenPanel, NSWindow, NSWorkspace};
use objc2_foundation::{NSString, NSURL};
use std::{
    cell::RefCell,
    path::Path,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

mod accessibility;

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXIsProcessTrusted() -> bool;
    fn AXUIElementCreateApplication(pid: i32) -> CFTypeRef;
    fn AXUIElementGetTypeID() -> usize;
    fn AXUIElementCopyAttributeValue(
        element: CFTypeRef,
        attribute: CFStringRef,
        value: *mut CFTypeRef,
    ) -> i32;
    fn AXUIElementPerformAction(element: CFTypeRef, action: CFStringRef) -> i32;
    fn AXUIElementSetMessagingTimeout(element: CFTypeRef, timeout: f32) -> i32;
}

fn process_trusted() -> bool {
    // SAFETY: AXIsProcessTrusted takes no arguments and reads process state alone.
    unsafe { AXIsProcessTrusted() }
}

#[derive(Clone, serde::Serialize)]
struct ConfirmLookup {
    failures: Vec<&'static str>,
    candidates: Vec<accessibility::ConfirmCandidate>,
}

type ConfirmLookups = Arc<Mutex<Vec<ConfirmLookup>>>;

struct Drive {
    panel: Retained<NSOpenPanel>,
    home: String,
    step: u8,
    next: Instant,
    deadline: Instant,
    identifier: String,
    confirm_lookups: ConfirmLookups,
}

enum Progress {
    Waiting,
    Complete,
    Confirm(String, Instant, ConfirmLookups),
}

thread_local! {
    static DRIVE: RefCell<Option<Drive>> = const { RefCell::new(None) };
    static DEADLINE: RefCell<Option<Instant>> = const { RefCell::new(None) };
}

fn ax_attribute(element: &CFType, name: &str) -> Result<CFType, String> {
    // SAFETY: The type check rejects non-AX objects before the AX call.
    if element.type_of() != unsafe { AXUIElementGetTypeID() } {
        return Err("The Home picker received a non-Accessibility element.".into());
    }
    // SAFETY: The type check above validates element. Bound each element's calls.
    let status = unsafe { AXUIElementSetMessagingTimeout(element.as_CFTypeRef(), 1.0) };
    if status != 0 {
        return Err(format!(
            "The Home picker could not bound Accessibility calls. Accessibility returned {status}."
        ));
    }
    let name = CFString::new(name);
    let mut value = std::ptr::null();
    // SAFETY: Both inputs stay alive, and value points to writable storage.
    let status = unsafe {
        AXUIElementCopyAttributeValue(
            element.as_CFTypeRef(),
            name.as_concrete_TypeRef(),
            &mut value,
        )
    };
    if status != 0 || value.is_null() {
        return Err(format!(
            "The Home picker could not read {name}. Accessibility returned {status}."
        ));
    }
    // SAFETY: A successful Copy call returns an owned Core Foundation object.
    Ok(unsafe { CFType::wrap_under_create_rule(value) })
}

fn press_confirm(
    identifier: &str,
    deadline: Instant,
    confirm_lookups: ConfirmLookups,
) -> Result<(), String> {
    // Run AX client calls off the AppKit thread so the app can answer them.
    // SAFETY: The current process exists, and Create returns an owned AX element.
    let app = unsafe {
        CFType::wrap_under_create_rule(AXUIElementCreateApplication(std::process::id() as i32))
    };
    let button = loop {
        if Instant::now() >= deadline {
            return Err("The Home picker drive timed out before confirmation.".into());
        }
        let mut failures = Vec::new();
        let mut candidates = Vec::new();
        let result = accessibility::confirm_button(
            &NativeAccessibility,
            &app,
            identifier,
            &mut failures,
            &mut candidates,
            || {
                if Instant::now() >= deadline {
                    Err(accessibility::ConfirmFailure::Deadline)
                } else {
                    Ok(())
                }
            },
        );
        if let Err(reason) = &result {
            failures.push(*reason);
        }
        confirm_lookups
            .lock()
            .map_err(|_| "The Home picker lookup snapshot is unavailable.")?
            .push(ConfirmLookup {
                failures: failures
                    .into_iter()
                    .map(|failure| failure.reason())
                    .collect(),
                candidates,
            });
        match result {
            Ok(Some(button)) => break button,
            Ok(None) => {}
            Err(reason) => {
                return Err(format!(
                    "The Home picker confirmation lookup failed: {}.",
                    reason.reason()
                ));
            }
        }
        std::thread::sleep(Duration::from_millis(100));
    };
    if Instant::now() >= deadline {
        return Err("The Home picker drive timed out before confirmation.".into());
    }
    let action = CFString::new("AXPress");
    // SAFETY: The attribute reads validated button as an AX element in this panel.
    let status =
        unsafe { AXUIElementPerformAction(button.as_CFTypeRef(), action.as_concrete_TypeRef()) };
    // AX can time out after a successful press. Never retry an uncertain action.
    // The next poll still requires panel closure and the exact selected Home.
    const AX_CANNOT_COMPLETE: i32 = -25204;
    if status != 0 && status != AX_CANNOT_COMPLETE {
        return Err(format!("The Home picker could not press its confirmation button. Accessibility returned {status}."));
    }
    Ok(())
}

struct NativeAccessibility;

impl NativeAccessibility {
    fn validate_element(value: CFType) -> Result<CFType, String> {
        // SAFETY: AXUIElementGetTypeID takes no arguments and returns the AX element type ID.
        if value.type_of() != unsafe { AXUIElementGetTypeID() } {
            return Err("The Home picker needs an Accessibility element.".into());
        }
        Ok(value)
    }
}

impl accessibility::Accessibility for NativeAccessibility {
    type Element = CFType;

    fn elements(&self, element: &CFType, name: &str) -> Result<Vec<CFType>, String> {
        let array = ax_attribute(element, name)?
            .downcast::<CFArray>()
            .ok_or("The Home picker needs an Accessibility element list.")?;
        Ok(array
            .iter()
            // SAFETY: The array owns these Core Foundation objects.
            .map(|element| unsafe { CFType::wrap_under_get_rule(*element) })
            .collect())
    }

    fn element(&self, element: &CFType, name: &str) -> Result<CFType, String> {
        Self::validate_element(ax_attribute(element, name)?)
    }

    fn string(&self, element: &CFType, name: &str) -> Result<String, String> {
        ax_attribute(element, name)?
            .downcast::<CFString>()
            .map(|value| value.to_string())
            .ok_or_else(|| format!("The Home picker needs text for {name}."))
    }

    fn boolean(&self, element: &CFType, name: &str) -> Result<bool, String> {
        ax_attribute(element, name)?
            .downcast::<CFBoolean>()
            .map(bool::from)
            .ok_or_else(|| format!("The Home picker needs a boolean for {name}."))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_element_validation_rejects_non_elements() {
        for value in [
            CFString::new("private AX text").as_CFType(),
            CFBoolean::true_value().as_CFType(),
        ] {
            assert_eq!(
                NativeAccessibility::validate_element(value).unwrap_err(),
                "The Home picker needs an Accessibility element."
            );
        }
    }

    #[test]
    fn native_element_validation_preserves_ax_elements() {
        // SAFETY: The current process exists, and Create returns an owned AX element.
        let element = unsafe {
            CFType::wrap_under_create_rule(AXUIElementCreateApplication(std::process::id() as i32))
        };
        assert_eq!(
            NativeAccessibility::validate_element(element.clone()).unwrap(),
            element
        );
    }
}

fn panel_directory(panel: &NSOpenPanel) -> Option<String> {
    panel.directoryURL()?.path().map(|path| path.to_string())
}

fn poll(home: String) -> Result<Progress, String> {
    let mtm = MainThreadMarker::new().ok_or("The Home picker needs the AppKit thread.")?;
    let app = NSApplication::sharedApplication(mtm);
    // Count the panel search too. Stop native actions before the runner's 30-second timeout.
    let deadline = DEADLINE.with(|slot| {
        *slot
            .borrow_mut()
            .get_or_insert_with(|| Instant::now() + Duration::from_secs(15))
    });
    if Instant::now() >= deadline {
        return Err("The Home picker drive timed out.".into());
    }
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
                return Ok(Progress::Waiting);
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
            let identifier = format!("muniment-e2e-home-{}", uuid::Uuid::now_v7());
            let marker = NSString::from_str(&identifier);
            panel.setAccessibilityIdentifier(Some(&marker));
            // These public panel properties reach the out-of-process Open panel.
            panel.setTitle(Some(&marker));
            panel.setPrompt(Some(&marker));
            panel.setDirectoryURL(Some(&NSURL::fileURLWithPath_isDirectory(
                &NSString::from_str(&home), true,
            )));
            *slot = Some(Drive {
                panel,
                home: home.clone(),
                step: 0,
                next: Instant::now() + Duration::from_millis(700),
                deadline,
                identifier,
                confirm_lookups: Arc::default(),
            });
        }
        let drive = slot
            .as_mut()
            .ok_or("The Home picker drive is unavailable.")?;
        if drive.home != home {
            return Err("The Home path changed during the picker drive.".into());
        }
        if Instant::now() < drive.next {
            return Ok(Progress::Waiting);
        }
        if drive.step == 1 {
            if drive.panel.isVisible() {
                return Ok(Progress::Waiting);
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
            DEADLINE.with(|deadline| *deadline.borrow_mut() = None);
            return Ok(Progress::Complete);
        }
        if !drive.panel.isVisible() {
            return Err("The NSOpenPanel closed before the picker drive finished.".into());
        }
        let expected = std::fs::canonicalize(&home).map_err(|error| error.to_string())?;
        if panel_directory(&drive.panel)
            .and_then(|path| std::fs::canonicalize(path).ok())
            .as_ref() != Some(&expected)
        {
            return Ok(Progress::Waiting);
        }
        // Mark confirmation before the AX call so concurrent polls cannot press twice.
        drive.step = 1;
        Ok(Progress::Confirm(
            drive.identifier.clone(),
            drive.deadline,
            Arc::clone(&drive.confirm_lookups),
        ))
    })
}

#[derive(serde::Serialize)]
pub(crate) struct WindowSnapshot {
    class: String,
    title: String,
    visible: bool,
    number: isize,
}

fn window_snapshot(window: &NSWindow) -> WindowSnapshot {
    WindowSnapshot {
        class: window.class().name().to_string_lossy().into_owned(),
        title: window.title().to_string(),
        visible: window.isVisible(),
        number: window.windowNumber(),
    }
}

#[derive(serde::Serialize)]
struct ActiveAppSnapshot {
    pid: i32,
    bundle_identifier: Option<String>,
    name: Option<String>,
}

#[derive(serde::Serialize)]
struct SystemPromptSnapshot {
    labels: Vec<String>,
    error: Option<String>,
}

fn system_prompt_snapshot(pid: i32) -> SystemPromptSnapshot {
    // This diagnostic only reads SecurityAgent after AppKit identifies its PID.
    // SAFETY: Create returns an owned AX element. A stale PID yields an AX error.
    let app = unsafe { CFType::wrap_under_create_rule(AXUIElementCreateApplication(pid)) };
    let deadline = Instant::now() + Duration::from_millis(400);
    let mut labels = Vec::new();
    let error = accessibility::prompt_labels(&NativeAccessibility, &app, &mut labels, || {
        if Instant::now() >= deadline {
            Err("The system prompt snapshot timed out.".into())
        } else {
            Ok(())
        }
    })
    .err();
    SystemPromptSnapshot { labels, error }
}

#[derive(serde::Serialize)]
pub(crate) struct FolderDialogSnapshot {
    windows: Vec<WindowSnapshot>,
    step: Option<u8>,
    directory: Option<String>,
    trusted: bool,
    active_app: Option<ActiveAppSnapshot>,
    app_active: bool,
    key_window: Option<WindowSnapshot>,
    system_prompt: Option<SystemPromptSnapshot>,
    confirm_lookups: Vec<ConfirmLookup>,
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
        let result: Result<FolderDialogSnapshot, String> = (|| {
            let mtm = MainThreadMarker::new().ok_or("The Home picker needs the AppKit thread.")?;
            let app = NSApplication::sharedApplication(mtm);
            let windows = app.windows();
            Ok(FolderDialogSnapshot {
                windows: (0..windows.len())
                    .map(|index| {
                        let window = windows.objectAtIndex(index);
                        window_snapshot(&window)
                    })
                    .collect(),
                step: DRIVE.with(|slot| slot.borrow().as_ref().map(|drive| drive.step)),
                directory: DRIVE.with(|slot| {
                    slot.borrow()
                        .as_ref()
                        .and_then(|drive| panel_directory(&drive.panel))
                }),
                trusted: process_trusted(),
                active_app: NSWorkspace::sharedWorkspace()
                    .frontmostApplication()
                    .map(|active| ActiveAppSnapshot {
                        pid: active.processIdentifier(),
                        bundle_identifier: active.bundleIdentifier().map(|value| value.to_string()),
                        name: active.localizedName().map(|value| value.to_string()),
                    }),
                app_active: app.isActive(),
                key_window: app.keyWindow().map(|window| window_snapshot(&window)),
                system_prompt: None,
                confirm_lookups: DRIVE.with(|slot| {
                    slot.borrow()
                        .as_ref()
                        .map(|drive| {
                            drive
                                .confirm_lookups
                                .lock()
                                .map(|lookups| lookups.clone())
                                .map_err(|_| "The Home picker lookup snapshot is unavailable.")
                        })
                        .transpose()
                        .map(Option::unwrap_or_default)
                })?,
            })
        })();
        let _ = sender.send(result);
    })
    .map_err(|error| error.to_string())?;
    let mut snapshot: FolderDialogSnapshot =
        receiver.recv().map_err(|error| error.to_string())??;
    if let Some(active) = &snapshot.active_app {
        if active.bundle_identifier.as_deref() == Some("com.apple.SecurityAgent") {
            let pid = active.pid;
            // Keep cross-process AX reads off the AppKit thread.
            snapshot.system_prompt = Some(
                tauri::async_runtime::spawn_blocking(move || system_prompt_snapshot(pid))
                    .await
                    .map_err(|error| error.to_string())?,
            );
        }
    }
    Ok(snapshot)
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
    match receiver.recv().map_err(|error| error.to_string())?? {
        Progress::Waiting => Ok(false),
        Progress::Complete => Ok(true),
        Progress::Confirm(identifier, deadline, confirm_lookups) => {
            tauri::async_runtime::spawn_blocking(move || {
                press_confirm(&identifier, deadline, confirm_lookups)
            })
            .await
            .map_err(|error| error.to_string())??;
            Ok(false)
        }
    }
}

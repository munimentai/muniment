// AppKit needs the process main thread, so this test uses its own event loop.
#[cfg(target_os = "macos")]
#[allow(dead_code)]
#[path = "../src"]
mod desktop {
    pub mod e2e_folder_dialog;
}

#[cfg(target_os = "macos")]
use desktop::e2e_folder_dialog;

#[cfg(target_os = "macos")]
fn main() {
    use block2::RcBlock;
    use objc2::{rc::autoreleasepool, MainThreadMarker};
    use objc2_app_kit::{
        NSApplication, NSApplicationActivationPolicy, NSEventMask, NSModalResponseOK, NSOpenPanel,
    };
    use objc2_foundation::{NSDate, NSDefaultRunLoopMode, NSString, NSURL};
    use std::{
        cell::RefCell,
        ffi::c_void,
        rc::Rc,
        time::{Duration, Instant},
    };

    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGSessionCopyCurrentDictionary() -> *const c_void;
    }
    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFRelease(value: *const c_void);
    }

    let session = unsafe { CGSessionCopyCurrentDictionary() };
    if session.is_null() {
        println!("SKIP folder-dialog-macos: CGSessionCopyCurrentDictionary found no window server session.");
        return;
    }
    // The Copy function transfers one retain to this caller.
    unsafe { CFRelease(session) };
    if !e2e_folder_dialog::is_trusted() {
        println!("SKIP folder-dialog-macos: AXIsProcessTrusted() returned false.");
        return;
    }

    let root = std::env::temp_dir().join(format!("muniment-picker-{}", uuid::Uuid::now_v7()));
    let home = root.join("Home space \"quote\" café #%🦀");
    std::fs::create_dir_all(&home).unwrap();
    // NSOpenPanel returns canonical paths, including /private for the macOS temporary directory.
    let home = home.canonicalize().unwrap().to_str().unwrap().to_owned();
    let outcome = std::panic::catch_unwind(|| {
        autoreleasepool(|_| {
            let mtm = MainThreadMarker::new().unwrap();
            let app = NSApplication::sharedApplication(mtm);
            assert!(app.setActivationPolicy(NSApplicationActivationPolicy::Regular));
            app.finishLaunching();
            let panel = NSOpenPanel::openPanel(mtm);
            panel.setCanChooseDirectories(true);
            panel.setCanChooseFiles(false);
            panel.setAllowsMultipleSelection(false);
            // Configure only the starting folder. The production keys must reach the chosen folder.
            panel.setDirectoryURL(Some(&NSURL::fileURLWithPath_isDirectory(
                &NSString::from_str(root.to_str().unwrap()),
                true,
            )));
            let completion = Rc::new(RefCell::new(None));
            let result = completion.clone();
            let chosen_panel = panel.clone();
            let handler = RcBlock::new(move |response| {
                let urls = chosen_panel.URLs();
                let paths: Vec<_> = (0..urls.len())
                    .map(|index| urls.objectAtIndex(index).path().unwrap().to_string())
                    .collect();
                *result.borrow_mut() = Some((response, paths));
            });
            panel.beginWithCompletionHandler(&handler);
            let deadline = Instant::now() + Duration::from_secs(15);
            let mut driven = false;
            while !driven || completion.borrow().is_none() {
                assert!(
                    Instant::now() < deadline,
                    "The NSOpenPanel completion handler did not run within 15 seconds."
                );
                // Pump real window-server events, not synthetic NSEvents dispatched inside the app.
                if let Some(event) = app.nextEventMatchingMask_untilDate_inMode_dequeue(
                    NSEventMask::Any,
                    Some(&NSDate::dateWithTimeIntervalSinceNow(0.05)),
                    unsafe { NSDefaultRunLoopMode },
                    true,
                ) {
                    app.sendEvent(&event);
                }
                app.updateWindows();
                if !driven {
                    driven = e2e_folder_dialog::poll(home.clone()).unwrap();
                }
            }
            assert_eq!(
                *completion.borrow(),
                Some((NSModalResponseOK, vec![home.clone()]))
            );
            assert!(!panel.isVisible());
        })
    });
    std::fs::remove_dir_all(root).unwrap();
    if let Err(error) = outcome {
        std::panic::resume_unwind(error);
    }
    println!("PASS folder-dialog-macos: The completion handler returned the chosen folder.");
}

#[cfg(not(target_os = "macos"))]
fn main() {
    println!("SKIP folder-dialog-macos: The test needs macOS.");
}

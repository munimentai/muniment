// AppKit requires the process main thread, so this test does not use the Rust test harness.
#[cfg(target_os = "macos")]
#[path = "../src"]
mod desktop {
    pub mod launcher;
}

#[cfg(target_os = "macos")]
use desktop::launcher;

#[cfg(target_os = "macos")]
fn panel_properties() {
    use launcher::macos;
    use objc2::{
        define_class, msg_send, rc::Retained, ClassType, MainThreadMarker, MainThreadOnly,
    };
    use objc2_app_kit::{
        NSApplication, NSApplicationActivationPolicy, NSBackingStoreType, NSFloatingWindowLevel,
        NSPanel, NSView, NSWindow, NSWindowCollectionBehavior, NSWindowStyleMask,
    };
    use objc2_foundation::{NSObjectProtocol, NSPoint, NSRect, NSSize};

    define_class!(
        #[unsafe(super(NSView))]
        #[thread_kind = MainThreadOnly]
        struct InputView;

        impl InputView {
            #[unsafe(method(acceptsFirstResponder))]
            fn accepts_first_responder(&self) -> bool { true }
        }
    );

    assert_eq!(
        launcher::position(-3840, 40, 3840, 2160, 2.0),
        tauri::PhysicalPosition::new(-2520, 680)
    );
    let mtm = MainThreadMarker::new().unwrap();
    let app = NSApplication::sharedApplication(mtm);
    assert!(app.setActivationPolicy(NSApplicationActivationPolicy::Regular));
    // These retained windows keep ownership if AppKit receives a close action.
    let main = unsafe {
        NSWindow::initWithContentRect_styleMask_backing_defer(
            NSWindow::alloc(mtm),
            NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(1100.0, 720.0)),
            NSWindowStyleMask::Titled,
            NSBackingStoreType::Buffered,
            false,
        )
    };
    unsafe { main.setReleasedWhenClosed(false) };
    let host = unsafe {
        NSWindow::initWithContentRect_styleMask_backing_defer(
            NSWindow::alloc(mtm),
            NSRect::new(NSPoint::new(300.0, 600.0), NSSize::new(600.0, 80.0)),
            NSWindowStyleMask::Borderless,
            NSBackingStoreType::Buffered,
            false,
        )
    };
    unsafe { host.setReleasedWhenClosed(false) };
    let content: Retained<InputView> =
        unsafe { msg_send![InputView::alloc(mtm), initWithFrame: host.frame()] };
    host.setContentView(Some(&content));
    assert!(host.makeFirstResponder(Some(&content)));
    let panel = macos::LauncherPanel::attach(&host, mtm);
    assert!(panel.isKindOfClass(NSPanel::class()));
    assert!(panel
        .styleMask()
        .contains(NSWindowStyleMask::NonactivatingPanel));
    assert!(panel.collectionBehavior().contains(
        NSWindowCollectionBehavior::CanJoinAllSpaces
            | NSWindowCollectionBehavior::FullScreenAuxiliary
    ));
    assert_eq!(panel.level(), NSFloatingWindowLevel);
    assert!(panel.isFloatingPanel());
    assert!(panel.canBecomeKeyWindow());
    assert!(!panel.canBecomeMainWindow());
    assert!(!panel.hidesOnDeactivate());
    assert!(!panel.becomesKeyOnlyIfNeeded());
    assert_eq!(panel.contentView().as_deref(), Some(content.as_super()));
    assert_eq!(
        panel.firstResponder().as_deref(),
        Some(content.as_super().as_super())
    );
    assert!(!panel.isReleasedWhenClosed());
    assert!(!host.isVisible());
    assert!(!panel.isVisible());
    assert!(host.contentView().is_some());
    assert_ne!(host.contentView(), panel.contentView());
    // Exercise first-open and reopen frames without a queued host move or a second physical screen.
    for (area, origin) in [
        (
            NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(1920.0, 1080.0)),
            NSPoint::new(660.0, 680.0),
        ),
        (
            NSRect::new(NSPoint::new(-1920.0, 200.0), NSSize::new(1920.0, 1080.0)),
            NSPoint::new(-1260.0, 880.0),
        ),
        (
            NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(500.0, 100.0)),
            NSPoint::new(0.0, 20.0),
        ),
    ] {
        panel.show(area);
        assert!(panel.isVisible());
        assert_eq!(panel.frame(), NSRect::new(origin, NSSize::new(600.0, 80.0)));
        panel.hide();
        assert!(!panel.isVisible());
    }
    assert_eq!(
        app.activationPolicy(),
        NSApplicationActivationPolicy::Regular
    );
    assert!(!main.isKindOfClass(NSPanel::class()));
    assert_eq!(main.styleMask(), NSWindowStyleMask::Titled);
    assert!(main.canBecomeMainWindow());
    assert!(!host.isVisible());
}

#[cfg(target_os = "macos")]
fn main() {
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSEvent, NSScreen, NSWindow};
    use objc2_foundation::NSPoint;
    use tauri::Manager;

    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGWarpMouseCursorPosition(point: NSPoint) -> i32;
    }

    let mut context = tauri::test::mock_context(tauri::test::noop_assets());
    let config: tauri::utils::config::Config =
        serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
    context.config_mut().app.windows = config.app.windows;
    // The context supplies empty assets, but the builder creates real Wry, Tao, and AppKit windows.
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            launcher::launcher_open,
            launcher::launcher_close,
            launcher::launcher_is_visible,
            launcher::launcher_present_main,
            launcher::launcher_register,
        ])
        .on_window_event(launcher::window_event)
        .setup(|app| {
            panel_properties();
            let mtm = MainThreadMarker::new().unwrap();
            let window = app.get_webview_window("launcher").unwrap();
            let host_ptr = window.ns_window().unwrap();
            let host = unsafe { &*host_ptr.cast::<NSWindow>() };
            let content = host.contentView().unwrap();
            launcher::setup(app.handle()).unwrap();
            // Both accessors must still resolve the hidden Tao host after the content moves.
            assert_eq!(window.ns_window().unwrap(), host_ptr);
            let view = window.ns_view().unwrap();
            assert!(!view.is_null());
            assert_ne!(
                view,
                objc2::rc::Retained::as_ptr(&content).cast_mut().cast()
            );
            let panel = content.window().unwrap();
            assert_ne!(&*panel, host);
            assert!(!panel.isVisible());

            // Run inside the main-thread callback, where Tauri dispatches inline and Tao queues moves.
            launcher::launcher_open(app.handle().clone()).unwrap();
            assert!(launcher::launcher_is_visible(app.handle().clone()).unwrap());
            assert!(!host.isVisible());
            let area = launcher::macos::work_area(mtm).unwrap();
            assert_panel_position(&panel, area);
            launcher::launcher_close(app.handle().clone()).unwrap();
            assert!(!launcher::launcher_is_visible(app.handle().clone()).unwrap());

            let screens = NSScreen::screens(mtm);
            let primary_top = screens.objectAtIndex(0).frame().size.height;
            let original_cursor = NSEvent::mouseLocation();
            // Reopen on each connected monitor, including monitors with a different scale or origin.
            for index in 0..screens.count() {
                let area = screens.objectAtIndex(index).visibleFrame();
                let cursor = NSPoint::new(
                    area.origin.x + area.size.width / 2.0,
                    primary_top - (area.origin.y + area.size.height / 2.0),
                );
                assert_eq!(unsafe { CGWarpMouseCursorPosition(cursor) }, 0);
                launcher::launcher_open(app.handle().clone()).unwrap();
                assert!(panel.isVisible());
                assert_panel_position(&panel, area);
                assert_eq!(window.ns_window().unwrap(), host_ptr);
                launcher::launcher_close(app.handle().clone()).unwrap();
                assert!(!panel.isVisible());
                assert!(!host.isVisible());
            }
            assert_eq!(
                unsafe {
                    CGWarpMouseCursorPosition(NSPoint::new(
                        original_cursor.x,
                        primary_top - original_cursor.y,
                    ))
                },
                0
            );
            Ok(())
        })
        .build(context)
        .unwrap()
        .run(|app, event| {
            if matches!(event, tauri::RunEvent::Ready) {
                app.exit(0);
            }
        });
}

#[cfg(target_os = "macos")]
fn assert_panel_position(panel: &objc2_app_kit::NSWindow, area: objc2_foundation::NSRect) {
    let frame = panel.frame();
    assert_eq!(frame.size, objc2_foundation::NSSize::new(600.0, 80.0));
    assert!((frame.origin.x + 300.0 - (area.origin.x + area.size.width / 2.0)).abs() < 1.0);
    assert!((frame.origin.y + 40.0 - (area.origin.y + area.size.height * 2.0 / 3.0)).abs() < 1.0);
}

#[cfg(not(target_os = "macos"))]
fn main() {}

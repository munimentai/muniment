// AppKit requires the process main thread, so this test does not use the Rust test harness.
#[cfg(target_os = "macos")]
#[path = "../src/launcher/macos.rs"]
mod macos;

#[cfg(target_os = "macos")]
fn main() {
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
    for _ in 0..2 {
        panel.show(&host);
        assert!(panel.isVisible());
        assert_eq!(panel.frame(), host.frame());
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

#[cfg(not(target_os = "macos"))]
fn main() {}

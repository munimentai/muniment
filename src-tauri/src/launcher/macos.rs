use objc2::{define_class, msg_send, rc::Retained, ClassType, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSBackingStoreType, NSEvent, NSFloatingWindowLevel, NSPanel, NSScreen, NSView, NSWindow,
    NSWindowCollectionBehavior, NSWindowStyleMask,
};
use objc2_foundation::{NSObjectProtocol, NSPoint, NSRect, NSSize};

pub fn work_area(mtm: MainThreadMarker) -> Option<NSRect> {
    let cursor = NSEvent::mouseLocation();
    let screens = NSScreen::screens(mtm);
    (0..screens.count())
        .map(|index| screens.objectAtIndex(index))
        .find(|screen| {
            let frame = screen.frame();
            cursor.x >= frame.origin.x
                && cursor.x < frame.origin.x + frame.size.width
                && cursor.y >= frame.origin.y
                && cursor.y < frame.origin.y + frame.size.height
        })
        .or_else(|| screens.firstObject())
        .map(|screen| screen.visibleFrame())
}

define_class!(
    #[unsafe(super(NSPanel))]
    #[thread_kind = MainThreadOnly]
    pub struct LauncherPanel;

    impl LauncherPanel {
        #[unsafe(method(canBecomeKeyWindow))]
        fn can_become_key_window(&self) -> bool { true }

        #[unsafe(method(canBecomeMainWindow))]
        fn can_become_main_window(&self) -> bool { false }
    }
);

impl LauncherPanel {
    pub fn attach(host: &NSWindow, mtm: MainThreadMarker) -> Retained<Self> {
        // Allocate a real panel. Changing Tao's window class would discard its ivars and methods.
        let panel: Retained<Self> = unsafe {
            msg_send![Self::alloc(mtm),
                initWithContentRect: host.frame(),
                styleMask: NSWindowStyleMask::Borderless | NSWindowStyleMask::NonactivatingPanel,
                backing: NSBackingStoreType::Buffered,
                defer: false]
        };
        // The retained panel owns its lifetime, even if AppKit receives a close action.
        unsafe { panel.setReleasedWhenClosed(false) };
        panel.setLevel(NSFloatingWindowLevel);
        panel.setFloatingPanel(true);
        panel.setHidesOnDeactivate(false);
        panel.setBecomesKeyOnlyIfNeeded(false);
        panel.setCollectionBehavior(
            NSWindowCollectionBehavior::CanJoinAllSpaces
                | NSWindowCollectionBehavior::FullScreenAuxiliary,
        );
        // Tauri retains the hidden host and webview. Only the launcher content moves into the panel.
        let content = host.contentView();
        let responder = host.firstResponder();
        // Tao resolves native handles through the host content view, even while the panel owns the webview.
        let placeholder = NSView::initWithFrame(
            NSView::alloc(mtm),
            NSRect::new(NSPoint::new(0.0, 0.0), host.frame().size),
        );
        host.setContentView(Some(&placeholder));
        panel.setContentView(content.as_deref());
        if let Some(responder) =
            responder.filter(|responder| responder.isKindOfClass(NSView::class()))
        {
            panel.makeFirstResponder(Some(&responder));
        }
        panel
    }

    pub fn show(&self, area: NSRect) {
        // AppKit uses logical points and a bottom-left origin on every screen, including mixed-scale screens.
        let frame = NSRect::new(
            NSPoint::new(
                area.origin.x + ((area.size.width - 600.0) / 2.0).max(0.0),
                area.origin.y + area.size.height - (area.size.height / 3.0 - 40.0).max(0.0) - 80.0,
            ),
            NSSize::new(600.0, 80.0),
        );
        // Move the panel synchronously. Tao queues host moves after the shortcut callback returns.
        self.setFrame_display(frame, true);
        self.orderFrontRegardless();
        self.makeKeyWindow();
    }

    pub fn hide(&self) {
        self.orderOut(None);
    }
}

use objc2::{define_class, msg_send, rc::Retained, ClassType, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSBackingStoreType, NSFloatingWindowLevel, NSPanel, NSView, NSWindow,
    NSWindowCollectionBehavior, NSWindowStyleMask,
};
use objc2_foundation::NSObjectProtocol;

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
        host.setContentView(None);
        panel.setContentView(content.as_deref());
        if let Some(responder) =
            responder.filter(|responder| responder.isKindOfClass(NSView::class()))
        {
            panel.makeFirstResponder(Some(&responder));
        }
        panel
    }

    pub fn show(&self, host: &NSWindow) {
        self.setFrame_display(host.frame(), true);
        self.orderFrontRegardless();
        self.makeKeyWindow();
    }

    pub fn hide(&self) {
        self.orderOut(None);
    }
}

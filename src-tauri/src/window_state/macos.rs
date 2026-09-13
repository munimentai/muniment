use std::cell::{Cell, RefCell};

use objc2::{
    define_class, msg_send, rc::Retained, sel, DefinedClass, MainThreadMarker, MainThreadOnly,
};
use objc2_app_kit::{
    NSButton, NSView, NSViewFrameDidChangeNotification, NSWindow, NSWindowButton,
    NSWindowDidExitFullScreenNotification, NSWindowDidResizeNotification, NSWindowStyleMask,
};
use objc2_foundation::{NSNotification, NSNotificationCenter, NSObject, NSPoint, NSRect};

thread_local! {
    static TRAFFIC_LIGHTS: RefCell<Option<Retained<TrafficLights>>> = const { RefCell::new(None) };
}

struct Insets {
    window: Retained<NSWindow>,
    buttons: Vec<Retained<NSButton>>,
    x: f64,
    y: f64,
    spacing: f64,
    applying: Cell<bool>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[ivars = Insets]
    struct TrafficLights;

    impl TrafficLights {
        #[unsafe(method(layoutChanged:))]
        fn layout_changed(&self, _notification: &NSNotification) {
            self.apply();
        }
    }
);

impl TrafficLights {
    fn apply(&self) {
        let state = self.ivars();
        // AppKit owns the traffic lights in full screen. Frame notifications can nest.
        if state
            .window
            .styleMask()
            .contains(NSWindowStyleMask::FullScreen)
            || state.applying.replace(true)
        {
            return;
        }
        for (index, button) in state.buttons.iter().enumerate() {
            // AppKit can detach the buttons during a full screen transition.
            if button
                .window()
                .as_deref()
                .is_none_or(|window| !std::ptr::eq(window, &*state.window))
            {
                continue;
            }
            // AppKit keeps the view hierarchy on this thread.
            let Some(parent) = (unsafe { button.superview() }) else {
                continue;
            };
            let frame = button.frame();
            // Convert the configured top-left inset to the button parent's coordinate system.
            // This also handles Wry resizing the title bar container after a hidden window shows.
            let target = NSRect::new(
                NSPoint::new(
                    state.x + index as f64 * state.spacing,
                    state.window.frame().size.height - state.y - frame.size.height,
                ),
                frame.size,
            );
            let origin = parent.convertRect_fromView(target, None).origin;
            if frame.origin != origin {
                button.setFrameOrigin(origin);
            }
        }
        state.applying.set(false);
    }
}

impl Drop for TrafficLights {
    fn drop(&mut self) {
        // The observer and all callbacks live on the AppKit thread.
        unsafe { NSNotificationCenter::defaultCenter().removeObserver(self) };
    }
}

pub(super) fn install<R: tauri::Runtime>(
    window: &tauri::WebviewWindow<R>,
    config: &tauri::utils::config::WindowConfig,
) -> tauri::Result<()> {
    let Some(inset) = config.traffic_light_position.as_ref() else {
        return Ok(());
    };
    let mtm = MainThreadMarker::new().expect("The title bar needs the main thread.");
    let host = window.ns_window()?;
    // Tauri owns the native window. Setup runs on the AppKit thread.
    let host = unsafe { &*host.cast::<NSWindow>() };
    let buttons: Vec<_> = [
        NSWindowButton::CloseButton,
        NSWindowButton::MiniaturizeButton,
        NSWindowButton::ZoomButton,
    ]
    .into_iter()
    .filter_map(|kind| host.standardWindowButton(kind))
    .collect();
    if buttons.len() != 3 {
        return Ok(());
    }
    let spacing = buttons[1].frame().origin.x - buttons[0].frame().origin.x;
    let mut views: Vec<Retained<NSView>> = buttons
        .iter()
        .cloned()
        .map(Retained::into_super)
        .map(Retained::into_super)
        .collect();
    // Setup reads the native view hierarchy on the AppKit thread.
    if let Some(parent) = unsafe { buttons[0].superview() } {
        if let Some(container) = unsafe { parent.superview() } {
            views.push(container);
        }
        views.push(parent);
    }
    let observer = TrafficLights::alloc(mtm).set_ivars(Insets {
        window: buttons[0]
            .window()
            .expect("The traffic lights need a window."),
        buttons,
        x: inset.x,
        y: inset.y,
        spacing,
        applying: Cell::new(false),
    });
    let observer: Retained<TrafficLights> = unsafe { msg_send![super(observer), init] };
    let center = NSNotificationCenter::defaultCenter();
    for view in &views {
        view.setPostsFrameChangedNotifications(true);
        // The selector accepts one notification. The retained observer outlives its registration.
        unsafe {
            center.addObserver_selector_name_object(
                &observer,
                sel!(layoutChanged:),
                Some(NSViewFrameDidChangeNotification),
                Some(view),
            );
        }
    }
    for name in unsafe {
        [
            NSWindowDidResizeNotification,
            NSWindowDidExitFullScreenNotification,
        ]
    } {
        unsafe {
            center.addObserver_selector_name_object(
                &observer,
                sel!(layoutChanged:),
                Some(name),
                Some(host),
            );
        }
    }
    observer.apply();
    TRAFFIC_LIGHTS.with(|slot| *slot.borrow_mut() = Some(observer));
    Ok(())
}

pub(super) fn apply() {
    TRAFFIC_LIGHTS.with(|slot| {
        if let Some(observer) = slot.borrow().as_ref() {
            observer.apply();
        }
    });
}

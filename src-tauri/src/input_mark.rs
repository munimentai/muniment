//! macOS draws its input-source mark beside the caret of the focused text
//! field, in a child window of the key window. When page focus moves from a
//! text field to an element that takes no text, WebKit drops its input
//! context and never tells AppKit to hide the mark, so the mark stays over
//! the new page at the old caret. The UI calls this command after a page
//! change or a closed panel leaves focus off any text field.
//!
//! The terminal draws its prompt where AppKit draws the mark, and AppKit
//! shows the mark again up to two seconds after the caret moves. So while the
//! terminal holds focus, each focus, keystroke or output asks the shell to
//! watch: a poller hides the mark every [`WATCH_PERIOD`] on the main thread
//! until [`WATCH_WINDOW`] after the last request, then stops.
//!
//! The same AppKit controller also draws the Writing Tools affordance. When
//! the pointer passes over it, the affordance grows into a 280 by 168 point
//! child window of the key window that hosts a remote view from another
//! process. A click there lands in that window, not on the page, and the app
//! can lose activation. The composer keeps its caret while a reply streams,
//! so the affordance can sit over Stop. [`refuse_affordance`] turns it off
//! for every web view at setup, and Writing Tools stays in the context menu.
//! [`log_focus_changes`] writes each lost activation and each new key window
//! to the runtime log, so a live check names a window that takes the click.

use std::sync::Mutex;
use std::time::{Duration, Instant};

/// How often the watch checks for the mark. A mark that appears lasts at
/// most this long.
pub const WATCH_PERIOD: Duration = Duration::from_millis(50);
/// How long the watch lasts after the last request.
pub const WATCH_WINDOW: Duration = Duration::from_millis(3000);

/// The watch window: the time it ends, and whether a poller runs.
#[derive(Debug, Default)]
struct Watch {
    until: Option<Instant>,
    polling: bool,
}

impl Watch {
    /// Extends the window to `WATCH_WINDOW` after `now`. Returns true when
    /// the caller must start a poller, because none runs.
    fn request(&mut self, now: Instant) -> bool {
        self.until = Some(now + WATCH_WINDOW);
        !std::mem::replace(&mut self.polling, true)
    }

    /// Ends the window. A running poller stops at its next tick.
    fn stop(&mut self) {
        self.until = None;
    }

    /// The poller asks this at each tick. True means check the mark now.
    /// False means the window ended and the poller exits.
    fn tick(&mut self, now: Instant) -> bool {
        let open = self.until.is_some_and(|until| now < until);
        if !open {
            self.until = None;
            self.polling = false;
        }
        open
    }
}

static WATCH: Mutex<Watch> = Mutex::new(Watch {
    until: None,
    polling: false,
});

fn watch() -> std::sync::MutexGuard<'static, Watch> {
    WATCH.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Hides the mark every [`WATCH_PERIOD`] until [`WATCH_WINDOW`] after this
/// call. A later call extends the window, and one poller serves them all.
#[tauri::command]
pub fn input_mark_watch(app: tauri::AppHandle) {
    // Only macOS draws the mark.
    if !cfg!(target_os = "macos") || !watch().request(Instant::now()) {
        return;
    }
    let started = std::thread::Builder::new()
        .name("input-mark-watch".into())
        .spawn(move || loop {
            std::thread::sleep(WATCH_PERIOD);
            if !watch().tick(Instant::now()) {
                return;
            }
            let _ = app.run_on_main_thread(dismiss_on_main);
        });
    if started.is_err() {
        watch().polling = false;
    }
}

/// Ends the watch, as when the terminal loses focus or closes.
#[tauri::command]
pub fn input_mark_unwatch() {
    watch().stop();
}

fn dismiss_on_main() {
    #[cfg(target_os = "macos")]
    macos::dismiss();
}

/// Hides the system's input-source mark. The mark returns at the caret when
/// a text field takes focus again.
#[tauri::command]
pub fn input_mark_dismiss() {
    #[cfg(target_os = "macos")]
    macos::dismiss();
}

/// Turns off the Writing Tools affordance in every web view of the app.
/// Setup calls this after Tauri creates the configured windows.
pub fn refuse_affordance() {
    #[cfg(target_os = "macos")]
    if !macos::refuse_affordance(macos::WEB_VIEW_CLASS) {
        eprintln!("The Writing Tools affordance stays on: the web view class is missing.");
    }
}

/// Writes each lost activation and each new key window to the runtime log.
pub fn log_focus_changes() {
    #[cfg(target_os = "macos")]
    macos::log_focus_changes();
}

/// One window in a focus record.
#[cfg_attr(not(any(target_os = "macos", test)), allow(dead_code))]
#[derive(Debug, Clone, PartialEq)]
struct WindowFacts {
    class: String,
    title: String,
    width: f64,
    height: f64,
    level: isize,
    key: bool,
    can_become_key: bool,
    child: bool,
}

/// The runtime log line for a focus change: `resign_active` names the
/// visible windows front to back, and `key_window` names the new key window.
#[cfg_attr(not(any(target_os = "macos", test)), allow(dead_code))]
fn focus_record(change: &str, windows: &[WindowFacts]) -> String {
    let mut record = format!("event=desktop_focus change={change}");
    for (index, window) in windows.iter().enumerate() {
        record.push_str(&format!(
            " window{index}={{class={} title={} size={}x{} level={} key={} can_become_key={} child={}}}",
            window.class,
            serde_json::to_string(&window.title).unwrap_or_default(),
            window.width.round(),
            window.height.round(),
            window.level,
            window.key,
            window.can_become_key,
            window.child,
        ));
    }
    record.push('\n');
    record
}

/// A resign record names at most this many windows, front to back.
#[cfg_attr(not(any(target_os = "macos", test)), allow(dead_code))]
const RECORDED_WINDOWS: usize = 4;

#[cfg(target_os = "macos")]
mod macos {
    use std::ffi::CStr;

    use objc2::rc::Retained;
    use objc2::runtime::{AnyClass, AnyObject, Bool, Imp, Sel};
    use objc2::{define_class, ffi, msg_send, sel, MainThreadMarker, MainThreadOnly};
    use objc2_app_kit::{
        NSApplication, NSApplicationDidResignActiveNotification, NSWindow,
        NSWindowDidBecomeKeyNotification,
    };
    use objc2_foundation::{NSArray, NSNotification, NSNotificationCenter, NSObject};

    use super::{focus_record, WindowFacts, RECORDED_WINDOWS};

    /// Wry draws each Tauri page in a web view of this class.
    pub(super) const WEB_VIEW_CLASS: &CStr = c"WryWebView";

    unsafe extern "C-unwind" fn refuse(_this: *mut AnyObject, _cmd: Sel) -> Bool {
        Bool::NO
    }

    /// Makes instances of `class_name` answer NO to
    /// `allowsWritingToolsAffordance`, which AppKit asks before it shows the
    /// affordance. The method goes on the subclass, so the parent class keeps
    /// its own answer. Returns false when the class is missing.
    pub(super) fn refuse_affordance(class_name: &CStr) -> bool {
        let Some(class) = AnyClass::get(class_name) else {
            return false;
        };
        // SAFETY: `refuse` matches the `B@:` encoding: a receiver, a
        // selector, and a BOOL result. class_addMethod leaves a method the
        // class already defines, as after a second call.
        unsafe {
            let imp = std::mem::transmute::<
                unsafe extern "C-unwind" fn(*mut AnyObject, Sel) -> Bool,
                Imp,
            >(refuse);
            let _ = ffi::class_addMethod(
                class as *const AnyClass as *mut AnyClass,
                sel!(allowsWritingToolsAffordance),
                imp,
                c"B@:".as_ptr(),
            );
        }
        true
    }

    fn facts(window: &NSWindow) -> WindowFacts {
        let frame = window.frame();
        WindowFacts {
            class: window.class().name().to_string_lossy().into_owned(),
            title: window.title().to_string(),
            width: frame.size.width,
            height: frame.size.height,
            level: window.level(),
            key: window.isKeyWindow(),
            can_become_key: window.canBecomeKeyWindow(),
            child: window.parentWindow().is_some(),
        }
    }

    fn record(line: &str) {
        let Some(home) = muniment_core::user_diagnostics::effective_user_home().ok() else {
            return;
        };
        let _ = crate::macos_runtime_service::write_child_diagnostic(
            &home.join("Library/Logs/Muniment"),
            line,
        );
    }

    define_class!(
        #[unsafe(super(NSObject))]
        #[thread_kind = MainThreadOnly]
        struct FocusLog;

        impl FocusLog {
            #[unsafe(method(appResignedActive:))]
            fn app_resigned_active(&self, _notification: &NSNotification) {
                let app = NSApplication::sharedApplication(self.mtm());
                // SAFETY: orderedWindows takes no argument and returns the
                // app's windows front to back.
                let windows: Retained<NSArray<NSWindow>> =
                    unsafe { msg_send![&*app, orderedWindows] };
                let visible: Vec<_> = windows
                    .iter()
                    .filter(|window| window.isVisible())
                    .take(RECORDED_WINDOWS)
                    .map(|window| facts(&window))
                    .collect();
                record(&focus_record("resign_active", &visible));
            }

            #[unsafe(method(windowBecameKey:))]
            fn window_became_key(&self, notification: &NSNotification) {
                let Some(window) = notification
                    .object()
                    .and_then(|object| object.downcast::<NSWindow>().ok())
                else {
                    return;
                };
                record(&focus_record("key_window", &[facts(&window)]));
            }
        }
    );

    thread_local! {
        static FOCUS_LOG: std::cell::RefCell<Option<Retained<FocusLog>>> =
            const { std::cell::RefCell::new(None) };
    }

    pub(super) fn log_focus_changes() {
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        if FOCUS_LOG.with(|slot| slot.borrow().is_some()) {
            return;
        }
        let observer = FocusLog::alloc(mtm).set_ivars(());
        let observer: Retained<FocusLog> = unsafe { msg_send![super(observer), init] };
        let center = NSNotificationCenter::defaultCenter();
        // SAFETY: each selector takes one notification, and the retained
        // observer lives as long as the app.
        unsafe {
            center.addObserver_selector_name_object(
                &observer,
                sel!(appResignedActive:),
                Some(NSApplicationDidResignActiveNotification),
                None,
            );
            center.addObserver_selector_name_object(
                &observer,
                sel!(windowBecameKey:),
                Some(NSWindowDidBecomeKeyNotification),
                None,
            );
        }
        FOCUS_LOG.with(|slot| *slot.borrow_mut() = Some(observer));
    }

    /// AppKit's lightweight text UI controller owns the mark, and only its
    /// `dismiss` hides it: deactivating the input context or moving the first
    /// responder leaves it on screen. A macOS without that controller draws
    /// no such mark, so the call does nothing there.
    pub(super) fn dismiss() {
        if objc2::MainThreadMarker::new().is_none() {
            return;
        }
        let Some(class) = AnyClass::get(c"NSCampoLightweightUIController") else {
            return;
        };
        let meta = class.metaclass();
        if !meta.responds_to(sel!(isEligible))
            || !meta.responds_to(sel!(sharedInstance))
            || !class.responds_to(sel!(isVisible))
            || !class.responds_to(sel!(dismiss))
        {
            return;
        }
        // SAFETY: the selectors exist on this class, as checked above, and
        // take no arguments. AppKit calls run on the main thread, as checked.
        unsafe {
            let eligible: bool = msg_send![class, isEligible];
            if !eligible {
                return;
            }
            let shared: Option<Retained<AnyObject>> = msg_send![class, sharedInstance];
            let Some(shared) = shared else {
                return;
            };
            let visible: bool = msg_send![&*shared, isVisible];
            if visible {
                let _: () = msg_send![&*shared, dismiss];
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_request_starts_one_poller_that_checks_until_the_window_ends() {
        let start = Instant::now();
        let mut watch = Watch::default();
        assert!(watch.request(start));
        // A second request while the poller runs starts no other poller.
        assert!(!watch.request(start + Duration::from_millis(10)));
        assert!(watch.tick(start + WATCH_PERIOD));
        assert!(watch.tick(start + Duration::from_millis(3000)));
        assert!(!watch.tick(start + Duration::from_millis(3010)));
        assert!(!watch.polling);
        // After the poller exits, the next request starts one again.
        assert!(watch.request(start + Duration::from_millis(4000)));
    }

    #[test]
    fn each_request_extends_the_window_from_its_own_time() {
        let start = Instant::now();
        let mut watch = Watch::default();
        watch.request(start);
        watch.request(start + Duration::from_millis(2500));
        assert!(watch.tick(start + Duration::from_millis(5000)));
        assert!(watch.tick(start + Duration::from_millis(5499)));
        assert!(!watch.tick(start + Duration::from_millis(5500)));
    }

    #[test]
    fn a_stop_ends_the_window_at_the_next_tick() {
        let start = Instant::now();
        let mut watch = Watch::default();
        watch.request(start);
        watch.stop();
        assert!(watch.polling);
        assert!(!watch.tick(start + WATCH_PERIOD));
        assert!(!watch.polling);
        // A request before the poller's next tick keeps that poller.
        let mut watch = Watch::default();
        watch.request(start);
        watch.stop();
        assert!(!watch.request(start + Duration::from_millis(20)));
        assert!(watch.tick(start + WATCH_PERIOD));
    }

    #[test]
    fn a_focus_record_names_each_window_front_to_back() {
        let affordance = WindowFacts {
            class: "NSCampoLightweightUIHostWindow".into(),
            title: String::new(),
            width: 280.0,
            height: 168.0,
            level: 0,
            key: false,
            can_become_key: true,
            child: true,
        };
        let main = WindowFacts {
            class: "TaoWindow".into(),
            title: "New \"thread\"".into(),
            width: 1280.0,
            height: 800.4,
            level: 0,
            key: true,
            can_become_key: true,
            child: false,
        };
        assert_eq!(
            focus_record("resign_active", &[affordance, main]),
            "event=desktop_focus change=resign_active \
             window0={class=NSCampoLightweightUIHostWindow title=\"\" size=280x168 level=0 key=false can_become_key=true child=true} \
             window1={class=TaoWindow title=\"New \\\"thread\\\"\" size=1280x800 level=0 key=true can_become_key=true child=false}\n"
        );
        assert_eq!(
            focus_record("resign_active", &[]),
            "event=desktop_focus change=resign_active\n"
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn the_refusal_turns_off_the_affordance_on_the_subclass_alone() {
        use objc2::runtime::{AnyClass, AnyObject, Bool, ClassBuilder, NSObject, Sel};
        use objc2::{msg_send, sel, ClassType};

        extern "C-unwind" fn allow(_this: &AnyObject, _cmd: Sel) -> Bool {
            Bool::YES
        }
        // The parent stands for WKWebView, which answers YES, and the child
        // for Wry's subclass.
        let mut parent =
            ClassBuilder::new(c"MunimentAffordanceParent", NSObject::class()).unwrap();
        unsafe {
            parent.add_method(
                sel!(allowsWritingToolsAffordance),
                allow as extern "C-unwind" fn(_, _) -> _,
            )
        };
        let parent = parent.register();
        ClassBuilder::new(c"MunimentAffordanceChild", parent)
            .unwrap()
            .register();
        let answer = |class: &AnyClass| -> bool {
            let object: objc2::rc::Retained<AnyObject> =
                unsafe { msg_send![msg_send![class, alloc], init] };
            unsafe { msg_send![&*object, allowsWritingToolsAffordance] }
        };
        let child = AnyClass::get(c"MunimentAffordanceChild").unwrap();
        assert!(answer(child));
        assert!(macos::refuse_affordance(c"MunimentAffordanceChild"));
        assert!(!answer(child));
        assert!(answer(parent));
        // A second call keeps the refusal, and a missing class reports false.
        assert!(macos::refuse_affordance(c"MunimentAffordanceChild"));
        assert!(!answer(child));
        assert!(!macos::refuse_affordance(c"MunimentNoSuchWebView"));
    }

    #[test]
    fn the_watch_hides_a_mark_within_a_tenth_of_a_second() {
        assert!(WATCH_PERIOD <= Duration::from_millis(100));
        assert_eq!(WATCH_WINDOW, Duration::from_secs(3));
    }
}

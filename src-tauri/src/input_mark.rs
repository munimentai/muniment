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

#[cfg(target_os = "macos")]
mod macos {
    use objc2::rc::Retained;
    use objc2::runtime::{AnyClass, AnyObject};
    use objc2::{msg_send, sel};

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
    fn the_watch_hides_a_mark_within_a_tenth_of_a_second() {
        assert!(WATCH_PERIOD <= Duration::from_millis(100));
        assert_eq!(WATCH_WINDOW, Duration::from_secs(3));
    }
}

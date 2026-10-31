//! macOS draws its input-source mark beside the caret of the focused text
//! field, in a child window of the key window. When page focus moves from a
//! text field to an element that takes no text, WebKit drops its input
//! context and never tells AppKit to hide the mark, so the mark stays over
//! the new page at the old caret. The UI calls this command after a page
//! change or a closed panel leaves focus off any text field.

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

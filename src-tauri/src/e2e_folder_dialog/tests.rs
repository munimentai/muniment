use super::*;
use std::cell::{Cell, RefCell};

struct Panel {
    directory: RefCell<Option<String>>,
    visible: Cell<bool>,
    focused: Cell<bool>,
    trusted: Cell<bool>,
    keys: RefCell<Vec<(u8, String)>>,
    selected: RefCell<Vec<String>>,
    key_error: Cell<bool>,
}

impl Panel {
    fn new() -> Self {
        Self {
            directory: RefCell::new(Some("/start".into())),
            visible: Cell::new(true),
            focused: Cell::new(true),
            trusted: Cell::new(true),
            keys: RefCell::new(Vec::new()),
            selected: RefCell::new(Vec::new()),
            key_error: Cell::new(false),
        }
    }
}

impl FolderPanel for Panel {
    fn directory(&self) -> Option<String> {
        self.directory.borrow().clone()
    }
    fn is_visible(&self) -> bool {
        self.visible.get()
    }
    fn is_focused(&self) -> bool {
        self.focused.get()
    }
    fn is_trusted(&self) -> bool {
        self.trusted.get()
    }
    fn send_step(&self, step: u8, home: &str) -> Result<(), String> {
        self.keys.borrow_mut().push((step, home.into()));
        if self.key_error.get() {
            Err("CGEvent allocation failed.".into())
        } else {
            Ok(())
        }
    }
    fn selected_paths(&self) -> Vec<String> {
        self.selected.borrow().clone()
    }
}

fn navigate(drive: &mut Drive<Panel>, home: &str, now: Instant) {
    for step in 0..3 {
        assert!(!drive.poll(home, now + SETTLE * step).unwrap());
    }
}

#[test]
fn keeps_four_steps_in_order_and_waits_for_navigation_and_close() {
    for home in ["/", "/tmp/Home space \"quote\" café #%🦀"] {
        let now = Instant::now();
        let mut drive = Drive::new(Panel::new(), home.into(), now);
        navigate(&mut drive, home, now);
        assert!(!drive.poll(home, now + SETTLE * 3).unwrap());
        assert_eq!(drive.panel.keys.borrow().len(), 3);
        *drive.panel.directory.borrow_mut() = Some(home.into());
        assert!(!drive.poll(home, now + SETTLE * 3).unwrap());
        assert!(!drive.poll(home, now + SETTLE * 4).unwrap());
        *drive.panel.selected.borrow_mut() = vec![home.into()];
        drive.panel.visible.set(false);
        assert!(drive.poll(home, now + SETTLE * 4).unwrap());
        assert!(drive.poll(home, now + SETTLE * 5).unwrap());
        assert_eq!(
            *drive.panel.keys.borrow(),
            (0..4).map(|step| (step, home.into())).collect::<Vec<_>>()
        );
    }
}

#[test]
fn missing_trust_fails_before_the_first_key_even_without_focus() {
    let now = Instant::now();
    let mut drive = Drive::new(Panel::new(), "/home".into(), now);
    drive.panel.trusted.set(false);
    drive.panel.focused.set(false);
    let error = drive.poll("/home", now).unwrap_err();
    assert!(error.contains("AXIsProcessTrusted() returned false"));
    assert!(error.contains("step 0 (Command Shift G)"));
    assert!(error.contains("The panel directory is \"/start\""));
    drive.panel.trusted.set(true);
    assert_eq!(drive.poll("/home", now + STEP_WAIT), Err(error));
    assert!(drive.panel.keys.borrow().is_empty());
}

#[test]
fn stuck_or_missing_directory_stops_before_the_outer_timeout_without_accepting() {
    for directory in [Some("/start".into()), None] {
        let now = Instant::now();
        let mut drive = Drive::new(Panel::new(), "/home".into(), now);
        navigate(&mut drive, "/home", now);
        *drive.panel.directory.borrow_mut() = directory.clone();
        let deadline = now + SETTLE * 2 + STEP_WAIT;
        assert!(!drive
            .poll("/home", deadline - Duration::from_millis(1))
            .unwrap());
        let error = drive.poll("/home", deadline).unwrap_err();
        assert!(error.contains("step 3 (Return to choose Home)"));
        assert!(error.contains(&format!(
            "The panel directory is {:?}",
            directory.unwrap_or_else(|| "unavailable".into())
        )));
        assert!(deadline - now < Duration::from_secs(30));
        assert_eq!(drive.poll("/home", deadline + STEP_WAIT), Err(error));
        assert_eq!(drive.panel.keys.borrow().len(), 3);
    }
}

#[test]
fn stuck_close_names_the_final_step_and_directory() {
    let now = Instant::now();
    let mut drive = Drive::new(Panel::new(), "/home".into(), now);
    navigate(&mut drive, "/home", now);
    *drive.panel.directory.borrow_mut() = Some("/home".into());
    drive.poll("/home", now + SETTLE * 3).unwrap();
    let error = drive
        .poll("/home", now + SETTLE * 3 + STEP_WAIT)
        .unwrap_err();
    assert!(error.contains("did not close after Return"));
    assert!(error.contains("step 4 (wait for close and verify URLs)"));
    assert!(error.contains("The panel directory is \"/home\""));
    assert_eq!(drive.panel.keys.borrow().len(), 4);
}

#[test]
fn rejects_cancellation_wrong_paths_and_multiple_paths() {
    for selected in [
        vec![],
        vec!["/wrong".into()],
        vec!["/home".into(), "/other".into()],
    ] {
        let now = Instant::now();
        let mut drive = Drive::new(Panel::new(), "/home".into(), now);
        navigate(&mut drive, "/home", now);
        *drive.panel.directory.borrow_mut() = Some("/home".into());
        drive.poll("/home", now + SETTLE * 3).unwrap();
        drive.panel.visible.set(false);
        *drive.panel.selected.borrow_mut() = selected;
        let error = drive.poll("/home", now + SETTLE * 4).unwrap_err();
        assert!(error.contains("did not select the isolated Home"));
        assert!(error.contains("step 4"));
    }
}

#[test]
fn waits_for_initial_focus_but_never_types_after_focus_loss() {
    let now = Instant::now();
    let mut drive = Drive::new(Panel::new(), "/home".into(), now);
    drive.panel.focused.set(false);
    assert!(!drive.poll("/home", now).unwrap());
    assert!(drive.panel.keys.borrow().is_empty());
    drive.panel.focused.set(true);
    drive.poll("/home", now).unwrap();
    drive.panel.focused.set(false);
    let error = drive.poll("/home", now + SETTLE).unwrap_err();
    assert!(error.contains("lost keyboard focus"));
    assert!(error.contains("step 1"));
    assert_eq!(drive.panel.keys.borrow().len(), 1);
    let mut drive = Drive::new(Panel::new(), "/home".into(), now);
    drive.panel.focused.set(false);
    assert!(drive
        .poll("/home", now + STEP_WAIT)
        .unwrap_err()
        .contains("step 0"));
    assert!(drive.panel.keys.borrow().is_empty());
}

#[test]
fn rejects_changed_home_early_close_revoked_trust_and_partial_key_failure() {
    for failure in ["home", "close", "trust", "key"] {
        let now = Instant::now();
        let mut drive = Drive::new(Panel::new(), "/home".into(), now);
        drive.poll("/home", now).unwrap();
        let home = if failure == "home" { "/other" } else { "/home" };
        drive.panel.visible.set(failure != "close");
        drive.panel.trusted.set(failure != "trust");
        drive.panel.key_error.set(failure == "key");
        let error = drive.poll(home, now + SETTLE).unwrap_err();
        assert!(error.contains("step 1"));
        assert!(error.contains("The panel directory is \"/start\""));
        assert_eq!(drive.poll(home, now + SETTLE * 2), Err(error));
        assert_eq!(
            drive.panel.keys.borrow().len(),
            if failure == "key" { 2 } else { 1 }
        );
    }
}

#[test]
fn rapid_polls_do_not_repeat_keys() {
    let now = Instant::now();
    let mut drive = Drive::new(Panel::new(), "/home".into(), now);
    drive.poll("/home", now).unwrap();
    for _ in 0..10 {
        assert!(!drive
            .poll("/home", now + SETTLE - Duration::from_millis(1))
            .unwrap());
    }
    assert_eq!(drive.panel.keys.borrow().len(), 1);
}

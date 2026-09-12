use std::time::{Duration, Instant};

const STEP_WAIT: Duration = Duration::from_secs(5);
const SETTLE: Duration = Duration::from_millis(500);

pub(super) trait FolderPanel {
    fn directory(&self) -> Option<String>;
    fn set_directory(&self, home: &str);
    fn is_visible(&self) -> bool;
    fn accept(&self);
    fn selected_paths(&self) -> Vec<String>;
}

pub(super) struct Drive<P> {
    panel: P,
    home: String,
    pub(super) step: u8,
    next: Instant,
    deadline: Instant,
    failure: Option<String>,
}

impl<P: FolderPanel> Drive<P> {
    pub(super) fn new(panel: P, home: String, now: Instant) -> Self {
        Self {
            panel,
            home,
            step: 0,
            next: now,
            deadline: now + STEP_WAIT,
            failure: None,
        }
    }

    fn error(&self, message: &str) -> String {
        let step = match self.step {
            0 => "setDirectoryURL",
            1 => "wait for directoryURL",
            2 => "ok / wait for close",
            _ => "verify URLs",
        };
        format!(
            "{message} The drive stopped at step {} ({step}). The panel directory is {:?}.",
            self.step,
            self.panel
                .directory()
                .unwrap_or_else(|| "unavailable".into())
        )
    }

    pub(super) fn poll(&mut self, home: &str, now: Instant) -> Result<bool, String> {
        if let Some(error) = &self.failure {
            return Err(error.clone());
        }
        let result = self.advance(home, now);
        if let Err(error) = &result {
            self.failure = Some(error.clone());
        }
        result
    }

    fn advance(&mut self, home: &str, now: Instant) -> Result<bool, String> {
        if self.home != home {
            return Err(self.error("The Home path changed during the picker drive."));
        }
        if now < self.next {
            return Ok(false);
        }
        if self.step >= 2 {
            if self.panel.is_visible() {
                if now >= self.deadline {
                    return Err(self.error("The NSOpenPanel did not close after ok."));
                }
                return Ok(false);
            }
            self.step = 3;
            if self.panel.selected_paths() != [self.home.clone()] {
                return Err(self.error("The NSOpenPanel did not select the isolated Home."));
            }
            return Ok(true);
        }
        if !self.panel.is_visible() {
            return Err(self.error("The NSOpenPanel closed before the picker drive finished."));
        }
        if self.step == 0 {
            self.panel.set_directory(&self.home);
            self.step = 1;
        } else {
            if self.panel.directory().as_deref() != Some(self.home.as_str()) {
                if now >= self.deadline {
                    return Err(self.error("The NSOpenPanel did not move to the isolated Home."));
                }
                return Ok(false);
            }
            // The folder-only panel accepts its current directory through its own action.
            self.panel.accept();
            self.step = 2;
        }
        self.next = now + SETTLE;
        self.deadline = now + STEP_WAIT;
        Ok(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};

    struct Panel {
        directory: RefCell<Option<String>>,
        requested: RefCell<Vec<String>>,
        visible: Cell<bool>,
        accepts: Cell<usize>,
        selected: RefCell<Vec<String>>,
    }

    impl Panel {
        fn new() -> Self {
            Self {
                directory: RefCell::new(Some("/start".into())),
                requested: RefCell::new(Vec::new()),
                visible: Cell::new(true),
                accepts: Cell::new(0),
                selected: RefCell::new(Vec::new()),
            }
        }
    }

    impl FolderPanel for Panel {
        fn directory(&self) -> Option<String> {
            self.directory.borrow().clone()
        }
        fn set_directory(&self, home: &str) {
            self.requested.borrow_mut().push(home.into());
        }
        fn is_visible(&self) -> bool {
            self.visible.get()
        }
        fn accept(&self) {
            self.accepts.set(self.accepts.get() + 1);
        }
        fn selected_paths(&self) -> Vec<String> {
            self.selected.borrow().clone()
        }
    }

    #[test]
    fn selects_the_literal_home_through_the_panel_api() {
        for home in ["/", "/tmp/Home space \"quote\" café #%"] {
            let now = Instant::now();
            let mut drive = Drive::new(Panel::new(), home.into(), now);
            assert!(!drive.poll(home, now).unwrap());
            assert_eq!(*drive.panel.requested.borrow(), [home]);
            assert!(!drive.poll(home, now + SETTLE).unwrap());
            assert_eq!(drive.panel.accepts.get(), 0);
            *drive.panel.directory.borrow_mut() = Some(home.into());
            assert!(!drive.poll(home, now + SETTLE).unwrap());
            assert_eq!(drive.panel.accepts.get(), 1);
            assert!(!drive.poll(home, now + SETTLE * 2).unwrap());
            *drive.panel.selected.borrow_mut() = vec![home.into()];
            drive.panel.visible.set(false);
            assert!(drive.poll(home, now + SETTLE * 2).unwrap());
            assert!(drive.poll(home, now + SETTLE * 3).unwrap());
            assert_eq!(drive.panel.accepts.get(), 1);
            assert_eq!(drive.panel.requested.borrow().len(), 1);
        }
    }

    #[test]
    fn reports_a_stuck_or_missing_directory_before_the_outer_timeout() {
        for directory in [Some("/start".into()), None] {
            let now = Instant::now();
            let mut drive = Drive::new(Panel::new(), "/home".into(), now);
            *drive.panel.directory.borrow_mut() = directory.clone();
            drive.poll("/home", now).unwrap();
            assert!(!drive
                .poll("/home", now + STEP_WAIT - Duration::from_millis(1))
                .unwrap());
            let error = drive.poll("/home", now + STEP_WAIT).unwrap_err();
            assert!(error.contains("step 1 (wait for directoryURL)"));
            assert!(error.contains(&format!(
                "The panel directory is {:?}",
                directory.unwrap_or_else(|| "unavailable".into())
            )));
            assert!(STEP_WAIT < Duration::from_secs(30));
            assert_eq!(drive.panel.accepts.get(), 0);
            assert_eq!(drive.poll("/home", now + STEP_WAIT * 2), Err(error));
            assert_eq!(drive.panel.requested.borrow().len(), 1);
        }
    }

    #[test]
    fn reports_a_panel_that_ignores_ok() {
        let now = Instant::now();
        let mut drive = Drive::new(Panel::new(), "/home".into(), now);
        drive.poll("/home", now).unwrap();
        *drive.panel.directory.borrow_mut() = Some("/home".into());
        drive.poll("/home", now + SETTLE).unwrap();
        let error = drive.poll("/home", now + SETTLE + STEP_WAIT).unwrap_err();
        assert!(error.contains("step 2 (ok / wait for close)"));
        assert!(error.contains("The panel directory is \"/home\""));
        assert_eq!(drive.panel.accepts.get(), 1);
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
            drive.poll("/home", now).unwrap();
            *drive.panel.directory.borrow_mut() = Some("/home".into());
            drive.poll("/home", now + SETTLE).unwrap();
            drive.panel.visible.set(false);
            *drive.panel.selected.borrow_mut() = selected;
            let error = drive.poll("/home", now + SETTLE * 2).unwrap_err();
            assert!(error.contains("did not select the isolated Home"));
            assert!(error.contains("step 3 (verify URLs)"));
        }
    }

    #[test]
    fn rejects_a_changed_home_or_early_close_without_accepting() {
        let now = Instant::now();
        let mut drive = Drive::new(Panel::new(), "/home".into(), now);
        let error = drive.poll("/other", now).unwrap_err();
        assert!(error.contains("Home path changed"));
        assert!(drive.panel.requested.borrow().is_empty());
        assert_eq!(drive.panel.accepts.get(), 0);
        let mut drive = Drive::new(Panel::new(), "/home".into(), now);
        drive.poll("/home", now).unwrap();
        drive.panel.visible.set(false);
        let error = drive.poll("/home", now + SETTLE).unwrap_err();
        assert!(error.contains("closed before"));
        assert!(error.contains("step 1"));
        assert_eq!(drive.panel.accepts.get(), 0);
    }
}

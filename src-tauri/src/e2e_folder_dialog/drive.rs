use std::time::{Duration, Instant};

const STEP_WAIT: Duration = Duration::from_secs(5);
const SETTLE: Duration = Duration::from_millis(800);

pub(super) trait FolderPanel {
    fn directory(&self) -> Option<String>;
    fn is_visible(&self) -> bool;
    fn is_focused(&self) -> bool;
    fn is_trusted(&self) -> bool;
    fn send_step(&self, step: u8, home: &str) -> Result<(), String>;
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
            0 => "Command Shift G",
            1 => "Command A and Home path",
            2 => "Return to navigate",
            3 => "Return to choose Home",
            _ => "wait for close and verify URLs",
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
        if self.step == 4 {
            if self.panel.is_visible() {
                if now >= self.deadline {
                    return Err(self.error("The NSOpenPanel did not close after Return."));
                }
                return Ok(false);
            }
            if self.panel.selected_paths() != [self.home.clone()] {
                return Err(self.error("The NSOpenPanel did not select the isolated Home."));
            }
            return Ok(true);
        }
        if !self.panel.is_visible() {
            return Err(self.error("The NSOpenPanel closed before the picker drive finished."));
        }
        if !self.panel.is_trusted() {
            return Err(self.error("AXIsProcessTrusted() returned false. The Home picker cannot post CGEvent keyboard events."));
        }
        if !self.panel.is_focused() {
            // Activation can arrive after the first poll. Later focus loss must stop all keys.
            if self.step == 0 && now < self.deadline {
                return Ok(false);
            }
            return Err(self.error("The NSOpenPanel lost keyboard focus."));
        }
        // Never accept the folder on screen until navigation reaches the isolated Home.
        if self.step == 3 && self.panel.directory().as_deref() != Some(home) {
            if now >= self.deadline {
                return Err(
                    self.error("The NSOpenPanel did not move to the isolated Home after Return.")
                );
            }
            return Ok(false);
        }
        self.panel
            .send_step(self.step, home)
            .map_err(|error| self.error(&error))?;
        self.step += 1;
        self.next = now + SETTLE;
        self.deadline = now + STEP_WAIT;
        Ok(false)
    }
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;

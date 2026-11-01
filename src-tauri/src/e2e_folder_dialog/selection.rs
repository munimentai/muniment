pub(super) fn matches_home(selected: Option<&str>, home: &str) -> bool {
    // The panel answers a resolved path, so compare canonical forms.
    let canonical = |path: &str| std::fs::canonicalize(path).ok();
    let expected = canonical(home);
    expected.is_some() && selected.and_then(canonical) == expected
}

// The trace outlives the panel and uses one clock for worker and AppKit milestones.
#[derive(Clone, Debug)]
#[cfg_attr(target_os = "macos", derive(serde::Serialize))]
pub(super) struct TimedEvent {
    #[cfg_attr(target_os = "macos", serde(flatten))]
    pub(super) event: super::accessibility::NavigationEvent,
    pub(super) elapsed_ms: u64,
    pub(super) reason: Option<String>,
}

pub(super) struct Trace {
    start: std::time::Instant,
    pub(super) events: Vec<TimedEvent>,
}

impl Trace {
    pub(super) fn new(start: std::time::Instant) -> Self {
        let mut trace = Self {
            start,
            events: Vec::new(),
        };
        trace.record(
            super::accessibility::NavigationEvent::stage("navigation_started"),
            start,
        );
        trace
    }

    pub(super) fn record(
        &mut self,
        event: super::accessibility::NavigationEvent,
        now: std::time::Instant,
    ) {
        // Reserve space for completion and failure even after discovery fills the trace.
        if self.events.len() >= 120
            && matches!(
                event.stage,
                "focused_window" | "sheet_control" | "sheet_owner"
            )
        {
            return;
        }
        if self.events.iter().any(|previous| previous.event == event) {
            return;
        }
        let previous_ms = self.events.last().map_or(0, |event| event.elapsed_ms);
        self.events.push(TimedEvent {
            event,
            elapsed_ms: now
                .saturating_duration_since(self.start)
                .as_millis()
                .min(u128::from(u64::MAX)) as u64,
            reason: None,
        });
        if let Some(last) = self.events.last_mut() {
            last.elapsed_ms = last.elapsed_ms.max(previous_ms);
        }
    }

    pub(super) fn awaited(&self) -> &'static str {
        let has = |stage| self.events.iter().any(|entry| entry.event.stage == stage);
        if has("directory_changed") {
            "panel closure and exact selected Home"
        } else if has("panel_refocused") {
            "canonical panel directory change"
        } else if has("go_committed") {
            "navigation sheet closure and panel focus"
        } else if has("value_set") {
            "path acceptance and sheet commit"
        } else if has("sheet_found") {
            "owned path entry"
        } else if has("key_posted") {
            "owned Go to Folder sheet discovery"
        } else {
            "marked panel discovery"
        }
    }

    pub(super) fn fail(&mut self, reason: String, now: std::time::Instant) {
        self.record(super::accessibility::NavigationEvent::stage("failed"), now);
        let awaited = self.awaited();
        if let Some(entry) = self
            .events
            .last_mut()
            .filter(|entry| entry.event.stage == "failed")
        {
            entry.reason = Some(format!("Awaiting {}: {reason}", awaited));
        }
    }
}

// Commitment includes positive sheet closure; a stale proxy directory alone is insufficient.
pub(super) fn ready_to_open(committed: bool, directory: Option<&str>, home: &str) -> bool {
    committed && matches_home(directory, home)
}

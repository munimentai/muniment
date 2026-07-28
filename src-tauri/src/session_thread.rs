use std::sync::Mutex;

#[derive(Clone, Debug, Eq, PartialEq)]
struct TrackedThread {
    thread_id: String,
    workspace: String,
    subject: Option<String>,
}

#[derive(Debug, Default)]
pub(crate) struct SessionThread {
    tracked: Mutex<Option<TrackedThread>>,
}

impl SessionThread {
    pub(crate) fn offered(&self, workspace: &str, subject: Option<&str>) -> Option<String> {
        self.tracked
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_ref()
            .filter(|tracked| {
                tracked.workspace == workspace && tracked.subject.as_deref() == subject
            })
            .map(|tracked| tracked.thread_id.clone())
    }

    pub(crate) fn record(&self, thread_id: String, workspace: &str, subject: Option<&str>) {
        *self
            .tracked
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(TrackedThread {
            thread_id,
            workspace: workspace.to_owned(),
            subject: subject.map(str::to_owned),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::SessionThread;

    #[test]
    fn starts_empty() {
        assert_eq!(
            SessionThread::default().offered("workspace-a", Some("owner")),
            None
        );
    }

    #[test]
    fn offers_only_an_exact_workspace_and_subject_match() {
        let tracker = SessionThread::default();
        tracker.record("thread-a".into(), "workspace-a", Some("owner"));

        assert_eq!(
            tracker.offered("workspace-a", Some("owner")),
            Some("thread-a".into())
        );
        assert_eq!(tracker.offered("workspace-b", Some("owner")), None);
        assert_eq!(tracker.offered("workspace-a", Some("other")), None);
        assert_eq!(tracker.offered("workspace-a", None), None);
    }

    #[test]
    fn records_the_latest_thread() {
        let tracker = SessionThread::default();
        tracker.record("thread-a".into(), "workspace-a", None);
        tracker.record("thread-b".into(), "workspace-b", None);

        assert_eq!(tracker.offered("workspace-a", None), None);
        assert_eq!(
            tracker.offered("workspace-b", None),
            Some("thread-b".into())
        );
    }
}

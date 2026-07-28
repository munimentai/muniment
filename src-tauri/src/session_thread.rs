use std::sync::Mutex;

#[derive(Clone, Debug, Eq, PartialEq)]
struct SelectedThread {
    thread_id: String,
    workspace: Option<String>,
    subject: Option<String>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
enum ThreadChoice {
    #[default]
    Unset,
    Selected(SelectedThread),
    Fresh(Option<String>),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum OfferedThread {
    AdoptNewest,
    Selected(String),
    Fresh,
}

#[derive(Debug, Default)]
pub(crate) struct SessionThread {
    choice: Mutex<ThreadChoice>,
}

impl SessionThread {
    pub(crate) fn offered(&self, workspace: &str, subject: Option<&str>) -> OfferedThread {
        match &*self
            .choice
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
        {
            ThreadChoice::Selected(selected)
                if selected.subject.as_deref() == subject
                    && selected
                        .workspace
                        .as_deref()
                        .is_none_or(|tracked| tracked == workspace) =>
            {
                OfferedThread::Selected(selected.thread_id.clone())
            }
            ThreadChoice::Fresh(fresh_subject) if fresh_subject.as_deref() == subject => {
                OfferedThread::Fresh
            }
            ThreadChoice::Unset | ThreadChoice::Selected(_) => OfferedThread::AdoptNewest,
            ThreadChoice::Fresh(_) => OfferedThread::AdoptNewest,
        }
    }

    pub(crate) fn select(&self, thread_id: String, subject: Option<&str>) {
        *self
            .choice
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) =
            ThreadChoice::Selected(SelectedThread {
                thread_id,
                workspace: None,
                subject: subject.map(str::to_owned),
            });
    }

    pub(crate) fn fresh(&self, subject: Option<&str>) {
        *self
            .choice
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) =
            ThreadChoice::Fresh(subject.map(str::to_owned));
    }

    pub(crate) fn record(&self, thread_id: String, workspace: &str, subject: Option<&str>) {
        *self
            .choice
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) =
            ThreadChoice::Selected(SelectedThread {
                thread_id,
                workspace: Some(workspace.to_owned()),
                subject: subject.map(str::to_owned),
            });
    }
}

#[cfg(test)]
mod tests {
    use super::{OfferedThread, SessionThread};

    #[test]
    fn starts_unset() {
        assert_eq!(
            SessionThread::default().offered("workspace-a", Some("owner")),
            OfferedThread::AdoptNewest
        );
    }

    #[test]
    fn selection_applies_without_a_workspace() {
        let tracker = SessionThread::default();
        tracker.select("thread-a".into(), Some("owner"));

        assert_eq!(
            tracker.offered("workspace-a", Some("owner")),
            OfferedThread::Selected("thread-a".into())
        );
        assert_eq!(
            tracker.offered("workspace-b", Some("owner")),
            OfferedThread::Selected("thread-a".into())
        );
        assert_eq!(
            tracker.offered("workspace-a", Some("other")),
            OfferedThread::AdoptNewest
        );
    }

    #[test]
    fn recorded_thread_requires_an_exact_workspace_and_subject_match() {
        let tracker = SessionThread::default();
        tracker.record("thread-a".into(), "workspace-a", Some("owner"));

        assert_eq!(
            tracker.offered("workspace-a", Some("owner")),
            OfferedThread::Selected("thread-a".into())
        );
        assert_eq!(
            tracker.offered("workspace-b", Some("owner")),
            OfferedThread::AdoptNewest
        );
        assert_eq!(
            tracker.offered("workspace-a", Some("other")),
            OfferedThread::AdoptNewest
        );
    }

    #[test]
    fn fresh_forces_a_new_thread() {
        let tracker = SessionThread::default();
        tracker.select("thread-a".into(), Some("owner"));
        tracker.fresh(Some("owner"));

        assert_eq!(
            tracker.offered("workspace-a", Some("owner")),
            OfferedThread::Fresh
        );
    }

    #[test]
    fn fresh_choice_does_not_cross_subjects() {
        let tracker = SessionThread::default();
        tracker.fresh(Some("owner"));

        assert_eq!(
            tracker.offered("workspace-a", Some("other")),
            OfferedThread::AdoptNewest
        );
    }
}

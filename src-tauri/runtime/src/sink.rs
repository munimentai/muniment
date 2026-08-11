//! Dormant runtime chat event delivery boundaries.

use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;

use muniment_core::chat_profile::ChatProfile;
use muniment_core::pi_launch::{PiLaunchBoundaries, PiLaunchError};
use muniment_core::run_events::{ChatEvent, ChatEventSink};

pub struct RuntimeChatEventSink {
    profile: ChatProfile,
    subscriber: Option<Sender<ChatEvent>>,
    executable_override: Option<PathBuf>,
}

impl RuntimeChatEventSink {
    pub fn new(profile_directory: impl AsRef<Path>, subscriber: Option<Sender<ChatEvent>>) -> Self {
        Self {
            profile: ChatProfile::new(profile_directory.as_ref()),
            subscriber,
            executable_override: None,
        }
    }

    #[doc(hidden)]
    pub fn with_pi_executable(mut self, executable: PathBuf) -> Self {
        self.executable_override = Some(executable);
        self
    }
}

impl ChatEventSink for RuntimeChatEventSink {
    fn deliver(&self, event: ChatEvent) -> Result<(), ()> {
        match &self.subscriber {
            Some(subscriber) => subscriber.send(event).map_err(|_| ()),
            None => Ok(()),
        }
    }
}

impl PiLaunchBoundaries for RuntimeChatEventSink {
    fn pi_session_root(&self) -> Result<PathBuf, PiLaunchError> {
        Ok(self.profile.pi_session_root())
    }

    fn memory_agent_extension_path(&self) -> Option<PathBuf> {
        None
    }

    fn pi_executable_override(&self) -> Option<PathBuf> {
        self.executable_override.clone()
    }
}

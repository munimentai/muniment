//! Dormant runtime chat event delivery boundaries.

use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;
use std::sync::Arc;

use muniment_core::chat_profile::ChatProfile;
use muniment_core::memory_runtime::ApplicationMemoryRuntime;
use muniment_core::pi_launch::{PiLaunchBoundaries, PiLaunchError};
use muniment_core::run_events::{ChatEvent, ChatEventSink};
use muniment_core::sidecar::pi_install::{PiArtifactDescriptor, PI_ARTIFACT};

pub struct RuntimeChatEventSink {
    profile: ChatProfile,
    subscriber: Option<Sender<ChatEvent>>,
    pi_artifact: PiArtifactDescriptor,
    memory_runtime: Arc<ApplicationMemoryRuntime>,
}

impl RuntimeChatEventSink {
    pub fn new(
        profile_directory: impl AsRef<Path>,
        subscriber: Option<Sender<ChatEvent>>,
        memory_runtime: Arc<ApplicationMemoryRuntime>,
    ) -> Self {
        Self {
            profile: ChatProfile::new(profile_directory.as_ref()),
            subscriber,
            pi_artifact: PI_ARTIFACT,
            memory_runtime,
        }
    }

    pub fn with_pi_artifact(mut self, pi_artifact: PiArtifactDescriptor) -> Self {
        self.pi_artifact = pi_artifact;
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
    fn pi_artifact(&self) -> PiArtifactDescriptor {
        self.pi_artifact
    }

    fn pi_session_root(&self) -> Result<PathBuf, PiLaunchError> {
        Ok(self.profile.pi_session_root())
    }

    fn memory_agent_extension_path(&self) -> Option<PathBuf> {
        Some(self.memory_runtime.agent_extension_path())
    }
}

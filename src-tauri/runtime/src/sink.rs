//! Dormant runtime chat event delivery boundaries.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Sender, SyncSender, TrySendError};
use std::sync::{Arc, Mutex, Weak};

use muniment_core::chat_profile::ChatProfile;
use muniment_core::memory_runtime::ApplicationMemoryRuntime;
use muniment_core::pi_launch::{PiLaunchBoundaries, PiLaunchError};
use muniment_core::run_events::{ChatEvent, ChatEventSink, ChatEventSubscription};
use muniment_core::sidecar::pi_install::{PiArtifactDescriptor, PI_ARTIFACT};

pub const CHAT_EVENT_SUBSCRIBER_QUEUE_CAPACITY: usize = 256;

/// Fans each chat event out to every live subscriber without blocking a run.
#[derive(Clone, Default)]
pub struct RuntimeChatEventBroadcast {
    shared: Arc<RuntimeChatEventBroadcastShared>,
}

#[derive(Default)]
struct RuntimeChatEventBroadcastShared {
    next_id: AtomicU64,
    subscribers: Mutex<Vec<RuntimeChatEventSubscriber>>,
}

struct RuntimeChatEventSubscriber {
    id: u64,
    sender: SyncSender<ChatEvent>,
}

impl RuntimeChatEventBroadcast {
    pub fn subscribe(&self) -> ChatEventSubscription {
        let (sender, receiver) = mpsc::sync_channel(CHAT_EVENT_SUBSCRIBER_QUEUE_CAPACITY);
        let id = self.shared.next_id.fetch_add(1, Ordering::Relaxed);
        self.shared
            .subscribers
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(RuntimeChatEventSubscriber { id, sender });
        let shared = Arc::downgrade(&self.shared);
        ChatEventSubscription::new(receiver, move || remove_subscriber(&shared, id))
    }

    #[doc(hidden)]
    pub fn subscriber_count(&self) -> usize {
        self.shared
            .subscribers
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .len()
    }

    fn deliver(&self, event: ChatEvent) {
        self.shared
            .subscribers
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .retain(
                |subscriber| match subscriber.sender.try_send(event.clone()) {
                    Ok(()) => true,
                    Err(TrySendError::Full(_) | TrySendError::Disconnected(_)) => false,
                },
            );
    }
}

fn remove_subscriber(shared: &Weak<RuntimeChatEventBroadcastShared>, id: u64) {
    let Some(shared) = shared.upgrade() else {
        return;
    };
    shared
        .subscribers
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .retain(|subscriber| subscriber.id != id);
}

enum RuntimeChatEventTarget {
    Subscriber(Option<Sender<ChatEvent>>),
    Broadcast(RuntimeChatEventBroadcast),
}

pub struct RuntimeChatEventSink {
    profile: ChatProfile,
    target: Mutex<RuntimeChatEventTarget>,
    pi_artifact: PiArtifactDescriptor,
    memory_runtime: Arc<ApplicationMemoryRuntime>,
}

impl RuntimeChatEventSink {
    pub fn new(
        profile_directory: impl AsRef<Path>,
        broadcast: RuntimeChatEventBroadcast,
        memory_runtime: Arc<ApplicationMemoryRuntime>,
    ) -> Self {
        Self {
            profile: ChatProfile::new(profile_directory.as_ref()),
            target: Mutex::new(RuntimeChatEventTarget::Broadcast(broadcast)),
            pi_artifact: PI_ARTIFACT,
            memory_runtime,
        }
    }

    pub fn with_subscriber(
        profile_directory: impl AsRef<Path>,
        subscriber: Option<Sender<ChatEvent>>,
        memory_runtime: Arc<ApplicationMemoryRuntime>,
    ) -> Self {
        Self {
            profile: ChatProfile::new(profile_directory.as_ref()),
            target: Mutex::new(RuntimeChatEventTarget::Subscriber(subscriber)),
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
    fn provenance(&self) -> (&str, &str) {
        ("muniment-runtime", env!("CARGO_PKG_VERSION"))
    }

    fn deliver(&self, event: ChatEvent) -> Result<(), ()> {
        let mut target = self.target.lock().map_err(|_| ())?;
        match &mut *target {
            RuntimeChatEventTarget::Subscriber(subscriber) => {
                if subscriber
                    .as_ref()
                    .is_some_and(|subscriber| subscriber.send(event).is_err())
                {
                    *subscriber = None;
                }
                Ok(())
            }
            RuntimeChatEventTarget::Broadcast(broadcast) => {
                broadcast.deliver(event);
                Ok(())
            }
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

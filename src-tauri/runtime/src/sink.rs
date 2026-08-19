//! Dormant runtime chat event delivery boundaries.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Sender, SyncSender, TrySendError};
use std::sync::{Arc, Mutex, Weak};

use muniment_core::attach::SignedWorkspaceApproval;
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
    approval: SignedWorkspaceApproval,
}

#[derive(Default)]
struct RuntimeChatEventBroadcastShared {
    next_id: AtomicU64,
    full_queue_drops: AtomicU64,
    subscribers: Mutex<Vec<RuntimeChatEventSubscriber>>,
}

struct RuntimeChatEventSubscriber {
    id: u64,
    sender: SyncSender<ChatEvent>,
}

impl RuntimeChatEventBroadcast {
    pub fn new(approval: SignedWorkspaceApproval) -> Self {
        Self {
            shared: Arc::new(RuntimeChatEventBroadcastShared::default()),
            approval,
        }
    }

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

    /// Counts the subscribers this broadcast dropped for a full queue.
    #[doc(hidden)]
    pub fn full_queue_drop_count(&self) -> u64 {
        self.shared.full_queue_drops.load(Ordering::Relaxed)
    }

    fn deliver(&self, workspace: &str, event: ChatEvent) {
        if !self
            .approval
            .approval()
            .is_some_and(|approval| approval.workspace == workspace)
        {
            return;
        }
        let mut full_queue_drops = Vec::new();
        self.shared
            .subscribers
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .retain(
                |subscriber| match subscriber.sender.try_send(event.clone()) {
                    Ok(()) => true,
                    Err(TrySendError::Full(_)) => {
                        full_queue_drops.push(subscriber.id);
                        false
                    }
                    // A disconnected receiver is the ordinary close of a client connection.
                    Err(TrySendError::Disconnected(_)) => false,
                },
            );
        self.shared
            .full_queue_drops
            .fetch_add(full_queue_drops.len() as u64, Ordering::Relaxed);
        // Report each drop after the lock releases, so no run waits on stderr.
        for id in full_queue_drops {
            eprintln!(
                "muniment-runtime: dropped chat-event subscriber {id} because its \
                 {CHAT_EVENT_SUBSCRIBER_QUEUE_CAPACITY}-event queue is full"
            );
        }
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

/// Names where one run sends its chat events.
pub enum RuntimeChatEventTarget {
    /// Sends to one caller-owned subscriber, or to nobody.
    Subscriber(Option<Sender<ChatEvent>>),
    /// Fans out to every live subscriber of the shared broadcast.
    Broadcast(RuntimeChatEventBroadcast),
}

pub struct RuntimeChatEventSink {
    profile: ChatProfile,
    target: Mutex<RuntimeChatEventTarget>,
    pi_artifact: PiArtifactDescriptor,
    memory_runtime: Arc<ApplicationMemoryRuntime>,
    thread_id: String,
    workspace: String,
}

impl RuntimeChatEventSink {
    pub fn new(
        profile_directory: impl AsRef<Path>,
        broadcast: RuntimeChatEventBroadcast,
        memory_runtime: Arc<ApplicationMemoryRuntime>,
        thread_id: String,
        workspace: String,
    ) -> Self {
        Self::with_target(
            profile_directory,
            RuntimeChatEventTarget::Broadcast(broadcast),
            memory_runtime,
            thread_id,
            workspace,
        )
    }

    pub fn with_subscriber(
        profile_directory: impl AsRef<Path>,
        subscriber: Option<Sender<ChatEvent>>,
        memory_runtime: Arc<ApplicationMemoryRuntime>,
        thread_id: String,
    ) -> Self {
        Self::with_target(
            profile_directory,
            RuntimeChatEventTarget::Subscriber(subscriber),
            memory_runtime,
            thread_id,
            String::new(),
        )
    }

    pub fn with_target(
        profile_directory: impl AsRef<Path>,
        target: RuntimeChatEventTarget,
        memory_runtime: Arc<ApplicationMemoryRuntime>,
        thread_id: String,
        workspace: String,
    ) -> Self {
        Self {
            profile: ChatProfile::new(profile_directory.as_ref()),
            target: Mutex::new(target),
            pi_artifact: PI_ARTIFACT,
            memory_runtime,
            thread_id,
            workspace,
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

    fn deliver(&self, mut event: ChatEvent) -> Result<(), ()> {
        event.thread_id = Some(self.thread_id.clone());
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
                broadcast.deliver(&self.workspace, event);
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

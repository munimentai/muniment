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
use muniment_core::runtime_eprintln as eprintln;
use muniment_core::sidecar::pi_install::{PiArtifactDescriptor, PI_SELECTED_ARTIFACT};

pub const CHAT_EVENT_SUBSCRIBER_QUEUE_CAPACITY: usize = 256;

/// Fans each chat event out to every live subscriber without blocking a run.
#[derive(Clone, Default)]
pub struct RuntimeChatEventBroadcast {
    shared: Arc<RuntimeChatEventBroadcastShared>,
    approval: SignedWorkspaceApproval,
    config_directory: Option<PathBuf>,
}

#[derive(Default)]
struct RuntimeChatEventBroadcastShared {
    next_id: AtomicU64,
    full_queue_drops: AtomicU64,
    subscribers: Mutex<Vec<RuntimeChatEventSubscriber>>,
    #[cfg(target_os = "linux")]
    delivery_failures: Mutex<std::collections::BTreeMap<(String, String), ChatEvent>>,
}

struct RuntimeChatEventSubscriber {
    id: u64,
    capacity: usize,
    sender: SyncSender<ChatEvent>,
}

impl RuntimeChatEventBroadcast {
    pub fn new(approval: SignedWorkspaceApproval) -> Self {
        Self {
            shared: Arc::new(RuntimeChatEventBroadcastShared::default()),
            approval,
            config_directory: None,
        }
    }

    pub fn with_config_directory(mut self, config_directory: PathBuf) -> Self {
        self.config_directory = Some(config_directory);
        self
    }

    fn allows_workspace(&self, workspace: &str) -> bool {
        if let Some(directory) = &self.config_directory {
            if muniment_core::local_mode::is_local_mode(directory) {
                return workspace == "local";
            }
        }
        self.approval
            .approval()
            .is_some_and(|approval| approval.workspace == workspace)
    }

    #[cfg(target_os = "linux")]
    pub(crate) fn record_delivery_failure(&self, workspace: String, event: ChatEvent) {
        // Subscription setup and socket writes do not acknowledge receipt by the shell.
        // Keep each run's latest cause for every desktop in its workspace until runtime exit.
        let mut failures = self
            .shared
            .delivery_failures
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        failures.insert((workspace.clone(), event.run_id.clone()), event.clone());
        self.deliver(&workspace, event);
    }

    pub fn subscribe(&self) -> ChatEventSubscription {
        #[cfg(target_os = "linux")]
        let failures = self
            .shared
            .delivery_failures
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        #[cfg(target_os = "linux")]
        let capacity = CHAT_EVENT_SUBSCRIBER_QUEUE_CAPACITY.max(failures.len());
        #[cfg(not(target_os = "linux"))]
        let capacity = CHAT_EVENT_SUBSCRIBER_QUEUE_CAPACITY;
        let (sender, receiver) = mpsc::sync_channel(capacity);
        #[cfg(target_os = "linux")]
        for ((workspace, _), failure) in failures.iter() {
            if self.allows_workspace(workspace) {
                let _ = sender.try_send(failure.clone());
            }
        }
        let id = self.shared.next_id.fetch_add(1, Ordering::Relaxed);
        self.shared
            .subscribers
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(RuntimeChatEventSubscriber {
                id,
                capacity,
                sender,
            });
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
        if !self.allows_workspace(workspace) {
            return;
        }
        self.fan_out(event);
    }

    /// Sends a device-wide event, such as the sign-in link, to every live
    /// subscriber. The workspace gate guards run events, and this event names
    /// no run.
    pub(crate) fn announce(&self, event: ChatEvent) {
        self.fan_out(event);
    }

    fn fan_out(&self, event: ChatEvent) {
        let mut full_queue_drops = Vec::new();
        self.shared
            .subscribers
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .retain(
                |subscriber| match subscriber.sender.try_send(event.clone()) {
                    Ok(()) => true,
                    Err(TrySendError::Full(_)) => {
                        full_queue_drops.push((subscriber.id, subscriber.capacity));
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
        for (id, capacity) in full_queue_drops {
            eprintln!(
                "muniment-runtime: dropped chat-event subscriber {id} because its \
                 {capacity}-event queue is full"
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
    earlier_models: Vec<String>,
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
            pi_artifact: PI_SELECTED_ARTIFACT,
            memory_runtime,
            thread_id,
            workspace,
            earlier_models: Vec::new(),
        }
    }

    pub fn with_pi_artifact(mut self, pi_artifact: PiArtifactDescriptor) -> Self {
        self.pi_artifact = pi_artifact;
        self
    }

    /// The models that answered earlier runs of this thread, for the prompt's facts.
    pub fn with_earlier_models(mut self, earlier_models: Vec<String>) -> Self {
        self.earlier_models = earlier_models;
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

    fn agent_instructions(&self) -> Result<Option<String>, PiLaunchError> {
        let sessions = self.profile.pi_session_root();
        let profile = sessions
            .parent()
            .ok_or(PiLaunchError::UnavailableSessionRoot)?;
        muniment_core::agents::thread_instructions(profile, &self.thread_id)
            .map_err(|e| PiLaunchError::rejected("agent", e))
    }

    fn project_directory(&self) -> Result<Option<PathBuf>, PiLaunchError> {
        let sessions = self.profile.pi_session_root();
        let profile = sessions
            .parent()
            .ok_or(PiLaunchError::UnavailableSessionRoot)?;
        muniment_core::projects::workspace_with_config(
            profile,
            self.memory_runtime.config_directory(),
            &self.thread_id,
        )
        .map(Some)
        .map_err(|error| PiLaunchError::rejected("project_folder", error))
    }

    fn earlier_models(&self) -> Vec<String> {
        self.earlier_models.clone()
    }
}

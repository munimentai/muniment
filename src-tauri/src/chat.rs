use std::collections::VecDeque;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::{channel, RecvTimeoutError, Sender};
use std::sync::OnceLock;
use std::sync::{Arc, Mutex};
#[cfg(test)]
use std::time::Duration;

use muniment_core::active_run::{
    cancel_active_run, queue_message, queue_permission_answer, queue_permission_answer_with_commit,
    ChatDelivery, ChatQueueRequest,
};
#[cfg(any(target_os = "linux", target_os = "windows"))]
use muniment_core::attach::thread_service::{
    ThreadListPage, ThreadListRequest, ThreadListService, ThreadOpenPage, ThreadOpenRequest,
};
#[cfg(any(target_os = "linux", target_os = "windows"))]
use muniment_core::attach::ProtocolError;
#[cfg(unix)]
use muniment_core::attach::{
    ClientError, DesktopClientHolder, RunCancelAccepted, RunMessageAccepted,
    RunPermissionAnswerAccepted, RunResumeAccepted, RunSubmitAccepted,
};
use muniment_core::attach::{RuntimeActivityGuard, RuntimeActivityRegistry};
#[cfg(test)]
use muniment_core::attachment::AttachmentDeliveryError;
use muniment_core::auth::{api_base_url, TokenSet};
#[cfg(test)]
use muniment_core::cas::LocalCas;
use muniment_core::chat_coordinate::coordinate;
use muniment_core::chat_grant::{
    fetch_grant as core_fetch_grant, grant_authorizes_workspace,
    validate_grant as core_validate_grant, ChatGrant, FetchGrantError,
};
use muniment_core::chat_profile::ChatProfile;
pub(crate) use muniment_core::chat_resume::ResumeContext;
use muniment_core::chat_resume::{
    clear_active_run, install_active_run, install_resume_run,
    resumable_context as core_resumable_context, run_resume, ChatResumeError, ResumeLaunch,
};
use muniment_core::chat_view::{chat_attachments, ChatAttachment, SelectedFile};
use muniment_core::journal::reconciliation::reconcile_interrupted_runs;
use muniment_core::journal::reducer::{project_chat, ChatProjector};
use muniment_core::journal::retention::{apply_retention_now_with, RetentionError};
#[cfg(target_os = "linux")]
use muniment_core::journal::thread_mutation::create_thread_now;
use muniment_core::journal::{EventEnvelope, Provenance};
#[cfg(test)]
use muniment_core::journal::{EventPayload, RunJournal};
use muniment_core::memory_index::ModelMemoryCapability;
use muniment_core::permission_gate::ChatPermissionAnswer;
#[cfg(test)]
use muniment_core::pi_execution::attachment_delivery_error;
pub(crate) use muniment_core::pi_execution::{attachment_error, PiRuntime};
use muniment_core::pi_launch::{PiLaunchBoundaries, PiLaunchError};
use muniment_core::run_events::{ChatEvent, ChatEventSink};
pub(crate) use muniment_core::run_events::{ChatStorage, SharedStorage};
pub(crate) use muniment_core::run_preparation::SessionThreadStart;
use muniment_core::run_preparation::{self as core_run_preparation, OpenSelectedFile};
use muniment_core::run_start::{
    start_desktop_run, ActiveRun, RunAttachBoundaries, RunStartBoundaries, RunStartError,
    RunStartLaunch, RunStartRequest, SubmitResult,
};
#[cfg(target_os = "linux")]
use muniment_core::thread_ownership::subject_owns_first_run;
#[cfg(test)]
use serde_json::json;
use serde_json::Value;
use std::path::PathBuf;
use tauri::{Emitter, Manager};
#[cfg(test)]
use uuid::Uuid;

#[cfg(unix)]
use crate::attach_service::DesktopClientSession;
use crate::auth;
#[cfg(unix)]
use muniment_core::attach::ChatPermissionAnswer as AttachChatPermissionAnswer;
#[cfg(test)]
use muniment_core::session_thread::OfferedThread;
use muniment_core::session_thread::SessionThread;

mod commands;
mod resume;
mod run_preparation;
mod state;

pub use commands::*;
pub use resume::*;
pub use run_preparation::*;
pub use state::*;

#[cfg(test)]
pub(crate) use resume::resumable_context;
pub(crate) use resume::state_session_root;
#[cfg(test)]
pub(crate) use run_preparation::{
    desktop_provenance, event_envelope, prepare_new_run, prepare_new_run_with_session_thread,
};
#[cfg(any(target_os = "linux", target_os = "windows"))]
pub(crate) use state::TauriRunStartBoundaries;
pub(crate) use state::{RetentionTrigger, TauriChatEventSink};

#[cfg(test)]
mod tests;

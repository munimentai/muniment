use muniment_core::active_run::ChatDelivery;
#[cfg(any(unix, target_os = "windows"))]
use muniment_core::attach::{
    ClientError, DesktopClientHolder, RunCancelAccepted, RunMessageAccepted,
    RunPermissionAnswerAccepted, RunResumeAccepted, RunSubmitAccepted,
};
use muniment_core::chat_profile::ChatProfile;
use muniment_core::chat_view::{ChatAttachment, SelectedFile};
use muniment_core::permission_gate::ChatPermissionAnswer;
use muniment_core::pi_execution::attachment_error;
use muniment_core::run_start::SubmitResult;
use std::path::PathBuf;
use tauri::Manager;

#[cfg(any(unix, target_os = "windows"))]
use crate::attach_service::DesktopClientSession;
use crate::auth;
#[cfg(any(unix, target_os = "windows"))]
use muniment_core::attach::ChatPermissionAnswer as AttachChatPermissionAnswer;
use muniment_core::session_thread::SessionThread;

mod commands;
mod resume;
mod run_preparation;
mod state;

pub use commands::*;
pub use resume::*;
pub use run_preparation::*;
pub use state::*;

pub(crate) use resume::state_session_root;

#[cfg(test)]
mod tests;

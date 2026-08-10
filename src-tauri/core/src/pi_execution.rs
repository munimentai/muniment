use std::sync::mpsc::Sender;
use std::time::Duration;

use serde_json::json;

use crate::attachment::{prepare_pi_images, AttachmentDeliveryError};
use crate::journal::reducer::ChatProjector;
use crate::run_events::{append_emit, ChatEventSink, SharedStorage};
use crate::sidecar::pi_chat::{PiChatEvent, PiImageContent, PromptCommand};
use crate::sidecar::{PiRpcWiring, PiSessionLocator, SidecarSupervisor};

pub const RPC_TIMEOUT: Duration = Duration::from_secs(30);

pub struct PiRuntime {
    pub supervisor: SidecarSupervisor,
    pub wiring: PiRpcWiring,
}

pub struct ResumeAttempt {
    result: Option<Sender<Result<(), String>>>,
}

impl ResumeAttempt {
    pub fn new(result: Option<Sender<Result<(), String>>>) -> Self {
        Self { result }
    }

    pub fn accepted(&mut self) {
        if let Some(result) = self.result.take() {
            let _ = result.send(Ok(()));
        }
    }
}

impl Drop for ResumeAttempt {
    fn drop(&mut self) {
        if let Some(result) = self.result.take() {
            let _ = result.send(Err("This reply could not be resumed. Try again.".into()));
        }
    }
}

pub fn attachment_error() -> String {
    "One or more selected files could not be added. Check the files and try again.".into()
}

pub fn attachment_delivery_error(error: AttachmentDeliveryError) -> String {
    match error {
        AttachmentDeliveryError::ImageSizeLimit { display_name } => format!(
            "{display_name} exceeds the 10 MB image limit. Choose a smaller image before sending again."
        ),
        AttachmentDeliveryError::ImageCountLimit { display_name } => format!(
            "{display_name} crosses the 10-image limit. Remove an image before sending again."
        ),
        AttachmentDeliveryError::ImageTotalSizeLimit { display_name } => format!(
            "{display_name} crosses the 20 MB total image limit. Remove images or choose smaller images before sending again."
        ),
        AttachmentDeliveryError::AmbiguousFormat { display_name } => format!(
            "{display_name} has an image format Muniment cannot verify. Choose a PNG, JPEG, GIF, or WebP image before sending again."
        ),
        AttachmentDeliveryError::Storage(_) | AttachmentDeliveryError::InvalidStoredLength => {
            attachment_error()
        }
    }
}

pub fn prepared_pi_images(
    storage: &SharedStorage,
    run_id: &str,
) -> Result<Vec<PiImageContent>, String> {
    let mut storage = storage.lock().map_err(|_| attachment_error())?;
    let events = storage
        .journal
        .events(run_id)
        .map_err(|_| attachment_error())?;
    prepare_pi_images(&storage.cas, &events)
        .map_err(attachment_delivery_error)
        .map(|images| {
            images
                .into_iter()
                .map(|image| PiImageContent::new(image.data, image.mime_type))
                .collect()
        })
}

pub fn prepared_pi_prompt<'a>(
    storage: &SharedStorage,
    run_id: &str,
    prompt: &'a str,
) -> Result<PromptCommand<'a>, String> {
    prepared_pi_images(storage, run_id).map(|images| PromptCommand::with_images(prompt, images))
}

#[derive(Debug)]
pub enum PreparedPromptError {
    Start,
    SessionRoot,
    Binding,
    Journal,
}

#[allow(clippy::too_many_arguments)]
pub fn coordinate_prepared_prompt<T>(
    sink: &impl ChatEventSink,
    journal: &SharedStorage,
    projector: &mut ChatProjector,
    run_id: &str,
    seq: &mut u64,
    subject: Option<&str>,
    submit: impl FnOnce() -> Result<(T, PiSessionLocator, Vec<PiChatEvent>), PreparedPromptError>,
) -> Result<(T, Vec<PiChatEvent>), PreparedPromptError> {
    let (handle, locator, buffered_events) = submit()?;
    append_emit(
        sink,
        journal,
        projector,
        run_id,
        seq,
        "runtime.pi_session.bound",
        json!({"run_id": run_id, "locator": locator.as_str()}),
        subject,
    )
    .map_err(|_| PreparedPromptError::Journal)?;
    append_emit(
        sink,
        journal,
        projector,
        run_id,
        seq,
        "model.prompt.accepted",
        json!({}),
        subject,
    )
    .map_err(|_| PreparedPromptError::Journal)?;
    Ok((handle, buffered_events))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resume_attempt_reports_acceptance_and_every_early_return() {
        let (sender, receiver) = std::sync::mpsc::channel();
        drop(ResumeAttempt::new(Some(sender)));
        assert_eq!(
            receiver.recv().unwrap().unwrap_err(),
            "This reply could not be resumed. Try again."
        );

        let (sender, receiver) = std::sync::mpsc::channel();
        let mut attempt = ResumeAttempt::new(Some(sender));
        attempt.accepted();
        drop(attempt);
        assert_eq!(receiver.recv().unwrap(), Ok(()));
        assert!(receiver.try_recv().is_err());
    }

    #[test]
    fn delivery_errors_name_the_limit_and_recovery() {
        assert_eq!(
            attachment_delivery_error(AttachmentDeliveryError::ImageSizeLimit {
                display_name: "photo.jpg".into(),
            }),
            "photo.jpg exceeds the 10 MB image limit. Choose a smaller image before sending again."
        );
        assert_eq!(
            attachment_delivery_error(AttachmentDeliveryError::ImageCountLimit {
                display_name: "eleventh.png".into(),
            }),
            "eleventh.png crosses the 10-image limit. Remove an image before sending again."
        );
        assert_eq!(
            attachment_delivery_error(AttachmentDeliveryError::ImageTotalSizeLimit {
                display_name: "last.gif".into(),
            }),
            "last.gif crosses the 20 MB total image limit. Remove images or choose smaller images before sending again."
        );
        assert_eq!(
            attachment_delivery_error(AttachmentDeliveryError::AmbiguousFormat {
                display_name: "unclear.webp".into(),
            }),
            "unclear.webp has an image format Muniment cannot verify. Choose a PNG, JPEG, GIF, or WebP image before sending again."
        );
    }

    #[test]
    fn storage_and_stored_length_errors_keep_the_generic_message() {
        assert_eq!(
            attachment_delivery_error(AttachmentDeliveryError::Storage(
                crate::cas::CasError::InvalidHash("invalid".into()),
            )),
            attachment_error()
        );
        assert_eq!(
            attachment_delivery_error(AttachmentDeliveryError::InvalidStoredLength),
            attachment_error()
        );
    }
}

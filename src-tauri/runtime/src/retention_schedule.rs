//! Periodic application of the recorded retention choice.

use std::sync::{mpsc, mpsc::Receiver, mpsc::Sender, Arc};
use std::time::Duration;

use crate::RuntimeAttachState;

pub(crate) enum RetentionScheduleCommand {
    Recheck,
    Stop,
}

pub(crate) fn start_retention_schedule(
    state: Arc<RuntimeAttachState>,
    interval: Duration,
    commands: Receiver<RetentionScheduleCommand>,
    checked: Option<Sender<()>>,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        apply_recorded_retention(&state, checked.as_ref());
        while let Ok(RetentionScheduleCommand::Recheck) | Err(mpsc::RecvTimeoutError::Timeout) =
            commands.recv_timeout(interval)
        {
            apply_recorded_retention(&state, checked.as_ref());
        }
    })
}

fn apply_recorded_retention(state: &RuntimeAttachState, checked: Option<&Sender<()>>) {
    let _ = state.apply_recorded_retention();
    if let Some(checked) = checked {
        let _ = checked.send(());
    }
}

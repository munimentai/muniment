//! Periodic application of the recorded retention choice.

use std::sync::{mpsc, mpsc::Receiver, mpsc::Sender, Arc};
use std::time::Duration;

use crate::RuntimeAttachState;

pub(crate) enum RetentionScheduleCommand {
    // Only the Linux activation sends a recheck.
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
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
        let mut retention_checked_at = std::time::Instant::now();
        loop {
            if let Err(error) = state.check_agent_schedules() {
                eprintln!("agent_schedule: {error}");
            }
            match commands.recv_timeout(interval.min(Duration::from_secs(15))) {
                Ok(RetentionScheduleCommand::Stop) | Err(mpsc::RecvTimeoutError::Disconnected) => {
                    break
                }
                Ok(RetentionScheduleCommand::Recheck) => {
                    apply_recorded_retention(&state, checked.as_ref());
                    retention_checked_at = std::time::Instant::now();
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    if retention_checked_at.elapsed() >= interval {
                        apply_recorded_retention(&state, checked.as_ref());
                        retention_checked_at = std::time::Instant::now();
                    }
                }
            }
        }
    })
}

fn apply_recorded_retention(state: &RuntimeAttachState, checked: Option<&Sender<()>>) {
    let _ = state.apply_recorded_retention();
    if let Some(checked) = checked {
        let _ = checked.send(());
    }
}

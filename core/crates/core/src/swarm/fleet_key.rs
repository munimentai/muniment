//! Whether this runtime may ask its credential store for the fleet key.
//!
//! A store that refuses the key does not hear the request again on its own:
//! a declined system prompt waits for the person to retry or for the next
//! start, and any other failure waits out a backoff that doubles up to half
//! an hour. `swarm.members` shows the refusal with a retry action.

use std::time::{Duration, Instant};

use serde_json::{json, Value};

use crate::model_router::credentials::CredentialRefusal;

/// The first wait after a store failure.
const FIRST_BACKOFF: Duration = Duration::from_secs(30);
/// The longest wait after store failures.
const MAX_BACKOFF: Duration = Duration::from_secs(30 * 60);

/// The store's last refusal of the fleet key.
#[derive(Clone, Debug)]
struct Refusal {
    declined: bool,
    message: String,
    failures: u32,
    retry_at: Option<Instant>,
}

/// The gate in front of the fleet key's credential store.
#[derive(Debug, Default)]
pub(super) struct KeyGate {
    refusal: Option<Refusal>,
}

impl KeyGate {
    /// Whether the store may hear a request for the key at `now`.
    pub(super) fn may_ask(&self, now: Instant) -> bool {
        match &self.refusal {
            None => true,
            Some(refusal) => refusal.retry_at.is_some_and(|at| now >= at),
        }
    }

    /// The sentence a caller answers while the gate is closed.
    pub(super) fn closed_message(&self) -> String {
        match &self.refusal {
            Some(refusal) if refusal.declined => {
                "The keychain refused the fleet key. Choose Try again in Settings → Machines."
                    .into()
            }
            Some(refusal) => format!("The fleet key could not be read: {}", refusal.message),
            None => String::new(),
        }
    }

    /// Records a refusal at `now`. A declined prompt waits for a retry. A
    /// failure waits out a backoff that doubles with each failure in a row.
    pub(super) fn refused(&mut self, refusal: &CredentialRefusal, now: Instant) {
        let failures = self
            .refusal
            .as_ref()
            .map_or(0, |held| held.failures)
            .saturating_add(1);
        let declined = matches!(refusal, CredentialRefusal::Declined(_));
        let retry_at = (!declined).then(|| now + backoff(failures));
        self.refusal = Some(Refusal {
            declined,
            message: refusal.message().to_owned(),
            failures,
            retry_at,
        });
    }

    /// The store answered: the gate opens and forgets the refusals.
    pub(super) fn answered(&mut self) {
        self.refusal = None;
    }

    /// The person asked to try again: the gate opens.
    pub(super) fn retry(&mut self) {
        self.refusal = None;
    }

    /// What `swarm.members` shows: none while the store answers, or the
    /// refusal and whether it waits for a retry.
    pub(super) fn view(&self) -> Value {
        match &self.refusal {
            None => Value::Null,
            Some(refusal) => json!({
                "state": if refusal.declined { "declined" } else { "failed" },
                "message": refusal.message,
            }),
        }
    }
}

/// The wait after `failures` store failures in a row.
fn backoff(failures: u32) -> Duration {
    let doublings = failures.saturating_sub(1).min(16);
    FIRST_BACKOFF
        .saturating_mul(1 << doublings)
        .min(MAX_BACKOFF)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn declined() -> CredentialRefusal {
        CredentialRefusal::Declined("User canceled the operation.".into())
    }

    fn failed() -> CredentialRefusal {
        CredentialRefusal::Failed("The keychain is not available.".into())
    }

    #[test]
    fn an_open_gate_lets_the_store_answer() {
        let gate = KeyGate::default();
        assert!(gate.may_ask(Instant::now()));
        assert_eq!(gate.view(), Value::Null);
    }

    #[test]
    fn a_declined_prompt_waits_for_a_retry_however_long() {
        let now = Instant::now();
        let mut gate = KeyGate::default();
        gate.refused(&declined(), now);
        assert!(!gate.may_ask(now));
        assert!(!gate.may_ask(now + Duration::from_secs(365 * 24 * 3600)));
        assert_eq!(gate.view()["state"], "declined");
        assert!(gate.closed_message().contains("Try again"));
        gate.retry();
        assert!(gate.may_ask(now));
        assert_eq!(gate.view(), Value::Null);
    }

    #[test]
    fn a_store_failure_backs_off_and_doubles_to_a_cap() {
        let now = Instant::now();
        let mut gate = KeyGate::default();
        gate.refused(&failed(), now);
        assert!(!gate.may_ask(now + Duration::from_secs(29)));
        assert!(gate.may_ask(now + Duration::from_secs(30)));
        gate.refused(&failed(), now);
        assert!(!gate.may_ask(now + Duration::from_secs(59)));
        assert!(gate.may_ask(now + Duration::from_secs(60)));
        for _ in 0..40 {
            gate.refused(&failed(), now);
        }
        assert!(!gate.may_ask(now + MAX_BACKOFF - Duration::from_secs(1)));
        assert!(gate.may_ask(now + MAX_BACKOFF));
        assert_eq!(gate.view()["state"], "failed");
    }

    #[test]
    fn an_answer_forgets_the_refusals() {
        let now = Instant::now();
        let mut gate = KeyGate::default();
        gate.refused(&failed(), now);
        gate.refused(&failed(), now);
        gate.answered();
        assert!(gate.may_ask(now));
        gate.refused(&failed(), now);
        assert!(gate.may_ask(now + FIRST_BACKOFF));
    }

    #[test]
    fn a_decline_after_failures_still_waits_for_a_retry() {
        let now = Instant::now();
        let mut gate = KeyGate::default();
        gate.refused(&failed(), now);
        gate.refused(&declined(), now);
        assert!(!gate.may_ask(now + MAX_BACKOFF));
    }
}

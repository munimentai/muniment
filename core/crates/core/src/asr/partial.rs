//! Pacing for partial transcripts of the utterance in progress.
//!
//! The offline recognizer has no streaming mode, so a host decodes the audio
//! of the utterance so far again at an interval and shows the result until the
//! utterance's final transcript replaces it. Each pass reads the whole
//! utterance, so the interval follows the last decode time and the partial
//! decodes take a bounded share of the time.

use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PartialPacing {
    /// The shortest wait between the starts of two partial decodes.
    pub min_interval: Duration,
    /// The longest wait between the starts of two partial decodes.
    pub max_interval: Duration,
    /// The wait is at least this many times the last decode time, so partial
    /// decodes run at most one part in `load_factor` of the time.
    pub load_factor: u32,
    /// The new audio, in samples, a partial decode needs since the last one.
    pub min_new_samples: usize,
}

impl Default for PartialPacing {
    fn default() -> Self {
        Self {
            min_interval: Duration::from_millis(600),
            max_interval: Duration::from_millis(2_000),
            load_factor: 4,
            min_new_samples: 3_200,
        }
    }
}

/// Decides when the utterance in progress gets a partial decode and which
/// decoded text is new enough to show.
#[derive(Debug, Clone)]
pub struct PartialTranscripts {
    pacing: PartialPacing,
    interval: Duration,
    last_decode: Option<(Instant, usize)>,
    shown: String,
}

impl PartialTranscripts {
    pub fn new(pacing: PartialPacing) -> Self {
        Self {
            pacing,
            interval: pacing.min_interval,
            last_decode: None,
            shown: String::new(),
        }
    }

    /// The current wait between the starts of two partial decodes.
    pub fn interval(&self) -> Duration {
        self.interval
    }

    /// Whether the utterance in progress, `samples` long at `now`, is due a
    /// partial decode. The first decode of an utterance is due at once.
    pub fn due(&self, now: Instant, samples: usize) -> bool {
        match self.last_decode {
            None => samples > 0,
            Some((started, decoded)) => {
                samples >= decoded.saturating_add(self.pacing.min_new_samples)
                    && now.saturating_duration_since(started) >= self.interval
            }
        }
    }

    /// Records a partial decode that started at `started`, took `took` and
    /// read `samples`. Returns the text to show, or `None` when the host
    /// already shows that text.
    pub fn decoded(
        &mut self,
        started: Instant,
        took: Duration,
        samples: usize,
        text: String,
    ) -> Option<String> {
        let paced = took.saturating_mul(self.pacing.load_factor);
        self.interval = paced
            .min(self.pacing.max_interval)
            .max(self.pacing.min_interval);
        self.last_decode = Some((started, samples));
        if text == self.shown {
            return None;
        }
        self.shown.clone_from(&text);
        Some(text)
    }

    /// Ends the utterance in progress. Returns whether a partial text shows,
    /// so a host that drops the utterance without a final transcript clears it.
    pub fn ended(&mut self) -> bool {
        self.last_decode = None;
        !std::mem::take(&mut self.shown).is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pacing() -> PartialPacing {
        PartialPacing {
            min_interval: Duration::from_millis(500),
            max_interval: Duration::from_millis(1_500),
            load_factor: 4,
            min_new_samples: 100,
        }
    }

    #[test]
    fn the_first_decode_of_an_utterance_is_due_at_once() {
        let partials = PartialTranscripts::new(pacing());
        let now = Instant::now();
        assert!(!partials.due(now, 0));
        assert!(partials.due(now, 1));
    }

    #[test]
    fn a_later_decode_waits_for_the_interval_and_new_audio() {
        let mut partials = PartialTranscripts::new(pacing());
        let start = Instant::now();
        partials.decoded(start, Duration::from_millis(50), 1_000, "hello".into());
        assert_eq!(partials.interval(), Duration::from_millis(500));
        assert!(!partials.due(start + Duration::from_millis(499), 5_000));
        assert!(!partials.due(start + Duration::from_millis(500), 1_099));
        assert!(partials.due(start + Duration::from_millis(500), 1_100));
    }

    #[test]
    fn the_interval_follows_the_decode_time_within_its_bounds() {
        let mut partials = PartialTranscripts::new(pacing());
        let start = Instant::now();
        partials.decoded(start, Duration::from_millis(200), 1_000, "a".into());
        assert_eq!(partials.interval(), Duration::from_millis(800));
        partials.decoded(start, Duration::from_secs(2), 2_000, "a b".into());
        assert_eq!(partials.interval(), Duration::from_millis(1_500));
        partials.decoded(start, Duration::from_millis(10), 3_000, "a b c".into());
        assert_eq!(partials.interval(), Duration::from_millis(500));
        partials.decoded(start, Duration::MAX, 4_000, "a b c d".into());
        assert_eq!(partials.interval(), Duration::from_millis(1_500));
    }

    #[test]
    fn only_changed_text_is_shown() {
        let mut partials = PartialTranscripts::new(pacing());
        let start = Instant::now();
        let took = Duration::from_millis(10);
        assert_eq!(partials.decoded(start, took, 1, String::new()), None);
        assert_eq!(
            partials.decoded(start, took, 2, "hello".into()),
            Some("hello".into())
        );
        assert_eq!(partials.decoded(start, took, 3, "hello".into()), None);
        assert_eq!(
            partials.decoded(start, took, 4, "hello there".into()),
            Some("hello there".into())
        );
    }

    #[test]
    fn ending_an_utterance_reports_a_shown_partial_and_starts_over() {
        let mut partials = PartialTranscripts::new(pacing());
        let start = Instant::now();
        assert!(!partials.ended());
        partials.decoded(start, Duration::from_millis(300), 5_000, "hello".into());
        assert!(partials.ended());
        assert!(!partials.ended());
        assert!(partials.due(start, 1));
        // The decode time of the last utterance still paces the next one.
        assert_eq!(partials.interval(), Duration::from_millis(1_200));
        assert_eq!(
            partials.decoded(start, Duration::from_millis(10), 1, "hello".into()),
            Some("hello".into())
        );
    }
}

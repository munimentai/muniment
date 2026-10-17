//! The hybrid logical clock that orders record writes across members.
//!
//! A clock is a wall time in milliseconds plus a counter. A local write takes
//! the later of the wall clock and the last clock issued, and a received
//! clock moves this runtime's clock past it. A write made after seeing
//! another member's write therefore always orders after it, even when that
//! member's wall clock runs a day ahead.

use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// One hybrid logical clock reading. Readings order by wall time, then
/// counter.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Hlc {
    pub millis: u64,
    pub counter: u16,
}

impl Hlc {
    /// The reading packed into one integer that sorts the same way.
    pub fn packed(self) -> i64 {
        ((self.millis << 16) | u64::from(self.counter)) as i64
    }

    pub fn from_packed(packed: i64) -> Self {
        let packed = packed.max(0) as u64;
        Self {
            millis: packed >> 16,
            counter: (packed & 0xffff) as u16,
        }
    }

    /// The reading for a wall time in milliseconds, with no counter.
    pub fn at(millis: i64) -> Self {
        Self {
            millis: millis.max(0) as u64,
            counter: 0,
        }
    }

    /// The next reading after this one at the same wall time.
    fn bump(self) -> Self {
        match self.counter.checked_add(1) {
            Some(counter) => Self {
                millis: self.millis,
                counter,
            },
            None => Self {
                millis: self.millis + 1,
                counter: 0,
            },
        }
    }

    /// The wall time of the reading.
    pub fn time(self) -> chrono::DateTime<chrono::Utc> {
        chrono::DateTime::from_timestamp_millis(self.millis as i64).unwrap_or_default()
    }
}

impl fmt::Display for Hlc {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{:012x}{:04x}", self.millis, self.counter)
    }
}

impl std::str::FromStr for Hlc {
    type Err = String;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        if text.len() != 16 || !text.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err("The clock is invalid.".into());
        }
        let millis = u64::from_str_radix(&text[..12], 16).map_err(|error| error.to_string())?;
        let counter = u16::from_str_radix(&text[12..], 16).map_err(|error| error.to_string())?;
        Ok(Self { millis, counter })
    }
}

impl Serialize for Hlc {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Hlc {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}

/// The clock of one runtime: the last reading it issued or saw.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HlcClock {
    last: Hlc,
}

/// A received clock this far past the wall clock gets a log line.
pub const SKEW_WARNING_MILLIS: u64 = 60 * 60 * 1000;

impl HlcClock {
    pub fn new(last: Hlc) -> Self {
        Self { last }
    }

    pub fn last(&self) -> Hlc {
        self.last
    }

    /// A reading for a local write at wall time `now_millis`, after every
    /// reading issued or seen before.
    pub fn tick(&mut self, now_millis: i64) -> Hlc {
        let wall = Hlc::at(now_millis);
        self.last = if wall > self.last {
            wall
        } else {
            self.last.bump()
        };
        self.last
    }

    /// Takes in a reading another member issued, so later local writes order
    /// after it. Answers how far it runs past the wall clock, in
    /// milliseconds.
    pub fn observe(&mut self, remote: Hlc, now_millis: i64) -> u64 {
        if remote > self.last {
            self.last = remote;
        }
        remote.millis.saturating_sub(now_millis.max(0) as u64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: i64 = 24 * 60 * 60 * 1000;
    const NOW: i64 = 1_790_000_000_000;

    #[test]
    fn readings_pack_print_and_parse_in_order() {
        let early = Hlc {
            millis: NOW as u64,
            counter: 7,
        };
        let late = Hlc {
            millis: NOW as u64 + 1,
            counter: 0,
        };
        assert!(early < late);
        assert!(early.packed() < late.packed());
        assert!(early.to_string() < late.to_string());
        assert_eq!(Hlc::from_packed(early.packed()), early);
        assert_eq!(early.to_string().parse::<Hlc>().unwrap(), early);
        let json = serde_json::to_string(&early).unwrap();
        assert_eq!(serde_json::from_str::<Hlc>(&json).unwrap(), early);
        assert!("xyz".parse::<Hlc>().is_err());
    }

    #[test]
    fn local_writes_order_even_when_the_wall_clock_stalls_or_steps_back() {
        let mut clock = HlcClock::default();
        let first = clock.tick(NOW);
        let second = clock.tick(NOW);
        let third = clock.tick(NOW - 5_000);
        assert!(first < second && second < third);
        assert_eq!(third.millis, NOW as u64);
        let mut saturated = HlcClock::new(Hlc {
            millis: NOW as u64,
            counter: u16::MAX,
        });
        let next = saturated.tick(NOW);
        assert_eq!(next.millis, NOW as u64 + 1);
    }

    #[test]
    fn a_member_a_day_ahead_still_orders_after_what_it_saw() {
        let mut ahead = HlcClock::default();
        let mut behind = HlcClock::default();
        let fast_write = ahead.tick(NOW + DAY);
        // A write that saw the fast write orders after it, although its own
        // wall clock is a day behind.
        let skew = behind.observe(fast_write, NOW);
        assert_eq!(skew, DAY as u64);
        let reply = behind.tick(NOW + 1);
        assert!(reply > fast_write);
        // And the fast member's next write orders after the reply.
        ahead.observe(reply, NOW + DAY + 2);
        assert!(ahead.tick(NOW + DAY + 2) > reply);
    }

    #[test]
    fn a_clock_restored_after_a_restart_keeps_ordering() {
        let mut clock = HlcClock::default();
        clock.observe(
            Hlc {
                millis: (NOW + DAY) as u64,
                counter: 3,
            },
            NOW,
        );
        let before = clock.tick(NOW);
        // The runtime keeps `last` and opens the clock from it again; the
        // wall clock reads earlier than everything issued.
        let mut restored = HlcClock::new(Hlc::from_packed(before.packed()));
        let after = restored.tick(NOW - DAY);
        assert!(after > before);
    }
}

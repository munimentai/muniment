//! How clock times read for the user: in this computer's time zone or one the
//! user picks, on a 12- or 24-hour clock. The `time` preference holds the
//! choice on each machine, since it belongs to the screen the user reads.
//! Words name no time zone. A schedule keeps its own zone for firing, and
//! only its words follow this choice.

use std::path::Path;

use chrono::{DateTime, Datelike, NaiveDate, TimeZone, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The preference key that holds the choice.
pub const TIME_KEY: &str = "time";

/// A clock style.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Clock {
    #[serde(rename = "12h")]
    Twelve,
    #[serde(rename = "24h")]
    TwentyFour,
}

/// The user's choice. An empty field matches this computer.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct TimeDisplay {
    /// An IANA time zone, or none for this computer's.
    pub zone: Option<String>,
    /// A clock, or none for the clock of this computer's locale.
    pub clock: Option<Clock>,
    /// The clock of this computer's locale, as the client reads it. Core
    /// cannot read the locale, so a missing value reads as 24-hour.
    pub computer_clock: Option<Clock>,
}

/// Checks a stored `time` value.
pub fn check(value: &Value) -> Result<(), String> {
    let invalid = || "The time preference is invalid.".to_owned();
    let display: TimeDisplay = serde_json::from_value(value.clone()).map_err(|_| invalid())?;
    if let Some(zone) = &display.zone {
        crate::agent_routines::parse_time_zone(zone).map_err(|_| invalid())?;
    }
    Ok(())
}

/// The choice this machine keeps, or the default when it holds none.
pub fn load(profile: &Path) -> TimeDisplay {
    crate::preferences::values(profile)
        .ok()
        .and_then(|values| values.get(TIME_KEY).cloned())
        .and_then(|value| serde_json::from_value(value).ok())
        .unwrap_or_default()
}

impl TimeDisplay {
    /// The zone times read in: the chosen zone, else this computer's.
    pub fn zone(&self) -> chrono_tz::Tz {
        self.zone
            .as_deref()
            .and_then(|zone| crate::agent_routines::parse_time_zone(zone).ok())
            .or_else(|| {
                crate::agent_routines::parse_time_zone(&crate::agent_routines::local_time_zone())
                    .ok()
            })
            .unwrap_or(chrono_tz::UTC)
    }

    /// Whether times read on a 12-hour clock.
    pub fn twelve_hour(&self) -> bool {
        self.clock.or(self.computer_clock) == Some(Clock::Twelve)
    }

    /// An hour and minute of the day: `8:05 AM` or `08:05`.
    pub fn wall(&self, hour: u32, minute: u32) -> String {
        if self.twelve_hour() {
            let half = if hour < 12 { "AM" } else { "PM" };
            let hour = match hour % 12 {
                0 => 12,
                hour => hour,
            };
            format!("{hour}:{minute:02} {half}")
        } else {
            format!("{hour:02}:{minute:02}")
        }
    }

    /// The clock time of `at` in the chosen zone.
    pub fn clock_time(&self, at: DateTime<Utc>) -> String {
        use chrono::Timelike;
        let local = at.with_timezone(&self.zone());
        self.wall(local.hour(), local.minute())
    }

    /// The day and clock time of `at`: `Oct 20, 8:50 AM`.
    pub fn day_and_time(&self, at: DateTime<Utc>) -> String {
        let local = at.with_timezone(&self.zone());
        format!("{}, {}", local.format("%b %-d"), self.clock_time(at))
    }

    /// A schedule's hour and minute in the zone `tz`, read in the chosen
    /// zone on the schedule zone's date at `now`: the clock time and how
    /// many days the chosen zone's date runs ahead of the schedule's, from
    /// -1 to 1. An unknown `tz` reads as the chosen zone.
    pub fn schedule_time(
        &self,
        hour: u32,
        minute: u32,
        tz: &str,
        now: DateTime<Utc>,
    ) -> (String, i64) {
        let zone = self.zone();
        let Ok(from) = crate::agent_routines::parse_time_zone(tz) else {
            return (self.wall(hour, minute), 0);
        };
        if from == zone {
            return (self.wall(hour, minute), 0);
        }
        let today = now.with_timezone(&from).date_naive();
        // A time that a clock change skips reads on the next day.
        let instant = (0..3).find_map(|offset| {
            let date: NaiveDate = today + chrono::Duration::days(offset);
            let naive = date.and_hms_opt(hour, minute, 0)?;
            from.from_local_datetime(&naive)
                .earliest()
                .map(|at| (date, at.with_timezone(&Utc)))
        });
        let Some((date, at)) = instant else {
            return (self.wall(hour, minute), 0);
        };
        let shown = at.with_timezone(&zone).date_naive();
        let shift = i64::from(shown.num_days_from_ce() - date.num_days_from_ce()).clamp(-1, 1);
        (self.clock_time(at), shift)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn display(zone: &str, clock: Clock) -> TimeDisplay {
        TimeDisplay {
            zone: Some(zone.into()),
            clock: Some(clock),
            computer_clock: None,
        }
    }

    #[test]
    fn clock_times_follow_the_clock_and_the_zone() {
        let at = Utc.with_ymd_and_hms(2026, 10, 20, 12, 50, 0).unwrap();
        let twelve = display("America/New_York", Clock::Twelve);
        let twenty_four = display("Europe/Paris", Clock::TwentyFour);
        assert_eq!(twelve.clock_time(at), "8:50 AM");
        assert_eq!(twenty_four.clock_time(at), "14:50");
        assert_eq!(twelve.day_and_time(at), "Oct 20, 8:50 AM");
        assert_eq!(twenty_four.day_and_time(at), "Oct 20, 14:50");
        assert_eq!(twelve.wall(0, 5), "12:05 AM");
        assert_eq!(twelve.wall(12, 0), "12:00 PM");
        assert_eq!(twenty_four.wall(8, 0), "08:00");
    }

    #[test]
    fn an_unset_clock_follows_the_computers_clock_else_24_hours() {
        let mut time = TimeDisplay::default();
        assert!(!time.twelve_hour());
        time.computer_clock = Some(Clock::Twelve);
        assert!(time.twelve_hour());
        time.clock = Some(Clock::TwentyFour);
        assert!(!time.twelve_hour());
    }

    #[test]
    fn a_schedule_reads_in_the_chosen_zone_with_its_day_shift() {
        let now = Utc.with_ymd_and_hms(2026, 10, 20, 12, 0, 0).unwrap();
        let paris = display("Europe/Paris", Clock::TwentyFour);
        assert_eq!(
            paris.schedule_time(8, 0, "Europe/Paris", now),
            ("08:00".into(), 0)
        );
        assert_eq!(
            paris.schedule_time(8, 0, "America/New_York", now),
            ("14:00".into(), 0)
        );
        assert_eq!(
            paris.schedule_time(22, 30, "America/New_York", now),
            ("04:30".into(), 1)
        );
        let new_york = display("America/New_York", Clock::Twelve);
        assert_eq!(
            new_york.schedule_time(1, 0, "Europe/Paris", now),
            ("7:00 PM".into(), -1)
        );
        assert_eq!(
            new_york.schedule_time(9, 0, "Not/AZone", now),
            ("9:00 AM".into(), 0)
        );
    }

    #[test]
    fn the_preference_checks_its_zone_and_clock() {
        assert!(check(&json!({"zone": "Asia/Tokyo", "clock": "12h"})).is_ok());
        assert!(check(&json!({"computer_clock": "24h"})).is_ok());
        assert!(check(&json!({})).is_ok());
        assert!(check(&json!({"zone": "Mars/Base"})).is_err());
        assert!(check(&json!({"clock": "13h"})).is_err());
        assert!(check(&json!("24h")).is_err());
    }

    #[test]
    fn the_choice_stays_on_this_machine() {
        let profile = std::env::temp_dir().join(uuid::Uuid::new_v4().to_string());
        std::fs::create_dir_all(&profile).unwrap();
        assert_eq!(load(&profile), TimeDisplay::default());
        assert!(!crate::preferences::is_fleet_key(TIME_KEY));
        crate::preferences::operation(
            &profile,
            "preferences.set",
            json!({"values": {"time": {"zone": "Asia/Tokyo", "clock": "12h"}}}),
        )
        .unwrap();
        assert_eq!(load(&profile), display("Asia/Tokyo", Clock::Twelve));
        assert!(crate::preferences::operation(
            &profile,
            "preferences.set",
            json!({"values": {"time": {"zone": "Nowhere"}}}),
        )
        .is_err());
        std::fs::remove_dir_all(profile).unwrap();
    }
}

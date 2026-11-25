use serde::{Deserialize, Serialize};
use std::fmt;

/// Simulation time.
///
/// Stored as a count of seconds since the simulation epoch. Conversion
/// helpers are deterministic and allocation-free.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
pub struct SimTime {
    pub secs: u64,
}

impl SimTime {
    pub const EPOCH: SimTime = SimTime { secs: 0 };

    pub fn from_secs(secs: u64) -> Self {
        Self { secs }
    }
    pub fn from_days(days: u64) -> Self {
        Self {
            secs: days * 86_400,
        }
    }
    pub fn from_hours(hours: u64) -> Self {
        Self {
            secs: hours * 3_600,
        }
    }
    pub fn from_mins(mins: u64) -> Self {
        Self { secs: mins * 60 }
    }

    pub fn as_secs(self) -> u64 {
        self.secs
    }
    pub fn as_mins(self) -> u64 {
        self.secs / 60
    }
    pub fn as_hours(self) -> u64 {
        self.secs / 3_600
    }
    pub fn as_days(self) -> u64 {
        self.secs / 86_400
    }

    pub fn hour_of_day(self) -> u32 {
        ((self.secs / 3_600) % 24) as u32
    }
    pub fn minute_of_day(self) -> u32 {
        ((self.secs / 60) % 1440) as u32
    }
    pub fn day_of_year(self) -> u32 {
        ((self.secs / 86_400) % 365) as u32
    }

    pub fn add_secs(self, secs: u64) -> Self {
        Self {
            secs: self.secs + secs,
        }
    }
    pub fn add_mins(self, mins: u64) -> Self {
        self.add_secs(mins * 60)
    }
    pub fn add_hours(self, hours: u64) -> Self {
        self.add_secs(hours * 3_600)
    }
    pub fn add_days(self, days: u64) -> Self {
        self.add_secs(days * 86_400)
    }
}

impl fmt::Display for SimTime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let d = self.as_days();
        let h = self.hour_of_day();
        // Minute within the hour (minute_of_day() is minutes since midnight, 0..1439).
        let m = (self.secs / 60) % 60;
        write!(f, "Day {d:04} {h:02}:{m:02}")
    }
}

/// Configurable simulation speed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TimeSpeed {
    Paused,
    RealTime,
    FastForward,
    Extreme,
    EventDriven,
}

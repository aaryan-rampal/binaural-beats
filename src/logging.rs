//! Per-run log file under `logs/`, using only the standard library.
//!
//! The TUI owns the terminal while running, so `main` prints the outcome and log path after exit.

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Message severity, ordered from least to most severe.
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
pub enum Level {
    Debug,
    Info,
    Warn,
    Error,
}

impl Level {
    /// Reads a `LOG_LEVEL` value, defaulting to `Info` when unset.
    fn from_setting(setting: Option<&str>) -> Result<Self, String> {
        match setting {
            None | Some("INFO") => Ok(Self::Info),
            Some("DEBUG") => Ok(Self::Debug),
            Some("WARN") => Ok(Self::Warn),
            Some("ERROR") => Ok(Self::Error),
            Some(other) => Err(format!(
                "LOG_LEVEL is `{other}`; set it to DEBUG, INFO, WARN or ERROR"
            )),
        }
    }
}

/// Append-only writer for one run's log file.
pub struct Logger {
    file: File,
    path: PathBuf,
    threshold: Level,
}

impl Logger {
    /// Creates `dir` if needed and opens a log file named with the current UTC time.
    pub fn create(dir: &Path) -> Result<Self, String> {
        let level = std::env::var("LOG_LEVEL").ok();
        let threshold = Level::from_setting(level.as_deref())?;
        fs::create_dir_all(dir).map_err(|error| {
            format!(
                "creating log directory {}: {error}; run from a writable folder",
                dir.display()
            )
        })?;
        let name = format!(
            "binaural-{}.log",
            timestamp(now_seconds()).replace(':', "-")
        );
        let path = dir.join(name);
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(|error| {
                format!(
                    "opening log file {}: {error}; run from a writable folder",
                    path.display()
                )
            })?;
        Ok(Self {
            file,
            path,
            threshold,
        })
    }

    /// Where this run's log is written.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Appends `message` when `level` meets the configured threshold.
    pub fn log(&mut self, level: Level, message: &str) -> Result<(), String> {
        if level < self.threshold {
            return Ok(());
        }
        let stamp = timestamp(now_seconds());
        writeln!(self.file, "{stamp} {level:?} {message}").map_err(|error| {
            format!(
                "writing log file {}: {error}; check free disk space",
                self.path.display()
            )
        })
    }
}

/// Seconds since the Unix epoch. A clock before 1970 is treated as the epoch itself.
fn now_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs() as i64)
}

/// Formats Unix seconds as an ISO 8601 UTC timestamp.
fn timestamp(seconds: i64) -> String {
    let (year, month, day) = civil_from_days(seconds.div_euclid(86_400));
    let second_of_day = seconds.rem_euclid(86_400);
    let (hour, minute, second) = (
        second_of_day / 3600,
        second_of_day % 3600 / 60,
        second_of_day % 60,
    );
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

/// Converts days since the Unix epoch to a date, using Howard Hinnant's `civil_from_days`.
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let shifted = days + 719_468;
    let era = shifted.div_euclid(146_097);
    let day_of_era = shifted.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * shifted_month + 2) / 5 + 1;
    let month = if shifted_month < 10 {
        shifted_month + 3
    } else {
        shifted_month - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

#[cfg(test)]
mod tests {
    use crate::logging::{Level, timestamp};

    #[test]
    fn timestamps_match_known_dates() {
        assert_eq!(timestamp(0), "1970-01-01T00:00:00Z");
        assert_eq!(timestamp(19_782 * 86_400), "2024-02-29T00:00:00Z");
        assert_eq!(timestamp(951_782_400 + 3661), "2000-02-29T01:01:01Z");
    }

    #[test]
    fn log_level_setting_defaults_to_info_and_rejects_typos() {
        assert_eq!(Level::from_setting(None), Ok(Level::Info));
        assert_eq!(Level::from_setting(Some("DEBUG")), Ok(Level::Debug));
        assert!(
            Level::from_setting(Some("verbose"))
                .unwrap_err()
                .contains("`verbose`")
        );
    }
}

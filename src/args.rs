//! Command-line parsing.

use std::ops::RangeInclusive;

use crate::app::{BEAT_RANGE, CARRIER_RANGE, Settings, VOLUME_RANGE};

pub const USAGE: &str = "\
binaural: binaural beat generator for the terminal (use headphones)

Usage: binaural [--carrier HZ] [--beat HZ] [--volume PCT]

Options:
  --carrier HZ   Tone midway between the ears, 50 to 1000 (default 150)
  --beat HZ      Difference between the ears, 0.5 to 50 (default 10)
  --volume PCT   Volume, 0 to 100 (default 25)
  -h, --help     Show this help

Environment:
  LOG_LEVEL      DEBUG, INFO, WARN or ERROR (default INFO); logs go to ./logs/";

/// What the command line asks for.
#[derive(Debug, PartialEq)]
pub enum Command {
    Run(Settings),
    Help,
}

/// Parses arguments, excluding the program name.
pub fn parse(mut args: impl Iterator<Item = String>) -> Result<Command, String> {
    let mut settings = Settings::default();
    while let Some(flag) = args.next() {
        match flag.as_str() {
            "-h" | "--help" => return Ok(Command::Help),
            "--carrier" => settings.carrier_hz = value(&flag, args.next(), &CARRIER_RANGE)?,
            "--beat" => settings.beat_hz = value(&flag, args.next(), &BEAT_RANGE)?,
            "--volume" => {
                settings.volume_pct = value(&flag, args.next(), &VOLUME_RANGE)?.round() as u8;
            }
            other => {
                return Err(format!(
                    "unknown argument `{other}`; see --help for options"
                ));
            }
        }
    }
    Ok(Command::Run(settings))
}

/// Parses the value after `flag`, requiring it to be a number within `range`.
fn value(flag: &str, raw: Option<String>, range: &RangeInclusive<f32>) -> Result<f32, String> {
    let (start, end) = (range.start(), range.end());
    let raw = raw.ok_or_else(|| format!("{flag} needs a value from {start} to {end}"))?;
    match raw.parse::<f32>() {
        Ok(number) if range.contains(&number) => Ok(number),
        _ => Err(format!(
            "{flag} got `{raw}`; use a number from {start} to {end}"
        )),
    }
}

#[cfg(test)]
mod tests {
    use crate::app::Settings;
    use crate::args::{Command, parse};

    /// Parses a space-separated argument string.
    fn parse_str(args: &str) -> Result<Command, String> {
        parse(args.split_whitespace().map(String::from))
    }

    #[test]
    fn no_arguments_uses_defaults() {
        assert_eq!(parse_str(""), Ok(Command::Run(Settings::default())));
    }

    #[test]
    fn flags_set_values() {
        let expected = Settings {
            carrier_hz: 432.0,
            beat_hz: 7.83,
            volume_pct: 10,
        };
        let parsed = parse_str("--carrier 432 --beat 7.83 --volume 10");
        assert_eq!(parsed, Ok(Command::Run(expected)));
    }

    #[test]
    fn help_is_recognized() {
        assert_eq!(parse_str("--help"), Ok(Command::Help));
        assert_eq!(parse_str("-h"), Ok(Command::Help));
    }

    #[test]
    fn bad_input_names_the_flag_and_range() {
        for (input, expected) in [
            ("--beat 99", "--beat got `99`; use a number from 0.5 to 50"),
            (
                "--beat NaN",
                "--beat got `NaN`; use a number from 0.5 to 50",
            ),
            (
                "--carrier abc",
                "--carrier got `abc`; use a number from 50 to 1000",
            ),
            ("--volume", "--volume needs a value from 0 to 100"),
            (
                "--nope",
                "unknown argument `--nope`; see --help for options",
            ),
        ] {
            assert_eq!(parse_str(input), Err(expected.to_string()), "{input}");
        }
    }
}

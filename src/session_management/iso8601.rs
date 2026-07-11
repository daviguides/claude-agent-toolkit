//! Minimal ISO-8601 timestamp parsing/formatting — hand-rolled rather
//! than pulling in a datetime crate, since this crate only ever needs
//! whole-millisecond UTC timestamps in the exact shape the CLI writes
//! (`YYYY-MM-DDTHH:MM:SS.sssZ` or a numeric `+HH:MM`/`-HH:MM` offset).

use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Converts a [`Duration`] (as returned by
/// `SystemTime::duration_since`) to whole Unix epoch milliseconds.
#[must_use]
#[allow(
    clippy::cast_possible_truncation,
    reason = "milliseconds since the epoch fits in an i64 until the year 292 million"
)]
pub(crate) fn duration_to_millis(duration: Duration) -> i64 {
    duration.as_millis() as i64
}

/// The current time in Unix epoch milliseconds.
///
/// # Panics
///
/// Panics if the system clock reports a time before the Unix epoch.
#[must_use]
pub(crate) fn now_millis() -> i64 {
    duration_to_millis(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock is before the Unix epoch"),
    )
}

/// The current time, formatted as `YYYY-MM-DDTHH:MM:SS.sssZ`.
///
/// # Panics
///
/// Panics if the system clock reports a time before the Unix epoch.
#[must_use]
pub(crate) fn iso_now() -> String {
    format_iso8601(now_millis())
}

/// Formats `epoch_millis` as `YYYY-MM-DDTHH:MM:SS.sssZ`.
#[must_use]
pub(crate) fn format_iso8601(epoch_millis: i64) -> String {
    let total_seconds = epoch_millis.div_euclid(1000);
    let millis = epoch_millis.rem_euclid(1000);
    let days = total_seconds.div_euclid(86_400);
    let seconds_of_day = total_seconds.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    let hour = seconds_of_day / 3600;
    let minute = (seconds_of_day % 3600) / 60;
    let second = seconds_of_day % 60;
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{millis:03}Z")
}

/// Parses an ISO-8601 timestamp (`Z` or a numeric offset) into Unix
/// epoch milliseconds. Returns `None` for anything that doesn't match
/// the expected shape rather than erroring — timestamps are advisory
/// metadata, not load-bearing for correctness.
#[must_use]
pub(crate) fn parse_iso8601_ms(timestamp: &str) -> Option<i64> {
    let normalized = timestamp.replacen('Z', "+00:00", 1);
    let (date_part, time_part) = normalized.split_once('T')?;
    let mut ymd = date_part.split('-');
    let year: i64 = ymd.next()?.parse().ok()?;
    let month: i64 = ymd.next()?.parse().ok()?;
    let day: i64 = ymd.next()?.parse().ok()?;

    let sign_index = time_part.rfind(['+', '-']);
    let (clock_part, offset_part) = match sign_index {
        Some(index) if index > 0 => time_part.split_at(index),
        _ => (time_part, "+00:00"),
    };
    let mut hms = clock_part.split(':');
    let hour: i64 = hms.next()?.parse().ok()?;
    let minute: i64 = hms.next()?.parse().ok()?;
    let second_str = hms.next()?;
    let (second, millis) = match second_str.split_once('.') {
        Some((sec, frac)) => {
            let sec: i64 = sec.parse().ok()?;
            let frac_ms: i64 = format!("{frac:0<3}")[..3].parse().ok()?;
            (sec, frac_ms)
        }
        None => (second_str.parse().ok()?, 0),
    };

    let offset_minutes = parse_offset_minutes(offset_part)?;
    let days = days_from_civil(year, month, day);
    let epoch_seconds = days * 86_400 + hour * 3600 + minute * 60 + second - offset_minutes * 60;
    Some(epoch_seconds * 1000 + millis)
}

fn parse_offset_minutes(offset: &str) -> Option<i64> {
    if offset.is_empty() {
        return Some(0);
    }
    let sign = if offset.starts_with('-') { -1 } else { 1 };
    let digits = &offset[1..];
    let mut parts = digits.split(':');
    let hours: i64 = parts.next()?.parse().ok()?;
    let minutes: i64 = parts.next().unwrap_or("0").parse().ok()?;
    Some(sign * (hours * 60 + minutes))
}

/// Days since the Unix epoch for a Gregorian calendar date (Howard
/// Hinnant's `days_from_civil` algorithm).
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Inverse of [`days_from_civil`] (Howard Hinnant's `civil_from_days`).
fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_and_format_round_trip_for_known_epoch() {
        let formatted = format_iso8601(0);
        assert_eq!(formatted, "1970-01-01T00:00:00.000Z");
        assert_eq!(parse_iso8601_ms(&formatted), Some(0));
    }

    #[test]
    fn parses_z_suffix_timestamp() {
        assert_eq!(
            parse_iso8601_ms("2024-01-15T10:30:00.000Z"),
            Some(1_705_314_600_000)
        );
    }

    #[test]
    fn parses_numeric_offset_timestamp() {
        // 2024-01-15T10:30:00+02:00 == 2024-01-15T08:30:00Z
        assert_eq!(
            parse_iso8601_ms("2024-01-15T10:30:00+02:00"),
            parse_iso8601_ms("2024-01-15T08:30:00Z")
        );
    }

    #[test]
    fn rejects_malformed_timestamp() {
        assert_eq!(parse_iso8601_ms("not a timestamp"), None);
    }

    #[test]
    fn format_then_parse_round_trips_for_arbitrary_millis() {
        let millis: i64 = 1_800_000_000_123;
        assert_eq!(parse_iso8601_ms(&format_iso8601(millis)), Some(millis));
    }

    #[test]
    fn iso_now_produces_a_parseable_recent_timestamp() {
        let now = iso_now();
        let parsed = parse_iso8601_ms(&now).expect("parses");
        assert!((now_millis() - parsed).abs() < 5000);
    }
}

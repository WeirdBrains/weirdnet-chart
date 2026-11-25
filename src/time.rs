//! Timestamps, fixed-offset time zones and calendar helpers for time axes.

/// Milliseconds since the Unix epoch (1970-01-01T00:00:00Z).
pub type Timestamp = i64;

/// One second in milliseconds.
pub const SECOND: i64 = 1_000;
/// One minute in milliseconds.
pub const MINUTE: i64 = 60 * SECOND;
/// One hour in milliseconds.
pub const HOUR: i64 = 60 * MINUTE;
/// One day in milliseconds.
pub const DAY: i64 = 24 * HOUR;

/// The time zone used for axis labels and tick alignment.
///
/// Charts don't carry a time-zone database: pass the offset that applies to the
/// visible range (for example from your platform or a tz crate). Ticks land on
/// whole local hours, days and months for that offset.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TimeZone {
    /// Coordinated Universal Time.
    #[default]
    Utc,
    /// A fixed offset from UTC in seconds (east is positive).
    Fixed(i32),
}

impl TimeZone {
    /// The offset from UTC in milliseconds.
    pub fn offset_ms(self) -> i64 {
        match self {
            TimeZone::Utc => 0,
            TimeZone::Fixed(secs) => secs as i64 * SECOND,
        }
    }

    /// The local wall-clock time of `t`, as milliseconds since the local epoch.
    pub fn to_local(self, t: Timestamp) -> i64 {
        t + self.offset_ms()
    }

    /// The UTC timestamp of local wall-clock milliseconds `local`.
    pub fn from_local(self, local: i64) -> Timestamp {
        local - self.offset_ms()
    }
}

/// A broken-down local date and time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DateTime {
    /// Year, for example 2026.
    pub year: i32,
    /// Month, 1 to 12.
    pub month: u32,
    /// Day of the month, 1 to 31.
    pub day: u32,
    /// Hour, 0 to 23.
    pub hour: u32,
    /// Minute, 0 to 59.
    pub minute: u32,
    /// Second, 0 to 59.
    pub second: u32,
    /// Millisecond, 0 to 999.
    pub millis: u32,
}

impl DateTime {
    /// Breaks `t` down in time zone `tz`.
    pub fn from_timestamp(t: Timestamp, tz: TimeZone) -> DateTime {
        let local = tz.to_local(t);
        let days = local.div_euclid(DAY);
        let ms = local.rem_euclid(DAY);
        let (year, month, day) = civil_from_days(days);
        DateTime {
            year,
            month,
            day,
            hour: (ms / HOUR) as u32,
            minute: (ms % HOUR / MINUTE) as u32,
            second: (ms % MINUTE / SECOND) as u32,
            millis: (ms % SECOND) as u32,
        }
    }

    /// The UTC timestamp of this local date and time in `tz`.
    pub fn to_timestamp(self, tz: TimeZone) -> Timestamp {
        let days = days_from_civil(self.year, self.month, self.day);
        let local = days * DAY + self.hour as i64 * HOUR + self.minute as i64 * MINUTE + self.second as i64 * SECOND + self.millis as i64;
        tz.from_local(local)
    }
}

/// Days since 1970-01-01 for a proleptic Gregorian date.
pub fn days_from_civil(year: i32, month: u32, day: u32) -> i64 {
    let y = year as i64 - i64::from(month <= 2);
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let m = month as i64;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + day as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The proleptic Gregorian date `(year, month, day)` of a day count since 1970-01-01.
pub fn civil_from_days(days: i64) -> (i32, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = (yoe + era * 400 + i64::from(month <= 2)) as i32;
    (year, month, day)
}

pub(crate) const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn civil_round_trip() {
        for days in [-800_000, -1, 0, 1, 59, 10_957, 20_782, 2_000_000] {
            let (y, m, d) = civil_from_days(days);
            assert_eq!(days_from_civil(y, m, d), days);
        }
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(days_from_civil(2000, 3, 1), 11_017);
    }

    #[test]
    fn breaks_down_with_offset() {
        // 2026-11-25T20:44:05.250Z
        let t = DateTime { year: 2026, month: 11, day: 25, hour: 20, minute: 44, second: 5, millis: 250 }.to_timestamp(TimeZone::Utc);
        let local = DateTime::from_timestamp(t, TimeZone::Fixed(-5 * 3600));
        assert_eq!((local.day, local.hour, local.minute, local.second, local.millis), (25, 15, 44, 5, 250));
        let east = DateTime::from_timestamp(t, TimeZone::Fixed(9 * 3600));
        assert_eq!((east.day, east.hour), (26, 5));
        assert_eq!(east.to_timestamp(TimeZone::Fixed(9 * 3600)), t);
    }
}

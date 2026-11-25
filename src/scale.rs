//! Scales and axis ticks: linear value scales, "nice" numeric ticks and calendar-aware time ticks.

use crate::time::{DAY, DateTime, HOUR, MINUTE, MONTHS, SECOND, TimeZone, Timestamp};

/// Maps a value domain linearly onto an output range (pixels, dots or cells).
///
/// The range may be inverted, as a y axis usually is (`range: (bottom, top)`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LinearScale {
    /// The input interval `(from, to)`.
    pub domain: (f64, f64),
    /// The output interval `(from, to)`.
    pub range: (f64, f64),
}

impl LinearScale {
    /// A scale from `domain` onto `range`.
    pub fn new(domain: (f64, f64), range: (f64, f64)) -> LinearScale {
        LinearScale { domain, range }
    }

    /// Maps a domain value to the range. A zero-width domain maps to the middle of the range.
    pub fn map(&self, v: f64) -> f64 {
        let (d0, d1) = self.domain;
        let (r0, r1) = self.range;
        if d1 == d0 {
            return (r0 + r1) / 2.0;
        }
        r0 + (v - d0) / (d1 - d0) * (r1 - r0)
    }

    /// Maps a range position back to the domain.
    pub fn invert(&self, p: f64) -> f64 {
        let (d0, d1) = self.domain;
        let (r0, r1) = self.range;
        if r1 == r0 {
            return (d0 + d1) / 2.0;
        }
        d0 + (p - r0) / (r1 - r0) * (d1 - d0)
    }
}

/// A step of 1, 2 or 5 times a power of ten that splits `span` into about `target` intervals.
pub fn nice_step(span: f64, target: usize) -> f64 {
    let target = target.max(1) as f64;
    if !span.is_finite() || span <= 0.0 {
        return 1.0;
    }
    let raw = span / target;
    let mag = 10f64.powf(raw.log10().floor());
    let norm = raw / mag;
    let nice = if norm < 1.5 {
        1.0
    } else if norm < 3.0 {
        2.0
    } else if norm < 7.0 {
        5.0
    } else {
        10.0
    };
    nice * mag
}

/// Tick values at multiples of [`nice_step`] inside `[min, max]`, with the step used.
pub fn nice_ticks(min: f64, max: f64, target: usize) -> (Vec<f64>, f64) {
    if !(min.is_finite() && max.is_finite()) || max < min {
        return (Vec::new(), 1.0);
    }
    let step = nice_step(max - min, target);
    let mut ticks = Vec::new();
    let first = (min / step).ceil();
    let last = (max / step + 1e-9).floor();
    let mut i = first;
    while i <= last && ticks.len() < 1000 {
        let v = i * step;
        // Avoid printing -0.
        ticks.push(if v == 0.0 { 0.0 } else { v });
        i += 1.0;
    }
    (ticks, step)
}

/// Formats `v` with just enough decimals to tell ticks `step` apart.
/// Values of a million or more use a compact `k`, `M`, `B` or `T` suffix.
pub fn format_value(v: f64, step: f64) -> String {
    let abs = v.abs();
    for (div, suffix) in [(1e12, "T"), (1e9, "B"), (1e6, "M")] {
        if abs >= div {
            let decimals = decimals_for(step / div);
            return format!("{:.*}{suffix}", decimals, v / div);
        }
    }
    format!("{:.*}", decimals_for(step), v)
}

fn decimals_for(step: f64) -> usize {
    if !step.is_finite() || step <= 0.0 || step >= 1.0 {
        return 0;
    }
    ((-step.log10()).ceil() as i64).clamp(0, 10) as usize
}

/// A labelled tick on a time axis.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TimeTick {
    /// Where the tick sits.
    pub time: Timestamp,
    /// Its label in the chart's time zone.
    pub label: String,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum TimeStep {
    Fixed(i64),
    Months(i64),
    Years(i64),
}

impl TimeStep {
    fn approx_ms(self) -> f64 {
        match self {
            TimeStep::Fixed(ms) => ms as f64,
            TimeStep::Months(n) => n as f64 * 30.44 * DAY as f64,
            TimeStep::Years(n) => n as f64 * 365.25 * DAY as f64,
        }
    }
}

const FIXED_STEPS: [i64; 31] = [
    1,
    2,
    5,
    10,
    20,
    50,
    100,
    200,
    500,
    SECOND,
    2 * SECOND,
    5 * SECOND,
    10 * SECOND,
    15 * SECOND,
    30 * SECOND,
    MINUTE,
    2 * MINUTE,
    5 * MINUTE,
    10 * MINUTE,
    15 * MINUTE,
    30 * MINUTE,
    HOUR,
    2 * HOUR,
    3 * HOUR,
    4 * HOUR,
    6 * HOUR,
    12 * HOUR,
    DAY,
    2 * DAY,
    7 * DAY,
    14 * DAY,
];

fn pick_step(span: i64, target: usize) -> TimeStep {
    let target = target.max(1) as f64;
    let fits = |s: TimeStep| span as f64 / s.approx_ms() <= target;
    for ms in FIXED_STEPS {
        if fits(TimeStep::Fixed(ms)) {
            return TimeStep::Fixed(ms);
        }
    }
    for n in [1, 2, 3, 6] {
        if fits(TimeStep::Months(n)) {
            return TimeStep::Months(n);
        }
    }
    let mut years = 1;
    loop {
        for k in [1, 2, 5] {
            if fits(TimeStep::Years(years * k)) || years * k >= 1_000_000 {
                return TimeStep::Years(years * k);
            }
        }
        years *= 10;
    }
}

/// About `target` ticks between `start` and `end`, on whole local seconds, minutes,
/// hours, days, months or years in `tz`, labelled for that step.
///
/// Sub-day ticks that fall on local midnight are labelled with the date instead of the time.
pub fn time_ticks(start: Timestamp, end: Timestamp, target: usize, tz: TimeZone) -> Vec<TimeTick> {
    if end <= start {
        return Vec::new();
    }
    let step = pick_step(end - start, target);
    let times = match step {
        TimeStep::Fixed(ms) => {
            // Weeks start on Monday: day 0 (1970-01-01) was a Thursday, day 4 a Monday.
            let anchor = if ms >= 7 * DAY { 4 * DAY } else { 0 };
            let local = tz.to_local(start) - anchor;
            let mut t = local.div_euclid(ms) * ms;
            if t < local {
                t += ms;
            }
            let mut out = Vec::new();
            while tz.from_local(t + anchor) <= end && out.len() < 1000 {
                out.push(tz.from_local(t + anchor));
                t += ms;
            }
            out
        }
        TimeStep::Months(n) | TimeStep::Years(n) => {
            let months = if let TimeStep::Years(_) = step { n * 12 } else { n };
            let dt = DateTime::from_timestamp(start, tz);
            let mut index = dt.year as i64 * 12 + dt.month as i64 - 1;
            let at_boundary = dt.day == 1 && dt.hour == 0 && dt.minute == 0 && dt.second == 0 && dt.millis == 0;
            if !at_boundary {
                index += 1;
            }
            index = index.div_euclid(months) * months + if index.rem_euclid(months) == 0 { 0 } else { months };
            let mut out = Vec::new();
            loop {
                let ts = month_start(index, tz);
                if ts > end || out.len() >= 1000 {
                    break;
                }
                out.push(ts);
                index += months;
            }
            out
        }
    };
    times.into_iter().map(|t| TimeTick { time: t, label: label(t, step, tz) }).collect()
}

fn month_start(index: i64, tz: TimeZone) -> Timestamp {
    let year = index.div_euclid(12) as i32;
    let month = index.rem_euclid(12) as u32 + 1;
    DateTime { year, month, day: 1, hour: 0, minute: 0, second: 0, millis: 0 }.to_timestamp(tz)
}

fn label(t: Timestamp, step: TimeStep, tz: TimeZone) -> String {
    let dt = DateTime::from_timestamp(t, tz);
    let date = || {
        if dt.month == 1 && dt.day == 1 { format!("{}", dt.year) } else { format!("{} {}", MONTHS[dt.month as usize - 1], dt.day) }
    };
    let midnight = dt.hour == 0 && dt.minute == 0 && dt.second == 0 && dt.millis == 0;
    match step {
        TimeStep::Fixed(ms) if ms < SECOND => format!("{:02}:{:02}:{:02}.{:03}", dt.hour, dt.minute, dt.second, dt.millis),
        TimeStep::Fixed(_) if midnight => date(),
        TimeStep::Fixed(ms) if ms < MINUTE => format!("{:02}:{:02}:{:02}", dt.hour, dt.minute, dt.second),
        TimeStep::Fixed(ms) if ms < DAY => format!("{:02}:{:02}", dt.hour, dt.minute),
        TimeStep::Fixed(_) => date(),
        TimeStep::Months(_) if dt.month == 1 => format!("{}", dt.year),
        TimeStep::Months(_) => MONTHS[dt.month as usize - 1].to_string(),
        TimeStep::Years(_) => format!("{}", dt.year),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ts(y: i32, mo: u32, d: u32, h: u32, mi: u32) -> Timestamp {
        DateTime { year: y, month: mo, day: d, hour: h, minute: mi, second: 0, millis: 0 }.to_timestamp(TimeZone::Utc)
    }

    #[test]
    fn linear_maps_and_inverts() {
        let s = LinearScale::new((10.0, 20.0), (100.0, 0.0));
        assert_eq!(s.map(10.0), 100.0);
        assert_eq!(s.map(15.0), 50.0);
        assert_eq!(s.invert(25.0), 17.5);
        assert_eq!(LinearScale::new((3.0, 3.0), (0.0, 10.0)).map(3.0), 5.0);
    }

    #[test]
    fn nice_numeric_ticks() {
        let (t, step) = nice_ticks(0.3, 9.7, 5);
        assert_eq!(step, 2.0);
        assert_eq!(t, vec![2.0, 4.0, 6.0, 8.0]);
        let (t, step) = nice_ticks(101.23, 101.31, 4);
        assert!((step - 0.02).abs() < 1e-12);
        assert_eq!(t.len(), 4);
        assert_eq!(format_value(t[0], step), "101.24");
        assert_eq!(format_value(2_500_000.0, 500_000.0), "2.5M");
        assert_eq!(format_value(-0.0, 1.0), "-0");
        assert_eq!(format_value(42.0, 10.0), "42");
    }

    #[test]
    fn intraday_ticks_align_and_show_dates_at_midnight() {
        let ticks = time_ticks(ts(2026, 11, 25, 21, 7), ts(2026, 11, 26, 3, 0), 3, TimeZone::Utc);
        let labels: Vec<_> = ticks.iter().map(|t| t.label.as_str()).collect();
        assert_eq!(labels, ["22:00", "Nov 26", "02:00"]);
    }

    #[test]
    fn ticks_follow_the_time_zone() {
        let est = TimeZone::Fixed(-5 * 3600);
        let ticks = time_ticks(ts(2026, 11, 25, 14, 0), ts(2026, 11, 25, 18, 0), 4, est);
        assert_eq!(ticks[0].label, "09:00");
        assert_eq!(ticks[0].time, ts(2026, 11, 25, 14, 0));
    }

    #[test]
    fn monthly_and_yearly_ticks() {
        let ticks = time_ticks(ts(2026, 2, 10, 0, 0), ts(2027, 2, 10, 0, 0), 5, TimeZone::Utc);
        let labels: Vec<_> = ticks.iter().map(|t| t.label.as_str()).collect();
        assert_eq!(labels, ["Apr", "Jul", "Oct", "2027"]);
        let years = time_ticks(ts(2001, 6, 1, 0, 0), ts(2026, 6, 1, 0, 0), 6, TimeZone::Utc);
        assert_eq!(years.first().map(|t| t.label.as_str()), Some("2005"));
        assert!(years.iter().all(|t| t.label.len() == 4));
    }

    #[test]
    fn second_and_sub_second_ticks() {
        let start = ts(2026, 1, 2, 3, 4);
        let ticks = time_ticks(start, start + 50 * SECOND, 5, TimeZone::Utc);
        assert_eq!(ticks[1].label, "03:04:10");
        let fine = time_ticks(start + 1, start + 300, 3, TimeZone::Utc);
        assert_eq!(fine[0].label, "03:04:00.100");
        assert!(time_ticks(start, start, 5, TimeZone::Utc).is_empty());
    }
}

//! The series model: time-ordered ring buffers of points or candles, live updates and downsampling.

use std::collections::VecDeque;
use std::fmt;

use crate::time::Timestamp;

/// A single value at a time, for line, area and bar series.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Point {
    /// When.
    pub time: Timestamp,
    /// The value.
    pub value: f64,
}

impl Point {
    /// A point at `time` with `value`.
    pub const fn new(time: Timestamp, value: f64) -> Point {
        Point { time, value }
    }
}

/// An OHLC bar with volume. `time` is the start of the bar's interval.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Candle {
    /// Start of the interval.
    pub time: Timestamp,
    /// First price.
    pub open: f64,
    /// Highest price.
    pub high: f64,
    /// Lowest price.
    pub low: f64,
    /// Last price.
    pub close: f64,
    /// Traded volume.
    pub volume: f64,
}

impl Candle {
    /// A candle from its parts.
    pub const fn new(time: Timestamp, open: f64, high: f64, low: f64, close: f64, volume: f64) -> Candle {
        Candle { time, open, high, low, close, volume }
    }

    /// A candle opened by a single trade.
    pub const fn from_trade(time: Timestamp, price: f64, size: f64) -> Candle {
        Candle { time, open: price, high: price, low: price, close: price, volume: size }
    }

    /// Whether the close is at or above the open.
    pub fn is_up(&self) -> bool {
        self.close >= self.open
    }

    /// Folds a later trade into this candle.
    pub fn apply_trade(&mut self, price: f64, size: f64) {
        self.high = self.high.max(price);
        self.low = self.low.min(price);
        self.close = price;
        self.volume += size;
    }

    /// Folds a later candle into this one (used when aggregating).
    pub fn merge(&mut self, later: &Candle) {
        self.high = self.high.max(later.high);
        self.low = self.low.min(later.low);
        self.close = later.close;
        self.volume += later.volume;
    }
}

/// Anything a [`Series`] can hold: it has a time and a value extent.
pub trait Sample: Copy {
    /// When the sample happened.
    fn time(&self) -> Timestamp;
    /// The lowest and highest value it covers.
    fn extent(&self) -> (f64, f64);
}

impl Sample for Point {
    fn time(&self) -> Timestamp {
        self.time
    }
    fn extent(&self) -> (f64, f64) {
        (self.value, self.value)
    }
}

impl Sample for Candle {
    fn time(&self) -> Timestamp {
        self.time
    }
    fn extent(&self) -> (f64, f64) {
        (self.low, self.high)
    }
}

/// What a push did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Change {
    /// A new sample was appended.
    Appended,
    /// A new sample was appended and the oldest one dropped to stay within capacity.
    AppendedEvicted,
    /// The newest sample had the same time and was replaced (a live update).
    Replaced,
}

/// A push older than the newest sample.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OutOfOrder {
    /// The rejected sample's time.
    pub time: Timestamp,
    /// The newest time already in the series.
    pub latest: Timestamp,
}

impl fmt::Display for OutOfOrder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "sample at {} is older than the newest sample at {}", self.time, self.latest)
    }
}

impl std::error::Error for OutOfOrder {}

/// A bounded, time-ordered ring buffer of samples.
///
/// Appending is O(1), the oldest samples drop off once `capacity` is reached, and range
/// lookups are binary searches. A push with the same time as the newest sample replaces it,
/// which is how a live candle or price updates in place.
#[derive(Clone, Debug)]
pub struct Series<T> {
    samples: VecDeque<T>,
    capacity: usize,
    revision: u64,
}

impl<T: Sample> Series<T> {
    /// An empty series holding at most `capacity` samples (at least one).
    pub fn with_capacity(capacity: usize) -> Series<T> {
        let capacity = capacity.max(1);
        Series { samples: VecDeque::with_capacity(capacity.min(4096)), capacity, revision: 0 }
    }

    /// The maximum number of samples kept.
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// Changes the capacity, dropping the oldest samples if needed.
    pub fn set_capacity(&mut self, capacity: usize) {
        self.capacity = capacity.max(1);
        while self.samples.len() > self.capacity {
            self.samples.pop_front();
        }
        self.revision += 1;
    }

    /// The number of samples.
    pub fn len(&self) -> usize {
        self.samples.len()
    }

    /// Whether there are no samples.
    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }

    /// Bumped on every change; compare it to skip work when nothing changed.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Appends `sample`, or replaces the newest one if it has the same time.
    pub fn push(&mut self, sample: T) -> Result<Change, OutOfOrder> {
        let change = match self.samples.back_mut() {
            Some(last) if last.time() == sample.time() => {
                *last = sample;
                Change::Replaced
            }
            Some(last) if last.time() > sample.time() => return Err(OutOfOrder { time: sample.time(), latest: last.time() }),
            _ => {
                let evicted = self.samples.len() >= self.capacity;
                if evicted {
                    self.samples.pop_front();
                }
                self.samples.push_back(sample);
                if evicted { Change::AppendedEvicted } else { Change::Appended }
            }
        };
        self.revision += 1;
        Ok(change)
    }

    /// Pushes every sample in order, stopping at the first out-of-order one.
    pub fn extend<I: IntoIterator<Item = T>>(&mut self, samples: I) -> Result<(), OutOfOrder> {
        for s in samples {
            self.push(s)?;
        }
        Ok(())
    }

    /// Replaces the contents with `samples`, which are sorted by time first (history loads).
    /// Later duplicates of a time win.
    pub fn replace_all(&mut self, samples: Vec<T>) {
        let mut samples = samples;
        samples.sort_by_key(|s| s.time());
        self.samples.clear();
        for s in samples {
            match self.samples.back_mut() {
                Some(last) if last.time() == s.time() => *last = s,
                _ => self.samples.push_back(s),
            }
        }
        while self.samples.len() > self.capacity {
            self.samples.pop_front();
        }
        self.revision += 1;
    }

    /// Edits the newest sample in place (its time must not change).
    pub fn update_last(&mut self, f: impl FnOnce(&mut T)) -> bool {
        let Some(last) = self.samples.back_mut() else { return false };
        let time = last.time();
        f(last);
        debug_assert_eq!(time, last.time(), "update_last must not change the time");
        self.revision += 1;
        true
    }

    /// Removes every sample.
    pub fn clear(&mut self) {
        self.samples.clear();
        self.revision += 1;
    }

    /// The oldest sample.
    pub fn first(&self) -> Option<&T> {
        self.samples.front()
    }

    /// The newest sample.
    pub fn last(&self) -> Option<&T> {
        self.samples.back()
    }

    /// Every sample, oldest first.
    pub fn iter(&self) -> impl DoubleEndedIterator<Item = &T> + ExactSizeIterator {
        self.samples.iter()
    }

    /// The sample at `index` (0 is the oldest).
    pub fn get(&self, index: usize) -> Option<&T> {
        self.samples.get(index)
    }

    /// The index range of samples with `start <= time <= end`.
    pub fn index_range(&self, start: Timestamp, end: Timestamp) -> std::ops::Range<usize> {
        let a = self.samples.partition_point(|s| s.time() < start);
        let b = self.samples.partition_point(|s| s.time() <= end);
        a..b.max(a)
    }

    /// Samples with `start <= time <= end`, oldest first.
    pub fn range(&self, start: Timestamp, end: Timestamp) -> impl DoubleEndedIterator<Item = &T> + ExactSizeIterator {
        self.samples.range(self.index_range(start, end))
    }

    /// Like [`range`](Self::range) plus one sample on each side when there is one, so lines
    /// run to the edges of a viewport.
    pub fn range_padded(&self, start: Timestamp, end: Timestamp) -> impl DoubleEndedIterator<Item = &T> + ExactSizeIterator {
        let r = self.index_range(start, end);
        let a = r.start.saturating_sub(1);
        let b = (r.end + 1).min(self.samples.len());
        self.samples.range(a..b)
    }

    /// The lowest and highest value among samples in `[start, end]`, ignoring non-finite values.
    pub fn extent(&self, start: Timestamp, end: Timestamp) -> Option<(f64, f64)> {
        fold_extent(self.range(start, end).map(Sample::extent))
    }

    /// The typical spacing between samples: the median of the last (up to) 64 gaps.
    pub fn interval(&self) -> Option<i64> {
        let n = self.samples.len();
        if n < 2 {
            return None;
        }
        let from = n.saturating_sub(65);
        let mut gaps: Vec<i64> = (from + 1..n).map(|i| self.samples[i].time() - self.samples[i - 1].time()).filter(|g| *g > 0).collect();
        if gaps.is_empty() {
            return None;
        }
        gaps.sort_unstable();
        Some(gaps[gaps.len() / 2])
    }
}

impl Series<Candle> {
    /// Folds a trade into the candle for `interval_ms`-long bars starting at multiples of the
    /// interval: it updates the open candle or starts the next one.
    pub fn apply_trade(&mut self, time: Timestamp, price: f64, size: f64, interval_ms: i64) -> Result<Change, OutOfOrder> {
        let interval_ms = interval_ms.max(1);
        let bucket = time.div_euclid(interval_ms) * interval_ms;
        match self.samples.back() {
            Some(last) if last.time == bucket => {
                self.update_last(|c| c.apply_trade(price, size));
                Ok(Change::Replaced)
            }
            _ => self.push(Candle::from_trade(bucket, price, size)),
        }
    }
}

pub(crate) fn fold_extent(it: impl Iterator<Item = (f64, f64)>) -> Option<(f64, f64)> {
    let mut out: Option<(f64, f64)> = None;
    for (lo, hi) in it {
        if !(lo.is_finite() && hi.is_finite()) {
            continue;
        }
        out = Some(match out {
            Some((a, b)) => (a.min(lo), b.max(hi)),
            None => (lo, hi),
        });
    }
    out
}

/// Reduces points to at most four per bucket (first, lowest, highest, last), keeping every
/// spike and every gap (non-finite value), for drawing `buckets` columns of a range `[start, end]`.
///
/// Points must be in time order. When there are no more points than buckets they are
/// returned unchanged.
pub fn downsample_min_max<'a>(points: impl ExactSizeIterator<Item = &'a Point>, start: Timestamp, end: Timestamp, buckets: usize) -> Vec<Point> {
    if points.len() <= buckets.max(1) * 4 || end <= start {
        return points.copied().collect();
    }
    let buckets = buckets.max(1);
    let width = (end - start) as f64 / buckets as f64;
    let bucket_of = |t: Timestamp| (((t - start) as f64 / width).floor().clamp(-1.0, buckets as f64)) as i64;
    let mut out = Vec::with_capacity(buckets * 4);
    let mut current: Option<(i64, [Point; 4])> = None;
    let flush = |out: &mut Vec<Point>, b: [Point; 4]| {
        let [first, lo, hi, last] = b;
        let mut mid = [lo, hi];
        mid.sort_by_key(|p| p.time);
        for p in [first, mid[0], mid[1], last] {
            if out.last() != Some(&p) {
                out.push(p);
            }
        }
    };
    for p in points {
        if !p.value.is_finite() {
            // Keep gaps: a non-finite value ends the bucket and is passed through.
            if let Some((_, acc)) = current.take() {
                flush(&mut out, acc);
            }
            out.push(*p);
            continue;
        }
        let b = bucket_of(p.time);
        match &mut current {
            Some((cb, acc)) if *cb == b => {
                if p.value < acc[1].value {
                    acc[1] = *p;
                }
                if p.value > acc[2].value {
                    acc[2] = *p;
                }
                acc[3] = *p;
            }
            _ => {
                if let Some((_, acc)) = current.take() {
                    flush(&mut out, acc);
                }
                current = Some((b, [*p; 4]));
            }
        }
    }
    if let Some((_, acc)) = current {
        flush(&mut out, acc);
    }
    out
}

/// Merges candles into wider bars so that at most `buckets` remain across `[start, end]`.
/// Each output bar starts at its first input candle's time. Candles must be in time order.
pub fn aggregate_candles<'a>(candles: impl ExactSizeIterator<Item = &'a Candle>, start: Timestamp, end: Timestamp, buckets: usize) -> Vec<Candle> {
    if candles.len() <= buckets.max(1) || end <= start {
        return candles.copied().collect();
    }
    let width = (end - start) as f64 / buckets.max(1) as f64;
    let mut out: Vec<Candle> = Vec::with_capacity(buckets + 2);
    let mut current_bucket = i64::MIN;
    for c in candles {
        let b = ((c.time - start) as f64 / width).floor() as i64;
        match out.last_mut() {
            Some(last) if b == current_bucket => last.merge(c),
            _ => {
                out.push(*c);
                current_bucket = b;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_replace_evict_and_order() {
        let mut s = Series::with_capacity(3);
        assert_eq!(s.push(Point::new(1, 1.0)), Ok(Change::Appended));
        assert_eq!(s.push(Point::new(1, 2.0)), Ok(Change::Replaced));
        assert_eq!(s.last().unwrap().value, 2.0);
        s.push(Point::new(2, 3.0)).unwrap();
        s.push(Point::new(3, 4.0)).unwrap();
        assert_eq!(s.push(Point::new(4, 5.0)), Ok(Change::AppendedEvicted));
        assert_eq!(s.first().unwrap().time, 2);
        assert_eq!(s.push(Point::new(3, 9.0)), Err(OutOfOrder { time: 3, latest: 4 }));
        assert_eq!(s.len(), 3);
        assert_eq!(s.revision(), 5);
    }

    #[test]
    fn ranges_and_extent() {
        let mut s = Series::with_capacity(100);
        s.extend((0..10).map(|i| Point::new(i * 10, i as f64))).unwrap();
        let r: Vec<_> = s.range(15, 45).map(|p| p.time).collect();
        assert_eq!(r, [20, 30, 40]);
        let p: Vec<_> = s.range_padded(15, 45).map(|p| p.time).collect();
        assert_eq!(p, [10, 20, 30, 40, 50]);
        assert_eq!(s.extent(15, 45), Some((2.0, 4.0)));
        assert_eq!(s.extent(1000, 2000), None);
        assert_eq!(s.interval(), Some(10));
        s.push(Point::new(100, f64::NAN)).unwrap();
        assert_eq!(s.extent(90, 100), Some((9.0, 9.0)));
    }

    #[test]
    fn replace_all_sorts_and_dedups() {
        let mut s = Series::with_capacity(2);
        s.replace_all(vec![Point::new(3, 3.0), Point::new(1, 1.0), Point::new(3, 4.0), Point::new(2, 2.0)]);
        let v: Vec<_> = s.iter().map(|p| (p.time, p.value)).collect();
        assert_eq!(v, [(2, 2.0), (3, 4.0)]);
    }

    #[test]
    fn trades_build_candles() {
        let mut s: Series<Candle> = Series::with_capacity(10);
        s.apply_trade(1_000, 10.0, 1.0, 60_000).unwrap();
        s.apply_trade(30_000, 12.0, 2.0, 60_000).unwrap();
        s.apply_trade(59_999, 9.0, 1.0, 60_000).unwrap();
        assert_eq!(s.apply_trade(60_000, 11.0, 1.0, 60_000), Ok(Change::Appended));
        assert_eq!(s.len(), 2);
        assert_eq!(*s.first().unwrap(), Candle::new(0, 10.0, 12.0, 9.0, 9.0, 4.0));
        assert!(!s.first().unwrap().is_up());
    }

    #[test]
    fn min_max_keeps_spikes() {
        let pts: Vec<Point> = (0..1000).map(|i| Point::new(i, if i == 517 { 100.0 } else { (i % 7) as f64 })).collect();
        let out = downsample_min_max(pts.iter(), 0, 1000, 10);
        assert!(out.len() <= 40);
        assert!(out.iter().any(|p| p.value == 100.0));
        assert_eq!(out.first().unwrap().time, 0);
        assert_eq!(out.last().unwrap().time, 999);
        assert!(out.windows(2).all(|w| w[0].time <= w[1].time));
        let few: Vec<Point> = pts[..5].to_vec();
        assert_eq!(downsample_min_max(few.iter(), 0, 1000, 10), few);
    }

    #[test]
    fn candles_aggregate() {
        let cs: Vec<Candle> = (0..100).map(|i| Candle::new(i * 60, i as f64, i as f64 + 2.0, i as f64 - 1.0, i as f64 + 1.0, 1.0)).collect();
        let out = aggregate_candles(cs.iter(), 0, 6000, 10);
        assert_eq!(out.len(), 10);
        assert_eq!(out[0], Candle::new(0, 0.0, 11.0, -1.0, 10.0, 10.0));
        let total: f64 = out.iter().map(|c| c.volume).sum();
        assert_eq!(total, 100.0);
    }
}

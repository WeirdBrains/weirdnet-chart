//! The visible time window: pan, zoom and following the live edge.

use crate::scale::LinearScale;
use crate::time::{DAY, Timestamp};

/// The visible time range `[start, end]` of a chart.
///
/// While *following*, new data keeps the newest sample at the right edge (less a small
/// margin). Panning back in time stops following; [`Chart`](crate::Chart) turns it back on when
/// the view is panned or zoomed back to the newest sample.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Viewport {
    start: Timestamp,
    end: Timestamp,
    following: bool,
    min_span: i64,
    max_span: i64,
    right_margin: f64,
}

impl Default for Viewport {
    /// The last hour before the Unix epoch, following.
    fn default() -> Self {
        Viewport::new(-3_600_000, 0)
    }
}

impl Viewport {
    /// A following viewport over `[start, end]`.
    pub fn new(start: Timestamp, end: Timestamp) -> Viewport {
        let mut v = Viewport { start: 0, end: 1, following: true, min_span: 1, max_span: 1_000 * 365 * DAY, right_margin: 0.03 };
        v.set_range(start, end);
        v
    }

    /// The first visible time.
    pub fn start(&self) -> Timestamp {
        self.start
    }

    /// The last visible time.
    pub fn end(&self) -> Timestamp {
        self.end
    }

    /// `end - start`, at least 1.
    pub fn span(&self) -> i64 {
        self.end - self.start
    }

    /// Whether new data scrolls the view.
    pub fn is_following(&self) -> bool {
        self.following
    }

    /// Turns following on or off.
    pub fn set_following(&mut self, following: bool) {
        self.following = following;
    }

    /// The fraction of the span left empty to the right of the newest sample while following.
    pub fn right_margin(&self) -> f64 {
        self.right_margin
    }

    /// Sets the right margin (clamped to `0.0..=0.5`).
    pub fn set_right_margin(&mut self, fraction: f64) {
        self.right_margin = fraction.clamp(0.0, 0.5);
    }

    /// Limits how far zoom can go in and out, in milliseconds.
    pub fn set_span_limits(&mut self, min_span: i64, max_span: i64) {
        self.min_span = min_span.max(1);
        self.max_span = max_span.max(self.min_span);
        let (s, e) = (self.start, self.end);
        self.set_range(s, e);
    }

    /// Shows `[start, end]`, keeping the centre if the span has to be clamped.
    pub fn set_range(&mut self, start: Timestamp, end: Timestamp) {
        let (start, end) = if end < start { (end, start) } else { (start, end) };
        let span = (end - start).clamp(self.min_span, self.max_span);
        let mid = start + (end - start) / 2;
        self.start = mid - span / 2;
        self.end = self.start + span;
    }

    /// Moves the window by `delta` milliseconds (positive is later). Moving back stops following.
    pub fn pan(&mut self, delta: i64) {
        self.start = self.start.saturating_add(delta);
        self.end = self.end.saturating_add(delta);
        if delta < 0 {
            self.following = false;
        }
    }

    /// Moves the window by a fraction of its span (positive is later).
    pub fn pan_fraction(&mut self, fraction: f64) {
        self.pan((self.span() as f64 * fraction).round() as i64);
    }

    /// Moves the window for a drag of `dx` pixels across a plot `width` pixels wide:
    /// dragging right shows earlier times.
    pub fn pan_pixels(&mut self, dx: f64, width: f64) {
        if width > 0.0 {
            self.pan_fraction(-dx / width);
        }
    }

    /// Scales the span by `factor` (below 1 zooms in) keeping the time at `anchor` (a fraction
    /// of the width, 0 is the left edge) in place.
    pub fn zoom(&mut self, factor: f64, anchor: f64) {
        if !(factor.is_finite() && factor > 0.0) {
            return;
        }
        let anchor = anchor.clamp(0.0, 1.0);
        let span = self.span() as f64;
        let new_span = (span * factor).clamp(self.min_span as f64, self.max_span as f64);
        let pivot = self.start as f64 + span * anchor;
        self.start = (pivot - new_span * anchor).round() as i64;
        self.end = self.start + new_span.round() as i64;
    }

    /// [`zoom`](Self::zoom) around pixel `x` of a plot `width` pixels wide.
    pub fn zoom_at_pixel(&mut self, factor: f64, x: f64, width: f64) {
        if width > 0.0 {
            self.zoom(factor, x / width);
        }
    }

    /// While following, shifts the window so `latest` sits at the right edge less the margin.
    pub fn follow_to(&mut self, latest: Timestamp) {
        if self.following {
            let span = self.span();
            let margin = (span as f64 * self.right_margin).round() as i64;
            self.end = latest + margin;
            self.start = self.end - span;
        }
    }

    /// The time at pixel `x` of a plot from `left` that is `width` pixels wide.
    pub fn time_at(&self, x: f64, left: f64, width: f64) -> Timestamp {
        self.scale(left, left + width).invert(x).round() as Timestamp
    }

    /// A scale from this window onto `[x0, x1]`.
    pub fn scale(&self, x0: f64, x1: f64) -> LinearScale {
        LinearScale::new((self.start as f64, self.end as f64), (x0, x1))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pan_and_zoom() {
        let mut v = Viewport::new(0, 1000);
        v.pan_pixels(50.0, 100.0);
        assert_eq!((v.start(), v.end()), (-500, 500));
        assert!(!v.is_following());
        v.zoom(0.5, 1.0);
        assert_eq!((v.start(), v.end()), (0, 500));
        v.zoom_at_pixel(2.0, 0.0, 100.0);
        assert_eq!((v.start(), v.end()), (0, 1000));
        assert_eq!(v.time_at(50.0, 0.0, 100.0), 500);
    }

    #[test]
    fn follows_the_live_edge() {
        let mut v = Viewport::new(0, 1000);
        v.set_right_margin(0.1);
        v.follow_to(5000);
        assert_eq!((v.start(), v.end()), (4100, 5100));
        v.pan(-10);
        v.follow_to(9000);
        assert_eq!(v.end(), 5090);
    }

    #[test]
    fn span_limits_clamp() {
        let mut v = Viewport::new(0, 1000);
        v.set_span_limits(100, 2000);
        v.zoom(0.001, 0.5);
        assert_eq!(v.span(), 100);
        v.zoom(1000.0, 0.5);
        assert_eq!(v.span(), 2000);
        v.set_range(10, 10);
        assert_eq!(v.span(), 100);
    }
}

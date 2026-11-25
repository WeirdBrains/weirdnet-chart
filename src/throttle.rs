//! Frame-rate throttling for live redraws.

use std::time::{Duration, Instant};

/// What to do after [`FrameThrottle::request`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Redraw {
    /// Draw now; the frame has been counted.
    Now,
    /// Draw after this delay, then call [`FrameThrottle::fire`].
    After(Duration),
    /// A delayed redraw is already scheduled; nothing to do.
    Scheduled,
}

/// Coalesces redraw requests to at most one frame per interval.
///
/// Feed it every data update with [`request`](Self::request). Bursts of updates (say a
/// hundred trades in a millisecond) turn into one immediate frame and at most one delayed
/// frame, so the last update is always drawn. Loops that poll instead of scheduling a timer
/// use [`poll`](Self::poll) and [`timeout`](Self::timeout).
#[derive(Clone, Debug)]
pub struct FrameThrottle {
    interval: Duration,
    last_frame: Option<Instant>,
    pending: bool,
}

impl FrameThrottle {
    /// At most `fps` frames per second (at least 1).
    pub fn new(fps: u32) -> FrameThrottle {
        FrameThrottle::with_interval(Duration::from_secs_f64(1.0 / fps.max(1) as f64))
    }

    /// At most one frame per `interval`.
    pub fn with_interval(interval: Duration) -> FrameThrottle {
        FrameThrottle { interval, last_frame: None, pending: false }
    }

    /// The minimum time between frames.
    pub fn interval(&self) -> Duration {
        self.interval
    }

    /// Whether a delayed redraw is waiting.
    pub fn is_pending(&self) -> bool {
        self.pending
    }

    /// Asks for a redraw at `now`.
    pub fn request(&mut self, now: Instant) -> Redraw {
        if self.pending {
            return Redraw::Scheduled;
        }
        match self.last_frame {
            Some(last) if now.saturating_duration_since(last) < self.interval => {
                self.pending = true;
                Redraw::After(self.interval - now.saturating_duration_since(last))
            }
            _ => {
                self.last_frame = Some(now);
                Redraw::Now
            }
        }
    }

    /// Records that the delayed redraw ran at `now`.
    pub fn fire(&mut self, now: Instant) {
        self.pending = false;
        self.last_frame = Some(now);
    }

    /// Records a frame drawn for another reason (a resize, say) at `now`.
    pub fn frame_drawn(&mut self, now: Instant) {
        self.last_frame = Some(now);
    }

    /// For polling loops: true (and the frame is counted) when a pending redraw is due.
    pub fn poll(&mut self, now: Instant) -> bool {
        if self.pending && self.timeout(now) == Some(Duration::ZERO) {
            self.fire(now);
            true
        } else {
            false
        }
    }

    /// How long until a pending redraw is due, or `None` if nothing is pending.
    pub fn timeout(&self, now: Instant) -> Option<Duration> {
        if !self.pending {
            return None;
        }
        let elapsed = self.last_frame.map_or(self.interval, |l| now.saturating_duration_since(l));
        Some(self.interval.saturating_sub(elapsed))
    }
}

impl Default for FrameThrottle {
    /// 60 frames per second.
    fn default() -> Self {
        FrameThrottle::new(60)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coalesces_bursts() {
        let t0 = Instant::now();
        let ms = Duration::from_millis;
        let mut th = FrameThrottle::with_interval(ms(16));
        assert_eq!(th.request(t0), Redraw::Now);
        assert_eq!(th.request(t0 + ms(4)), Redraw::After(ms(12)));
        for i in 5..15 {
            assert_eq!(th.request(t0 + ms(i)), Redraw::Scheduled);
        }
        assert_eq!(th.timeout(t0 + ms(10)), Some(ms(6)));
        assert!(!th.poll(t0 + ms(10)));
        assert!(th.poll(t0 + ms(16)));
        assert!(!th.is_pending());
        assert_eq!(th.request(t0 + ms(40)), Redraw::Now);
    }

    #[test]
    fn fps_interval() {
        assert_eq!(FrameThrottle::new(50).interval(), Duration::from_millis(20));
        assert_eq!(FrameThrottle::new(0).interval(), Duration::from_secs(1));
    }
}

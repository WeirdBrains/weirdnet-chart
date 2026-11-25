//! Real-time charts for [Weird Brains](https://weirdbrains.com) apps: a renderer-independent
//! core with optional [GPUI](https://crates.io/crates/gpui) and
//! [Ratatui](https://crates.io/crates/ratatui) backends.
//!
//! The core has no GUI dependencies. It holds time-ordered series in ring buffers
//! ([`Series`]), arranges them in panes over one time axis ([`Chart`]), tracks the visible
//! window with pan, zoom and live following ([`Viewport`]), and lays the visible part out as
//! backend-neutral primitives ([`Scene`]) with "nice" value ticks and calendar-aware time
//! ticks. [`FrameThrottle`] turns a stream of live updates into redraws at the frame rate.
//!
//! ```
//! use weirdnet_chart::{Candle, Chart, Metrics, Pane, Size, YRange, time::MINUTE};
//!
//! let mut chart = Chart::new();
//! let candles = chart.add_candles(Chart::MAIN, "BTC-USD").unwrap();
//! let volume = chart.add_pane(Pane { weight: 1.0, y_range: YRange::AutoFromZero, title: None });
//! chart.add_volume(volume, candles).unwrap();
//! chart.show_latest(60 * MINUTE);
//!
//! // History, then live trades folded into one-minute candles.
//! chart.push_candle(candles, Candle::new(0, 100.0, 101.0, 99.5, 100.5, 12.0)).unwrap();
//! chart.apply_trade(candles, 61_000, 100.7, 0.5, MINUTE).unwrap();
//!
//! let scene = chart.layout(Size::new(800.0, 400.0), &Metrics::PIXELS);
//! assert_eq!(scene.panes.len(), 2);
//! ```
//!
//! # Features
//!
//! - `ratatui`: [`ratatui::ChartWidget`], a Ratatui widget drawn with braille dots.
//! - `gpui`: [`gpui::ChartView`], a GPUI view drawn with paths and quads, with pan and zoom.
#![warn(missing_docs)]

pub mod chart;
pub mod color;
pub mod scale;
pub mod scene;
pub mod series;
pub mod throttle;
pub mod time;
pub mod viewport;

#[cfg(feature = "gpui")]
#[path = "backend/gpui.rs"]
pub mod gpui;
#[cfg(feature = "ratatui")]
#[path = "backend/ratatui.rs"]
pub mod ratatui;

pub use chart::{Chart, ChartError, Mark, Pane, PaneId, SeriesId, SeriesStyle, Theme, YRange};
pub use color::Rgba;
pub use scale::{LinearScale, TimeTick};
pub use scene::{Align, Layer, Metrics, PaneLayout, Pos, Primitive, Rect, Scene, Size};
pub use series::{Candle, Change, OutOfOrder, Point, Sample, Series};
pub use throttle::{FrameThrottle, Redraw};
pub use time::{TimeZone, Timestamp};
pub use viewport::Viewport;

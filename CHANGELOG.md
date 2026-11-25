# Changelog

All notable changes to this crate are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the crate uses
[Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added

- Time-ordered ring-buffer series of points and candles, with live in-place updates and trade folding.
- Line, area, bar, candlestick, OHLC and volume marks in stacked panes over one time axis.
- Nice value ticks and calendar-aware time ticks for a fixed UTC offset.
- Viewport pan, zoom, span limits and live following.
- Min-max line downsampling and candle aggregation to the plot width.
- Backend-neutral scenes and a frame-rate redraw throttle.
- `ratatui` feature: a braille-dot chart widget with column-based pan and zoom.
- `gpui` feature: a chart view with drag-to-pan, scroll-to-zoom and throttled live redraws.

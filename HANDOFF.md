# Handoff

Running notes for [weirdnet-chart](https://github.com/WeirdBrains/weirdnet-chart), newest first.

## 2026-11-25: 0.1.0 released

- What changed: [weirdnet-chart 0.1.0](https://crates.io/crates/weirdnet-chart/0.1.0) is on
  crates.io (features `ratatui`, `gpui`, `gpui-test-support`); docs build at
  [docs.rs](https://docs.rs/weirdnet-chart/0.1.0).
- Commits: `b5c2c02` (`release: v0.1.0`), tag `v0.1.0`, and this entry.
- Checks: [CI](https://github.com/WeirdBrains/weirdnet-chart/actions) passed on `71eba6f` on
  Linux (fmt, clippy and tests with no features, `ratatui` and `ratatui,gpui-test-support`,
  docs, release validation, `cargo package`), macOS and Windows (tests with `ratatui`, clippy
  with `ratatui,gpui`); the release script re-ran its checks before tagging.
- Not tested: as in the entry below (no real window, GPU or live terminal).
- Left: listing the crate on the Weirdnet site.
- Next: the first app integration.

## 2026-11-25: initial crate, unreleased

- What changed: the first implementation. Ring-buffer series of points and candles with live
  in-place updates and trade folding; panes, marks (line, area, bars, candles, OHLC, volume),
  nice value ticks, calendar-aware time ticks with fixed UTC offsets, the viewport (pan, zoom,
  span limits, live following), min-max downsampling and candle aggregation, backend-neutral
  scenes and the frame throttle; the `ratatui` widget and the `gpui` view. The version stays
  0.0.0 until the first cut (0.1.0).
- Commits: `d3ade4a` (crate), `2eb488e` (docs), `9cf662b` (CI and release config), and this entry.
- Checks: `cargo fmt --check`; `cargo clippy --all-targets -D warnings` with no features,
  `ratatui`, and `ratatui,gpui-test-support`; `cargo test` with the same feature sets (unit,
  integration, doc and headless GPUI tests); `cargo doc --no-deps --features ratatui,gpui` with
  `-D warnings`; the terminal_snapshot example runs.
- Not tested: GPUI drawing in a real window or on a GPU (tests use GPUI's headless test
  platform, which lays out and paints without a display); the GPUI backend on macOS and Windows
  beyond CI builds; the Ratatui widget in a live terminal (tests render into buffers).
- Left: the first CI run on GitHub; the 0.1.0 release (both done, see above).

# Handoff

Running notes for [weirdnet-chart](https://github.com/WeirdBrains/weirdnet-chart), newest first.

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
- Left: the first CI run on GitHub; the 0.1.0 release.
- Next: release 0.1.0 with `python3 scripts/weirdnet_release.py --version 0.1.0 --dry-run`,
  then without `--dry-run`.

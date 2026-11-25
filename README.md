# weirdnet-chart

Real-time charts for [Weird Brains](https://weirdbrains.com) apps on Weirdnet: line, area,
bar, candlestick, OHLC and volume series with axes, pan, zoom and live updates. The core is
renderer-independent and has no GUI dependencies; [GPUI](https://crates.io/crates/gpui) and
[Ratatui](https://crates.io/crates/ratatui) backends are optional features.

- **Series.** Time-ordered ring buffers of points or candles ([`Series`](https://docs.rs/weirdnet-chart/latest/weirdnet_chart/series/struct.Series.html)).
  Appending is O(1), the oldest samples drop off at capacity, a push with the newest
  timestamp replaces it in place (a live candle), and trades fold into fixed-interval candles.
- **Panes.** Any number of panes stacked over one time axis, each with its own y axis
  (auto, auto from zero, or fixed), for example price above volume.
- **Axes.** "Nice" value ticks with compact labels, and calendar-aware time ticks from
  milliseconds to decades that land on whole local minutes, hours, days, months and years
  for a fixed UTC offset.
- **Viewport.** Pan, zoom around a point, span limits, and following the newest data; panning
  back to the live edge resumes following.
- **Big data, small frames.** Only the visible range is laid out. Lines are reduced to the
  first, lowest, highest and last point per pixel column (spikes and gaps survive), and
  candles merge into wider bars when they would be thinner than three pixels.
- **Frame-rate redraws.** [`FrameThrottle`](https://docs.rs/weirdnet-chart/latest/weirdnet_chart/throttle/struct.FrameThrottle.html)
  turns a burst of updates into one frame now and at most one more after the frame interval.
- **Scenes.** [`Chart::layout`](https://docs.rs/weirdnet-chart/latest/weirdnet_chart/chart/struct.Chart.html#method.layout)
  produces backend-neutral rectangles, lines, polylines, areas and text in clip layers, so a
  new backend only has to draw five primitives.

## Quick start

```toml
[dependencies]
weirdnet-chart = { version = "0.1", features = ["ratatui"] }
```

```rust
use weirdnet_chart::{Candle, Chart, Pane, YRange, time::MINUTE};

let mut chart = Chart::new();
let candles = chart.add_candles(Chart::MAIN, "BTC-USD").unwrap();
let volume = chart.add_pane(Pane { weight: 1.0, y_range: YRange::AutoFromZero, title: None });
chart.add_volume(volume, candles).unwrap();
chart.show_latest(60 * MINUTE);

chart.push_candle(candles, Candle::new(0, 100.0, 101.0, 99.5, 100.5, 12.0)).unwrap();
// Live trades update the open candle or start the next one.
chart.apply_trade(candles, 61_000, 100.7, 0.5, MINUTE).unwrap();
```

### Ratatui

`ratatui::ChartWidget` draws with braille dots (two across and four down per cell) on
[ratatui-core](https://crates.io/crates/ratatui-core), so it works with `ratatui` 0.30:

```rust,ignore
use weirdnet_chart::ratatui::{ChartWidget, pan_columns, zoom_columns};

terminal.draw(|f| f.render_widget(ChartWidget::new(&chart), f.area()))?;
pan_columns(&mut chart, area, 4);          // like dragging four columns right
zoom_columns(&mut chart, area, 0.8, col);  // zoom in around a column
```

Pair it with `FrameThrottle::poll` and `FrameThrottle::timeout` in the event loop so live data
redraws at the frame rate. The [terminal_snapshot](examples/terminal_snapshot.rs) example
renders into a buffer and prints it:
`cargo run --example terminal_snapshot --features ratatui`.

### GPUI

`gpui::ChartView` is a view that owns the chart, draws it with paths, quads and shaped text,
pans on drag and horizontal scroll, and zooms on vertical scroll. Feed it from a task:

```rust,ignore
view.update(cx, |view, cx| {
    view.update_chart(cx, |chart| chart.apply_trade(id, t, price, size, MINUTE).ok());
});
```

`update_chart` coalesces redraws to 60 frames per second (`set_fps` changes that).
`gpui::paint_scene` draws a laid-out scene in your own element or `canvas`.

The backend builds against `gpui` 0.2.2 from crates.io. An app that pins GPUI to a git
revision can point this crate at the same revision with `[patch.crates-io]`, as long as that
revision's `gpui` is semver-compatible. On Linux, GPUI needs the X11, Wayland, xkbcommon,
Vulkan, fontconfig and freetype development packages; on Debian and Ubuntu:
`libxkbcommon-dev libxkbcommon-x11-dev libx11-xcb-dev libxcb1-dev libwayland-dev libvulkan-dev libfontconfig-dev libfreetype-dev`.
gpui 0.2.2 also pulls in `xattr` 0.2.3, which needs `libc` 0.2.189 or older on Linux; this
repository's `Cargo.lock` pins it there.

## Features

| Feature | Default | What it adds |
| --- | --- | --- |
| `ratatui` | no | `ratatui::ChartWidget`, `render_scene`, column-based pan and zoom |
| `gpui` | no | `gpui::ChartView`, `paint_scene` |
| `gpui-test-support` | no | GPUI's headless test platform, for this crate's own tests |

## Time zones

Charts carry no time-zone database. Pass the UTC offset for the visible range with
`Chart::set_time_zone(TimeZone::Fixed(seconds))`; timestamps are milliseconds since the Unix
epoch.

## Licence

BSD-3-Clause, copyright Weird Brains LLC. See [LICENSE](LICENSE).

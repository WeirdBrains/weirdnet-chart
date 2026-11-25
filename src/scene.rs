//! Layout: turning the visible part of a [`Chart`] into backend-neutral drawing primitives.

use crate::chart::{Chart, Data, Entry, Mark, YRange};
use crate::color::Rgba;
use crate::scale::{LinearScale, format_value, nice_ticks, time_ticks};
use crate::series::{Candle, Point, aggregate_candles, downsample_min_max};
use crate::time::Timestamp;

/// A position in backend units, with y growing downwards.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pos {
    /// Horizontal position.
    pub x: f32,
    /// Vertical position, downwards.
    pub y: f32,
}

impl Pos {
    /// A position.
    pub const fn new(x: f32, y: f32) -> Pos {
        Pos { x, y }
    }
}

/// An axis-aligned rectangle in backend units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    /// Left edge.
    pub x: f32,
    /// Top edge.
    pub y: f32,
    /// Width.
    pub w: f32,
    /// Height.
    pub h: f32,
}

impl Rect {
    /// A rectangle.
    pub const fn new(x: f32, y: f32, w: f32, h: f32) -> Rect {
        Rect { x, y, w, h }
    }

    /// The right edge.
    pub fn right(&self) -> f32 {
        self.x + self.w
    }

    /// The bottom edge.
    pub fn bottom(&self) -> f32 {
        self.y + self.h
    }

    /// Whether `p` lies inside (edges included).
    pub fn contains(&self, p: Pos) -> bool {
        p.x >= self.x && p.x <= self.right() && p.y >= self.y && p.y <= self.bottom()
    }
}

/// The size of the drawing surface in backend units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Size {
    /// Width.
    pub width: f32,
    /// Height.
    pub height: f32,
}

impl Size {
    /// A size.
    pub const fn new(width: f32, height: f32) -> Size {
        Size { width, height }
    }
}

/// Text alignment relative to a text primitive's position.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    /// The position is the left (or top) edge.
    Start,
    /// The position is the centre.
    Center,
    /// The position is the right (or bottom) edge.
    End,
}

/// One thing to draw.
#[derive(Clone, Debug, PartialEq)]
pub enum Primitive {
    /// A filled rectangle.
    Rect {
        /// Where.
        rect: Rect,
        /// Fill colour.
        color: Rgba,
    },
    /// A straight line.
    Line {
        /// Start.
        from: Pos,
        /// End.
        to: Pos,
        /// Stroke colour.
        color: Rgba,
        /// Stroke width.
        width: f32,
        /// Drawn dashed (grid lines).
        dashed: bool,
    },
    /// Connected line segments.
    Polyline {
        /// The vertices, left to right.
        points: Vec<Pos>,
        /// Stroke colour.
        color: Rgba,
        /// Stroke width.
        width: f32,
    },
    /// The region between a polyline and a horizontal baseline, filled.
    Area {
        /// The upper edge, left to right.
        points: Vec<Pos>,
        /// The y of the lower edge.
        baseline: f32,
        /// Fill colour.
        color: Rgba,
    },
    /// A single line of text.
    Text {
        /// The anchor.
        pos: Pos,
        /// The text.
        text: String,
        /// Colour.
        color: Rgba,
        /// Horizontal alignment to the anchor.
        h_align: Align,
        /// Vertical alignment to the anchor.
        v_align: Align,
    },
}

/// Primitives that share a clip rectangle, drawn in order.
#[derive(Clone, Debug, PartialEq)]
pub struct Layer {
    /// Clip to this rectangle, if set.
    pub clip: Option<Rect>,
    /// What to draw.
    pub primitives: Vec<Primitive>,
}

/// A laid-out pane.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PaneLayout {
    /// The pane's plot rectangle.
    pub rect: Rect,
    /// Maps values to y positions in the pane.
    pub y: LinearScale,
}

/// How big text is, in backend units, so the layout can reserve room for labels.
/// Labels are treated as monospaced.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Metrics {
    /// Width of one character.
    pub char_width: f32,
    /// Height of one line of text.
    pub line_height: f32,
    /// Gap between labels and lines.
    pub padding: f32,
    /// The thinnest line or bar worth drawing (one pixel, dot or cell).
    pub min_stroke: f32,
}

impl Metrics {
    /// Pixel metrics for roughly 12-pixel text.
    pub const PIXELS: Metrics = Metrics { char_width: 7.0, line_height: 16.0, padding: 4.0, min_stroke: 1.0 };
}

impl Default for Metrics {
    fn default() -> Self {
        Metrics::PIXELS
    }
}

/// The drawing for one frame of a chart.
#[derive(Clone, Debug, PartialEq)]
pub struct Scene {
    /// The surface size it was laid out for.
    pub size: Size,
    /// Fill behind everything (may be transparent).
    pub background: Rgba,
    /// The plot area spanning all panes, without the axes.
    pub plot: Rect,
    /// Maps times to x positions.
    pub x: LinearScale,
    /// Each pane, top to bottom.
    pub panes: Vec<PaneLayout>,
    /// What to draw, back to front.
    pub layers: Vec<Layer>,
}

impl Scene {
    /// Every primitive, back to front.
    pub fn primitives(&self) -> impl Iterator<Item = &Primitive> {
        self.layers.iter().flat_map(|l| l.primitives.iter())
    }

    /// The time at x position `x`.
    pub fn time_at(&self, x: f32) -> Timestamp {
        self.x.invert(x as f64).round() as Timestamp
    }

    /// The pane under `p` and the value there.
    pub fn value_at(&self, p: Pos) -> Option<(usize, f64)> {
        self.panes.iter().enumerate().find(|(_, pl)| pl.rect.contains(p)).map(|(i, pl)| (i, pl.y.invert(p.y as f64)))
    }
}

struct PaneRange {
    lo: f64,
    hi: f64,
    ticks: Vec<f64>,
    step: f64,
}

impl Chart {
    /// Lays out the visible data for a surface of `size`, with text measured by `metrics`.
    pub fn layout(&self, size: Size, metrics: &Metrics) -> Scene {
        let m = metrics;
        let theme = &self.theme;
        let (start, end) = (self.viewport.start(), self.viewport.end());
        let mut scene = Scene {
            size,
            background: theme.background,
            plot: Rect::new(0.0, 0.0, 0.0, 0.0),
            x: self.viewport.scale(0.0, 0.0),
            panes: Vec::new(),
            layers: Vec::new(),
        };
        let x_axis_h = m.line_height + m.padding;
        if size.width < m.char_width * 12.0 || size.height < x_axis_h + m.line_height * 2.0 {
            return scene;
        }

        // Pane heights.
        let gap = (m.line_height / 2.0).max(m.min_stroke);
        let n = self.panes.len();
        let avail = size.height - x_axis_h - m.padding - gap * (n.saturating_sub(1)) as f32;
        let total_w: f32 = self.panes.iter().map(|p| p.weight.max(0.0)).sum::<f32>().max(f32::EPSILON);
        let mut heights: Vec<f32> = self.panes.iter().map(|p| avail * p.weight.max(0.0) / total_w).collect();
        if heights.iter().all(|h| *h <= 0.0) {
            heights = vec![avail / n as f32; n];
        }

        // Vertical ranges and ticks, which decide the axis width.
        let ranges: Vec<PaneRange> = self
            .panes
            .iter()
            .enumerate()
            .map(|(i, pane)| {
                let entries = self.entries.iter().filter(|e| e.visible && e.pane.0 == i);
                let extent = crate::series::fold_extent(entries.filter_map(|e| self.entry_extent(e, start, end)));
                let (mut lo, mut hi) = match (pane.y_range, extent) {
                    (YRange::Fixed(a, b), _) => (a.min(b), a.max(b)),
                    (_, Some((a, b))) => (a, b),
                    (_, None) => (0.0, 1.0),
                };
                if matches!(pane.y_range, YRange::AutoFromZero) {
                    lo = lo.min(0.0);
                    hi = hi.max(0.0);
                }
                if hi - lo <= f64::EPSILON * hi.abs().max(1.0) {
                    let pad = if lo == 0.0 { 1.0 } else { lo.abs() * 0.01 };
                    lo -= pad;
                    hi += pad;
                }
                if !matches!(pane.y_range, YRange::Fixed(..)) {
                    let pad = (hi - lo) * 0.08;
                    if !(matches!(pane.y_range, YRange::AutoFromZero) && lo >= 0.0) {
                        lo -= pad;
                    }
                    hi += pad;
                }
                let target = ((heights[i] / (m.line_height * 2.5)).floor() as usize).max(2);
                let (ticks, step) = nice_ticks(lo, hi, target);
                PaneRange { lo, hi, ticks, step }
            })
            .collect();

        let mut label_chars = 4;
        for r in &ranges {
            for t in &r.ticks {
                label_chars = label_chars.max(format_value(*t, r.step).chars().count());
            }
        }
        for e in self.entries.iter().filter(|e| e.visible && e.style.show_last_value) {
            if let Some(v) = self.last_value(e) {
                label_chars = label_chars.max(format_value(v, ranges[e.pane.0].step / 10.0).chars().count());
            }
        }
        let axis_w = label_chars as f32 * m.char_width + m.padding * 3.0;
        let plot = Rect::new(m.padding, m.padding, (size.width - axis_w - m.padding).max(1.0), avail + gap * (n.saturating_sub(1)) as f32);
        scene.plot = plot;
        scene.x = self.viewport.scale(plot.x as f64, plot.right() as f64);

        let mut y = plot.y;
        for (i, r) in ranges.iter().enumerate() {
            let rect = Rect::new(plot.x, y, plot.w, heights[i].max(0.0));
            scene.panes.push(PaneLayout { rect, y: LinearScale::new((r.lo, r.hi), (rect.bottom() as f64, rect.y as f64)) });
            y += heights[i] + gap;
        }

        // Grid, axes and labels.
        let x_ticks = time_ticks(start, end, ((plot.w / (m.char_width * 10.0)).floor() as usize).max(2), self.tz);
        let mut grid = Vec::new();
        let mut labels = Vec::new();
        let stroke = m.min_stroke;
        for (pl, r) in scene.panes.iter().zip(&ranges) {
            for t in &r.ticks {
                let py = pl.y.map(*t) as f32;
                grid.push(Primitive::Line {
                    from: Pos::new(pl.rect.x, py),
                    to: Pos::new(pl.rect.right(), py),
                    color: theme.grid,
                    width: stroke,
                    dashed: true,
                });
                labels.push(Primitive::Text {
                    pos: Pos::new(plot.right() + m.padding * 2.0, py),
                    text: format_value(*t, r.step),
                    color: theme.text,
                    h_align: Align::Start,
                    v_align: Align::Center,
                });
            }
            for tick in &x_ticks {
                let px = scene.x.map(tick.time as f64) as f32;
                grid.push(Primitive::Line {
                    from: Pos::new(px, pl.rect.y),
                    to: Pos::new(px, pl.rect.bottom()),
                    color: theme.grid,
                    width: stroke,
                    dashed: true,
                });
            }
            grid.push(Primitive::Line {
                from: Pos::new(pl.rect.x, pl.rect.bottom()),
                to: Pos::new(pl.rect.right(), pl.rect.bottom()),
                color: theme.axis,
                width: stroke,
                dashed: false,
            });
        }
        grid.push(Primitive::Line {
            from: Pos::new(plot.right(), plot.y),
            to: Pos::new(plot.right(), plot.bottom()),
            color: theme.axis,
            width: stroke,
            dashed: false,
        });
        let mut last_right = f32::NEG_INFINITY;
        for tick in &x_ticks {
            let px = scene.x.map(tick.time as f64) as f32;
            let half = tick.label.chars().count() as f32 * m.char_width / 2.0;
            if px - half < last_right + m.char_width || px - half < 0.0 || px + half > plot.right() {
                continue;
            }
            last_right = px + half;
            labels.push(Primitive::Text {
                pos: Pos::new(px, plot.bottom() + m.padding),
                text: tick.label.clone(),
                color: theme.text,
                h_align: Align::Center,
                v_align: Align::Start,
            });
        }
        if theme.background.a > 0.0 {
            scene.layers.push(Layer {
                clip: None,
                primitives: vec![Primitive::Rect { rect: Rect::new(0.0, 0.0, size.width, size.height), color: theme.background }],
            });
        }
        scene.layers.push(Layer { clip: None, primitives: grid });

        // Series, clipped to their pane.
        for (i, pl) in scene.panes.iter().enumerate() {
            let mut prims = Vec::new();
            for e in self.entries.iter().filter(|e| e.visible && e.pane.0 == i) {
                self.draw_entry(e, &scene.x, pl, m, &mut prims);
            }
            if let Some(title) = &self.panes[i].title {
                prims.push(Primitive::Text {
                    pos: Pos::new(pl.rect.x + m.padding, pl.rect.y + m.padding),
                    text: title.clone(),
                    color: theme.text,
                    h_align: Align::Start,
                    v_align: Align::Start,
                });
            }
            scene.layers.push(Layer { clip: Some(pl.rect), primitives: prims });
        }

        // Last-value tags on the axis.
        for e in self.entries.iter().filter(|e| e.visible && e.style.show_last_value) {
            let (Some(v), Some(pl)) = (self.last_value(e), scene.panes.get(e.pane.0)) else { continue };
            let py = pl.y.map(v) as f32;
            if py < pl.rect.y || py > pl.rect.bottom() {
                continue;
            }
            let color = self.last_color(e);
            let text = format_value(v, ranges[e.pane.0].step / 10.0);
            labels.retain(
                |p| !matches!(p, Primitive::Text { pos, h_align: Align::Start, .. } if (pos.y - py).abs() < m.line_height && pos.x > plot.right()),
            );
            labels.push(Primitive::Rect {
                rect: Rect::new(plot.right() + m.padding, py - m.line_height / 2.0, axis_w - m.padding, m.line_height),
                color,
            });
            labels.push(Primitive::Text {
                pos: Pos::new(plot.right() + m.padding * 2.0, py),
                text,
                color: theme.tag_text,
                h_align: Align::Start,
                v_align: Align::Center,
            });
        }
        scene.layers.push(Layer { clip: None, primitives: labels });
        scene
    }

    fn last_value(&self, e: &Entry) -> Option<f64> {
        match &e.data {
            Data::Points(s) => s.last().map(|p| p.value),
            Data::Candles(s) => s.last().map(|c| c.close),
            Data::VolumeOf(src) => self.candles(*src)?.last().map(|c| c.volume),
        }
        .filter(|v| v.is_finite())
    }

    fn last_color(&self, e: &Entry) -> Rgba {
        let up = |c: Option<&Candle>| if c.is_none_or(|c| c.is_up()) { self.theme.up } else { self.theme.down };
        match &e.data {
            Data::Points(_) => e.style.color,
            Data::Candles(s) => up(s.last()),
            Data::VolumeOf(src) => up(self.candles(*src).and_then(|s| s.last())),
        }
    }

    fn draw_entry(&self, e: &Entry, x: &LinearScale, pl: &PaneLayout, m: &Metrics, out: &mut Vec<Primitive>) {
        let (start, end) = (self.viewport.start(), self.viewport.end());
        let px_per_ms = (pl.rect.w as f64 / (end - start).max(1) as f64).max(0.0);
        let columns = pl.rect.w.max(1.0) as usize;
        let ymap = |v: f64| pl.y.map(v) as f32;
        let zero_y = ymap(0.0f64.clamp(pl.y.domain.0, pl.y.domain.1));
        match (&e.data, e.mark) {
            (Data::Points(s), Mark::Line | Mark::Area) => {
                let pts = downsample_min_max(s.range_padded(start, end), start, end, columns);
                for run in finite_runs(&pts) {
                    let poly: Vec<Pos> = run.iter().map(|p| Pos::new(x.map(p.time as f64) as f32, ymap(p.value))).collect();
                    if e.mark == Mark::Area && poly.len() >= 2 {
                        out.push(Primitive::Area {
                            points: poly.clone(),
                            baseline: pl.rect.bottom(),
                            color: e.style.color.with_alpha(e.style.color.a * 0.25),
                        });
                    }
                    if poly.len() >= 2 {
                        out.push(Primitive::Polyline { points: poly, color: e.style.color, width: e.style.width.max(m.min_stroke) });
                    } else if let Some(p) = poly.first() {
                        let w = e.style.width.max(m.min_stroke);
                        out.push(Primitive::Rect { rect: Rect::new(p.x - w / 2.0, p.y - w / 2.0, w, w), color: e.style.color });
                    }
                }
            }
            (Data::Points(s), _) => {
                let interval = s.interval().unwrap_or((end - start) / columns.max(1) as i64).max(1);
                let w = ((interval as f64 * px_per_ms * 0.8) as f32).max(m.min_stroke);
                for p in s.range(start, end).filter(|p| p.value.is_finite()) {
                    let cx = x.map(p.time as f64) as f32;
                    let (a, b) = (zero_y, ymap(p.value));
                    out.push(Primitive::Rect { rect: Rect::new(cx - w / 2.0, a.min(b), w, (a - b).abs().max(m.min_stroke)), color: e.style.color });
                }
            }
            (Data::Candles(_), _) | (Data::VolumeOf(_), _) => {
                let Some(s) = self.candles_of(e) else { return };
                let base_interval = s.interval().unwrap_or(60_000).max(1);
                let mut interval = base_interval;
                let slot_px = interval as f64 * px_per_ms;
                let min_slot = (m.min_stroke * 3.0) as f64;
                let candles: Vec<Candle> = if slot_px < min_slot {
                    let buckets = ((pl.rect.w as f64 / min_slot).floor() as usize).max(1);
                    interval = ((end - start) / buckets as i64).max(interval);
                    aggregate_candles(s.range_padded(start, end), start, end, buckets)
                } else {
                    s.range_padded(start, end).copied().collect()
                };
                let slot = (interval as f64 * px_per_ms) as f32;
                let body_w = (slot * 0.7).max(m.min_stroke);
                // Aggregated volume is shown per base interval so the pane's range still fits.
                let volume_scale = base_interval as f64 / interval as f64;
                for c in candles.iter().filter(|c| c.open.is_finite() && c.high.is_finite() && c.low.is_finite() && c.close.is_finite()) {
                    let color = if c.is_up() { self.theme.up } else { self.theme.down };
                    let cx = x.map(c.time as f64 + interval as f64 / 2.0) as f32;
                    match e.mark {
                        Mark::Volume => {
                            if !c.volume.is_finite() {
                                continue;
                            }
                            let top = ymap(c.volume * volume_scale);
                            out.push(Primitive::Rect {
                                rect: Rect::new(cx - body_w / 2.0, top.min(zero_y), body_w, (zero_y - top).abs().max(m.min_stroke)),
                                color: color.with_alpha(0.6),
                            });
                        }
                        Mark::Ohlc => {
                            let line = |a: Pos, b: Pos| Primitive::Line { from: a, to: b, color, width: m.min_stroke, dashed: false };
                            out.push(line(Pos::new(cx, ymap(c.high)), Pos::new(cx, ymap(c.low))));
                            out.push(line(Pos::new(cx - body_w / 2.0, ymap(c.open)), Pos::new(cx, ymap(c.open))));
                            out.push(line(Pos::new(cx, ymap(c.close)), Pos::new(cx + body_w / 2.0, ymap(c.close))));
                        }
                        _ => {
                            out.push(Primitive::Line {
                                from: Pos::new(cx, ymap(c.high)),
                                to: Pos::new(cx, ymap(c.low)),
                                color,
                                width: m.min_stroke,
                                dashed: false,
                            });
                            let (a, b) = (ymap(c.open), ymap(c.close));
                            out.push(Primitive::Rect {
                                rect: Rect::new(cx - body_w / 2.0, a.min(b), body_w, (a - b).abs().max(m.min_stroke)),
                                color,
                            });
                        }
                    }
                }
            }
        }
    }

    fn candles_of<'a>(&'a self, e: &'a Entry) -> Option<&'a crate::series::Series<Candle>> {
        match &e.data {
            Data::Candles(s) => Some(s),
            Data::VolumeOf(src) => self.candles(*src),
            Data::Points(_) => None,
        }
    }
}

fn finite_runs(points: &[Point]) -> impl Iterator<Item = &[Point]> {
    points.split(|p| !p.value.is_finite()).filter(|r| !r.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chart::{Pane, SeriesStyle};
    use crate::time::MINUTE;

    fn sample_chart() -> (Chart, crate::SeriesId) {
        let mut c = Chart::new();
        let candles = c.add_candles(Chart::MAIN, "XYZ").unwrap();
        let vol_pane = c.add_pane(Pane { weight: 1.0, y_range: YRange::AutoFromZero, title: Some("Volume".into()) });
        c.add_volume(vol_pane, candles).unwrap();
        let data: Vec<Candle> = (0..120)
            .map(|i| {
                let o = 100.0 + (i as f64 * 0.3).sin() * 5.0;
                let cl = o + if i % 3 == 0 { -1.0 } else { 1.0 };
                Candle::new(i * MINUTE, o, o.max(cl) + 0.5, o.min(cl) - 0.5, cl, 10.0 + i as f64)
            })
            .collect();
        c.set_candles(candles, data).unwrap();
        c.show_latest(60 * MINUTE);
        (c, candles)
    }

    #[test]
    fn lays_out_panes_axes_and_candles() {
        let (c, _) = sample_chart();
        let scene = c.layout(Size::new(800.0, 400.0), &Metrics::PIXELS);
        assert_eq!(scene.panes.len(), 2);
        let (a, b) = (scene.panes[0].rect, scene.panes[1].rect);
        assert!(a.h > b.h * 2.5 && a.h < b.h * 3.5, "{a:?} {b:?}");
        assert!(b.y > a.bottom());
        assert!(scene.plot.right() < 800.0 - 7.0 * 4.0);
        let texts: Vec<&str> =
            scene.primitives().filter_map(|p| if let Primitive::Text { text, .. } = p { Some(text.as_str()) } else { None }).collect();
        assert!(texts.contains(&"Volume"));
        assert!(texts.iter().any(|t| t.contains(':')), "{texts:?}");
        let rects =
            scene.layers.iter().filter(|l| l.clip == Some(a)).flat_map(|l| &l.primitives).filter(|p| matches!(p, Primitive::Rect { .. })).count();
        assert!((55..=64).contains(&rects), "{rects} candle bodies");
        // Every candle body sits inside its pane vertically.
        for l in scene.layers.iter().filter(|l| l.clip.is_some()) {
            for p in &l.primitives {
                if let Primitive::Rect { rect, .. } = p {
                    let clip = l.clip.unwrap();
                    assert!(rect.y >= clip.y - 0.01 && rect.bottom() <= clip.bottom() + 0.01, "{rect:?} outside {clip:?}");
                }
            }
        }
    }

    #[test]
    fn zoomed_out_candles_are_aggregated() {
        let (mut c, _) = sample_chart();
        c.fit_all();
        let scene = c.layout(Size::new(200.0, 300.0), &Metrics::PIXELS);
        let bodies = scene
            .layers
            .iter()
            .filter(|l| l.clip == Some(scene.panes[0].rect))
            .flat_map(|l| &l.primitives)
            .filter(|p| matches!(p, Primitive::Rect { .. }))
            .count();
        let max = (scene.panes[0].rect.w / 3.0) as usize + 2;
        assert!(bodies <= max && bodies > 10, "{bodies} > {max}");
    }

    #[test]
    fn last_value_tag_and_lines() {
        let mut c = Chart::new();
        let id = c.add_series(Chart::MAIN, "p", Mark::Area, SeriesStyle::new(Rgba::hex(0x3366ff)).with_last_value()).unwrap();
        for t in 0..10_000 {
            c.push_point(id, Point::new(t * 1000, if t == 5000 { f64::NAN } else { (t % 100) as f64 })).unwrap();
        }
        c.fit_all();
        let scene = c.layout(Size::new(640.0, 320.0), &Metrics::PIXELS);
        let polys: Vec<usize> =
            scene.primitives().filter_map(|p| if let Primitive::Polyline { points, .. } = p { Some(points.len()) } else { None }).collect();
        assert_eq!(polys.len(), 2, "the NaN splits the line");
        assert!(polys.iter().sum::<usize>() <= 640 * 4);
        assert!(scene.primitives().any(|p| matches!(p, Primitive::Area { .. })));
        assert!(scene.primitives().any(|p| matches!(p, Primitive::Text { text, .. } if text.starts_with("99"))), "last value tag");
        let mid = scene.time_at(scene.plot.x + scene.plot.w / 2.0);
        assert!((mid - c.viewport().start() - c.viewport().span() / 2).abs() < 1000);
    }

    #[test]
    fn tiny_or_empty_surfaces_do_not_panic() {
        let c = Chart::new();
        assert!(c.layout(Size::new(10.0, 10.0), &Metrics::PIXELS).layers.is_empty());
        let s = c.layout(Size::new(300.0, 200.0), &Metrics::PIXELS);
        assert_eq!(s.panes.len(), 1);
    }
}

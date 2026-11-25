//! A [Ratatui](https://crates.io/crates/ratatui) backend (feature `ratatui`).
//!
//! Charts are laid out in braille dots, two across and four down per terminal cell, and
//! drawn into a [`Buffer`]. It builds on `ratatui-core`, so the widget works with `ratatui`
//! 0.30 and any other crate built on it.
//!
//! ```
//! use ratatui_core::{buffer::Buffer, layout::Rect, widgets::Widget};
//! use weirdnet_chart::{Chart, Point, Rgba, ratatui::ChartWidget};
//!
//! let mut chart = Chart::new();
//! let price = chart.add_line(Chart::MAIN, "price", Rgba::hex(0x42a5f5)).unwrap();
//! for t in 0..100 {
//!     chart.push_point(price, Point::new(t * 1000, (t as f64 / 9.0).sin())).unwrap();
//! }
//! chart.fit_all();
//! let area = Rect::new(0, 0, 60, 16);
//! let mut buf = Buffer::empty(area);
//! ChartWidget::new(&chart).render(area, &mut buf);
//! ```
//!
//! Pan and zoom by terminal columns with [`pan_columns`] and [`zoom_columns`].

use ratatui_core::buffer::Buffer;
use ratatui_core::layout::Rect as CellRect;
use ratatui_core::style::Color;
use ratatui_core::widgets::Widget;

use crate::chart::Chart;
use crate::color::Rgba;
use crate::scene::{Align, Metrics, Pos, Primitive, Rect, Scene, Size};

/// Layout metrics in braille dots: a character is 2 dots wide and 4 high.
pub const METRICS: Metrics = Metrics { char_width: 2.0, line_height: 4.0, padding: 1.0, min_stroke: 1.0 };

/// Draws a [`Chart`] with braille dots.
#[derive(Clone, Copy, Debug)]
pub struct ChartWidget<'a> {
    chart: &'a Chart,
}

impl<'a> ChartWidget<'a> {
    /// A widget for `chart`.
    pub fn new(chart: &'a Chart) -> ChartWidget<'a> {
        ChartWidget { chart }
    }
}

impl Widget for ChartWidget<'_> {
    fn render(self, area: CellRect, buf: &mut Buffer) {
        (&self).render(area, buf);
    }
}

impl Widget for &ChartWidget<'_> {
    fn render(self, area: CellRect, buf: &mut Buffer) {
        let scene = scene_for(self.chart, area);
        render_scene(&scene, area, buf);
    }
}

/// Lays `chart` out for a terminal `area`, in dots.
pub fn scene_for(chart: &Chart, area: CellRect) -> Scene {
    chart.layout(Size::new(area.width as f32 * 2.0, area.height as f32 * 4.0), &METRICS)
}

/// Pans `chart` as if dragged `columns` cells to the right (negative pans towards newer data)
/// when drawn in `area`.
pub fn pan_columns(chart: &mut Chart, area: CellRect, columns: i32) {
    let plot = scene_for(chart, area).plot;
    chart.pan_pixels(columns as f64 * 2.0, plot.w as f64);
}

/// Zooms `chart` by `factor` (below 1 zooms in) around terminal column `column` of `area`.
pub fn zoom_columns(chart: &mut Chart, area: CellRect, factor: f64, column: u16) {
    let plot = scene_for(chart, area).plot;
    let x = (column.saturating_sub(area.x) as f64 * 2.0 - plot.x as f64).clamp(0.0, plot.w as f64);
    chart.zoom_at(factor, x, plot.w as f64);
}

const BRAILLE_BASE: u32 = 0x2800;
// Dot bit for (column 0..2, row 0..4) within a cell.
const DOT_BITS: [[u8; 4]; 2] = [[0x01, 0x02, 0x04, 0x40], [0x08, 0x10, 0x20, 0x80]];

#[derive(Clone, Copy, Default)]
struct DotCell {
    bits: u8,
    fg: Option<Rgba>,
    bg: Option<Rgba>,
    text: Option<(char, Rgba)>,
}

struct Raster {
    w: i32,
    h: i32,
    cells: Vec<DotCell>,
    backdrop: Rgba,
}

impl Raster {
    fn cell(&mut self, cx: i32, cy: i32) -> Option<&mut DotCell> {
        if cx < 0 || cy < 0 || cx >= self.w || cy >= self.h {
            return None;
        }
        self.cells.get_mut((cy * self.w + cx) as usize)
    }

    fn dot(&mut self, x: i32, y: i32, color: Rgba, clip: Option<(i32, i32, i32, i32)>) {
        if let Some((x0, y0, x1, y1)) = clip
            && (x < x0 || x > x1 || y < y0 || y > y1)
        {
            return;
        }
        if x < 0 || y < 0 {
            return;
        }
        let color = if color.a < 1.0 { color.over(self.backdrop) } else { color };
        if let Some(c) = self.cell(x / 2, y / 4) {
            c.bits |= DOT_BITS[(x % 2) as usize][(y % 4) as usize];
            c.fg = Some(color);
            c.text = None;
        }
    }

    fn line(&mut self, a: Pos, b: Pos, color: Rgba, dashed: bool, clip: Option<(i32, i32, i32, i32)>) {
        let (mut x0, mut y0) = (a.x.round() as i32, a.y.round() as i32);
        let (x1, y1) = (b.x.round() as i32, b.y.round() as i32);
        let (dx, dy) = ((x1 - x0).abs(), -(y1 - y0).abs());
        let (sx, sy) = (if x0 < x1 { 1 } else { -1 }, if y0 < y1 { 1 } else { -1 });
        let mut err = dx + dy;
        let mut step = 0;
        loop {
            if !dashed || step % 3 == 0 {
                self.dot(x0, y0, color, clip);
            }
            step += 1;
            if (x0 == x1 && y0 == y1) || step > 100_000 {
                break;
            }
            let e2 = 2 * err;
            if e2 >= dy {
                err += dy;
                x0 += sx;
            }
            if e2 <= dx {
                err += dx;
                y0 += sy;
            }
        }
    }

    fn fill(&mut self, r: Rect, color: Rgba, clip: Option<(i32, i32, i32, i32)>) {
        let x0 = r.x.round() as i32;
        let x1 = ((r.right().round() as i32) - 1).max(x0);
        let y0 = r.y.round() as i32;
        let y1 = ((r.bottom().round() as i32) - 1).max(y0);
        for y in y0..=y1.min(y0 + 4096) {
            for x in x0..=x1.min(x0 + 4096) {
                self.dot(x, y, color, clip);
            }
        }
    }

    fn background(&mut self, r: Rect, color: Rgba) {
        let color = if color.a < 1.0 { color.over(self.backdrop) } else { color };
        let cx0 = (r.x / 2.0).floor() as i32;
        let cx1 = (r.right() / 2.0).ceil() as i32;
        let cy0 = (r.y / 4.0).round() as i32;
        let cy1 = ((r.bottom() / 4.0).round() as i32).max(cy0 + 1);
        for cy in cy0..cy1 {
            for cx in cx0..cx1 {
                if let Some(c) = self.cell(cx, cy) {
                    c.bg = Some(color);
                }
            }
        }
    }

    fn text(&mut self, pos: Pos, text: &str, color: Rgba, h: Align, v: Align) {
        let len = text.chars().count() as i32;
        let col = match h {
            Align::Start => (pos.x / 2.0).ceil() as i32,
            Align::Center => (pos.x / 2.0).round() as i32 - len / 2,
            Align::End => (pos.x / 2.0).floor() as i32 - len,
        };
        let row = match v {
            Align::Start => (pos.y / 4.0).ceil() as i32,
            Align::Center => (pos.y / 4.0).floor() as i32,
            Align::End => (pos.y / 4.0).floor() as i32 - 1,
        };
        for (i, ch) in text.chars().enumerate() {
            if let Some(c) = self.cell(col + i as i32, row) {
                c.text = Some((ch, color));
                c.bits = 0;
            }
        }
    }
}

fn to_color(c: Rgba) -> Color {
    let (r, g, b) = c.to_rgb8();
    Color::Rgb(r, g, b)
}

/// Draws a scene laid out in dots (see [`scene_for`]) into `area` of `buf`.
///
/// Rectangles in unclipped layers (the background and last-value tags) become cell
/// backgrounds; everything else is drawn with braille dots or text.
pub fn render_scene(scene: &Scene, area: CellRect, buf: &mut Buffer) {
    let area = area.intersection(buf.area);
    if area.is_empty() {
        return;
    }
    let backdrop = if scene.background.a >= 1.0 { scene.background } else { Rgba::hex(0x000000) };
    let mut r =
        Raster { w: area.width as i32, h: area.height as i32, cells: vec![DotCell::default(); area.width as usize * area.height as usize], backdrop };
    for layer in &scene.layers {
        let clip = layer.clip.map(|c| (c.x.round() as i32, c.y.round() as i32, c.right().round() as i32, c.bottom().round() as i32));
        for p in &layer.primitives {
            match p {
                Primitive::Rect { rect, color } if layer.clip.is_none() => r.background(*rect, *color),
                Primitive::Rect { rect, color } => r.fill(*rect, *color, clip),
                Primitive::Line { from, to, color, dashed, .. } => r.line(*from, *to, *color, *dashed, clip),
                Primitive::Polyline { points, color, .. } => {
                    for w in points.windows(2) {
                        r.line(w[0], w[1], *color, false, clip);
                    }
                }
                Primitive::Area { points, baseline, color } => {
                    for w in points.windows(2) {
                        let (a, b) = (w[0], w[1]);
                        let (xa, xb) = (a.x.round() as i32, b.x.round() as i32);
                        for x in xa..xb.min(xa + 4096) {
                            let t = if xb == xa { 0.0 } else { (x - xa) as f32 / (xb - xa) as f32 };
                            let y = a.y + (b.y - a.y) * t;
                            r.line(Pos::new(x as f32, y), Pos::new(x as f32, *baseline), *color, false, clip);
                        }
                    }
                }
                Primitive::Text { pos, text, color, h_align, v_align } => r.text(*pos, text, *color, *h_align, *v_align),
            }
        }
    }
    for cy in 0..r.h {
        for cx in 0..r.w {
            let c = r.cells[(cy * r.w + cx) as usize];
            let Some(cell) = buf.cell_mut((area.x + cx as u16, area.y + cy as u16)) else { continue };
            if let Some(bg) = c.bg {
                cell.bg = to_color(bg);
            }
            if let Some((ch, fg)) = c.text {
                cell.set_char(ch);
                cell.fg = to_color(fg);
            } else if c.bits != 0 {
                cell.set_char(char::from_u32(BRAILLE_BASE + c.bits as u32).unwrap_or(' '));
                if let Some(fg) = c.fg {
                    cell.fg = to_color(fg);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Candle, Pane, Point, YRange, time::MINUTE};
    use ratatui_core::backend::TestBackend;
    use ratatui_core::terminal::Terminal;

    fn lines(buf: &Buffer) -> Vec<String> {
        (0..buf.area.height).map(|y| (0..buf.area.width).map(|x| buf[(x, y)].symbol().to_string()).collect()).collect()
    }

    #[test]
    fn draws_candles_volume_and_axes_on_a_test_backend() {
        let mut chart = Chart::new();
        let c = chart.add_candles(Chart::MAIN, "XYZ").unwrap();
        let vp = chart.add_pane(Pane { weight: 1.0, y_range: YRange::AutoFromZero, title: None });
        chart.add_volume(vp, c).unwrap();
        for i in 0..40 {
            let o = 100.0 + i as f64;
            chart.push_candle(c, Candle::new(i * MINUTE, o, o + 3.0, o - 2.0, if i % 2 == 0 { o + 2.0 } else { o - 1.0 }, 5.0 + i as f64)).unwrap();
        }
        chart.fit_all();
        let mut term = Terminal::new(TestBackend::new(70, 20)).unwrap();
        term.draw(|f| f.render_widget(ChartWidget::new(&chart), f.area())).unwrap();
        let buf = term.backend().buffer();
        let text = lines(buf).join("\n");
        assert!(text.chars().any(|ch| ('\u{2801}'..='\u{28ff}').contains(&ch)), "{text}");
        assert!(text.contains("00:"), "time labels:\n{text}");
        assert!(text.contains("130") && text.contains("138"), "price labels and the last-value tag:\n{text}");
        // Rising and falling candles use the theme colours.
        let up = to_color(chart.theme().up);
        let down = to_color(chart.theme().down);
        let fgs: Vec<Color> = buf.content().iter().map(|c| c.fg).collect();
        assert!(fgs.contains(&up) && fgs.contains(&down));
        // The last-value tag has a background.
        assert!(buf.content().iter().any(|c| c.bg == up || c.bg == down));
    }

    #[test]
    fn line_chart_snapshot_is_stable() {
        let mut chart = Chart::new();
        let id = chart.add_line(Chart::MAIN, "p", Rgba::hex(0xffffff)).unwrap();
        for t in 0..=60 {
            chart.push_point(id, Point::new(t * 1000, t as f64)).unwrap();
        }
        chart.viewport_mut().set_right_margin(0.0);
        chart.fit_all();
        let area = CellRect::new(0, 0, 40, 10);
        let mut a = Buffer::empty(area);
        ChartWidget::new(&chart).render(area, &mut a);
        let mut b = Buffer::empty(area);
        (&ChartWidget::new(&chart)).render(area, &mut b);
        assert_eq!(a, b);
        let rows = lines(&a);
        // A rising line: the top row's braille sits further right than the bottom row's.
        let first_dot = |row: &str| row.chars().position(|ch| ('\u{2801}'..='\u{28ff}').contains(&ch));
        let top = rows.iter().find_map(|r| first_dot(r)).unwrap();
        let bottom = rows.iter().rev().find_map(|r| first_dot(r)).unwrap();
        assert!(top >= bottom, "{rows:#?}");
    }

    #[test]
    fn pan_and_zoom_by_columns() {
        let mut chart = Chart::new();
        let id = chart.add_line(Chart::MAIN, "p", Rgba::hex(0xffffff)).unwrap();
        for t in 0..=100 {
            chart.push_point(id, Point::new(t * 1000, 0.0)).unwrap();
        }
        chart.fit_all();
        let area = CellRect::new(0, 0, 60, 10);
        let span = chart.viewport().span();
        pan_columns(&mut chart, area, 10);
        assert!(!chart.viewport().is_following());
        zoom_columns(&mut chart, area, 0.5, 20);
        assert!((chart.viewport().span() - span / 2).abs() <= 1);
        // Tiny areas draw nothing and don't panic.
        let tiny = CellRect::new(0, 0, 3, 2);
        let mut buf = Buffer::empty(tiny);
        ChartWidget::new(&chart).render(tiny, &mut buf);
    }
}

//! A [GPUI](https://crates.io/crates/gpui) backend (feature `gpui`).
//!
//! [`ChartView`] is a GPUI view that owns a [`Chart`], draws it with paths, quads and shaped
//! text, pans on drag and horizontal scroll, and zooms on vertical scroll. Live updates go
//! through [`ChartView::update_chart`], which coalesces redraws to the frame rate.
//! [`paint_scene`] draws any [`Scene`] inside your own element or `canvas`.
//!
//! ```no_run
//! use gpui::{App, AppContext, Entity};
//! use weirdnet_chart::{Chart, Point, Rgba, gpui::ChartView};
//!
//! fn build(cx: &mut App) -> Entity<ChartView> {
//!     let mut chart = Chart::new();
//!     chart.add_line(Chart::MAIN, "price", Rgba::hex(0x42a5f5)).unwrap();
//!     cx.new(|_| ChartView::new(chart))
//! }
//!
//! // From a task fed by a market data stream:
//! fn on_tick(view: &Entity<ChartView>, cx: &mut App, t: i64, price: f64) {
//!     view.update(cx, |view, cx| {
//!         view.update_chart(cx, |chart| {
//!             let id = chart.series_ids().next().unwrap();
//!             chart.push_point(id, Point::new(t, price)).ok();
//!         });
//!     });
//! }
//! ```
//!
//! This backend builds against `gpui` from crates.io. An app that pins GPUI to a git
//! revision can point this crate at the same revision with a `[patch.crates-io]` entry, as
//! long as that revision's `gpui` is semver-compatible.

use std::cell::Cell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui::{
    App, Bounds, ContentMask, Context, Hsla, InteractiveElement, IntoElement, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent,
    ParentElement, PathBuilder, Pixels, Render, ScrollWheelEvent, SharedString, Styled, Task, TextRun, Window, canvas, div, fill, point, px, size,
};

use crate::chart::Chart;
use crate::color::Rgba;
use crate::scene::{Align, Metrics, Pos, Primitive, Rect, Scene, Size};
use crate::throttle::{FrameThrottle, Redraw};

/// Layout metrics for a GPUI font size: monospace-ish character width and line height.
pub fn metrics_for(font_size: f32) -> Metrics {
    Metrics { char_width: font_size * 0.62, line_height: (font_size * 1.4).ceil(), padding: (font_size / 3.0).round().max(2.0), min_stroke: 1.0 }
}

fn hsla(c: Rgba) -> Hsla {
    gpui::Rgba { r: c.r, g: c.g, b: c.b, a: c.a }.into()
}

/// Draws `scene`, laid out in pixels, with its top-left corner at `origin`.
pub fn paint_scene(scene: &Scene, origin: gpui::Point<Pixels>, font_size: Pixels, window: &mut Window, cx: &mut App) {
    let at = |p: Pos| point(origin.x + px(p.x), origin.y + px(p.y));
    let rect = |r: Rect| Bounds::new(at(Pos::new(r.x, r.y)), size(px(r.w), px(r.h)));
    let line_height = px(metrics_for(f32::from(font_size)).line_height);
    for layer in &scene.layers {
        let mask = layer.clip.map(|r| ContentMask { bounds: rect(r) });
        window.with_content_mask(mask, |window| {
            for p in &layer.primitives {
                match p {
                    Primitive::Rect { rect: r, color } => window.paint_quad(fill(rect(*r), hsla(*color))),
                    Primitive::Line { from, to, color, width, dashed } => {
                        if !dashed && (from.x == to.x || from.y == to.y) {
                            // Axis-aligned solid lines as crisp quads.
                            let (x0, x1) = (from.x.min(to.x), from.x.max(to.x));
                            let (y0, y1) = (from.y.min(to.y), from.y.max(to.y));
                            let r = if from.x == to.x {
                                Rect::new(x0 - width / 2.0, y0, *width, y1 - y0)
                            } else {
                                Rect::new(x0, y0 - width / 2.0, x1 - x0, *width)
                            };
                            window.paint_quad(fill(rect(r), hsla(*color)));
                        } else {
                            let mut b = PathBuilder::stroke(px(*width));
                            if *dashed {
                                b = b.dash_array(&[px(2.0), px(3.0)]);
                            }
                            b.move_to(at(*from));
                            b.line_to(at(*to));
                            if let Ok(path) = b.build() {
                                window.paint_path(path, hsla(*color));
                            }
                        }
                    }
                    Primitive::Polyline { points, color, width } => {
                        let mut b = PathBuilder::stroke(px(*width));
                        let mut it = points.iter();
                        let Some(first) = it.next() else { continue };
                        b.move_to(at(*first));
                        for p in it {
                            b.line_to(at(*p));
                        }
                        if let Ok(path) = b.build() {
                            window.paint_path(path, hsla(*color));
                        }
                    }
                    Primitive::Area { points, baseline, color } => {
                        let (Some(first), Some(last)) = (points.first(), points.last()) else { continue };
                        let mut b = PathBuilder::fill();
                        b.move_to(at(Pos::new(first.x, *baseline)));
                        for p in points {
                            b.line_to(at(*p));
                        }
                        b.line_to(at(Pos::new(last.x, *baseline)));
                        b.close();
                        if let Ok(path) = b.build() {
                            window.paint_path(path, hsla(*color));
                        }
                    }
                    Primitive::Text { pos, text, color, h_align, v_align } => {
                        let run = TextRun {
                            len: text.len(),
                            font: window.text_style().font(),
                            color: hsla(*color),
                            background_color: None,
                            underline: None,
                            strikethrough: None,
                        };
                        let shaped = window.text_system().shape_line(SharedString::from(text.clone()), font_size, &[run], None);
                        let w = shaped.width;
                        let mut o = at(*pos);
                        match h_align {
                            Align::Start => {}
                            Align::Center => o.x -= w / 2.0,
                            Align::End => o.x -= w,
                        }
                        match v_align {
                            Align::Start => {}
                            Align::Center => o.y -= line_height / 2.0,
                            Align::End => o.y -= line_height,
                        }
                        shaped.paint(o, line_height, window, cx).ok();
                    }
                }
            }
        });
    }
}

/// A GPUI view that draws a [`Chart`], with throttled live updates, drag-to-pan and
/// scroll-to-zoom.
pub struct ChartView {
    chart: Chart,
    throttle: FrameThrottle,
    font_size: f32,
    interactive: bool,
    // The plot's left edge and width in window coordinates, from the last layout.
    plot: Rc<Cell<Option<(f32, f32)>>>,
    drag_x: Option<f32>,
    redraw: Option<Task<()>>,
}

impl ChartView {
    /// A view of `chart` at 60 frames per second with 12-pixel labels.
    pub fn new(chart: Chart) -> ChartView {
        ChartView {
            chart,
            throttle: FrameThrottle::new(60),
            font_size: 12.0,
            interactive: true,
            plot: Rc::new(Cell::new(None)),
            drag_x: None,
            redraw: None,
        }
    }

    /// The chart.
    pub fn chart(&self) -> &Chart {
        &self.chart
    }

    /// Edits the chart; call [`request_redraw`](Self::request_redraw) afterwards, or use
    /// [`update_chart`](Self::update_chart).
    pub fn chart_mut(&mut self) -> &mut Chart {
        &mut self.chart
    }

    /// Edits the chart and schedules a throttled redraw.
    pub fn update_chart<R>(&mut self, cx: &mut Context<Self>, f: impl FnOnce(&mut Chart) -> R) -> R {
        let r = f(&mut self.chart);
        self.request_redraw(cx);
        r
    }

    /// Redraws now, or once the frame interval has passed; bursts collapse into one frame.
    pub fn request_redraw(&mut self, cx: &mut Context<Self>) {
        match self.throttle.request(Instant::now()) {
            Redraw::Now => cx.notify(),
            Redraw::After(delay) => self.schedule(delay, cx),
            Redraw::Scheduled => {}
        }
    }

    fn schedule(&mut self, delay: Duration, cx: &mut Context<Self>) {
        self.redraw = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(delay).await;
            this.update(cx, |view, cx| {
                view.throttle.fire(Instant::now());
                view.redraw = None;
                cx.notify();
            })
            .ok();
        }));
    }

    /// Caps redraws at `fps` frames per second.
    pub fn set_fps(&mut self, fps: u32) {
        self.throttle = FrameThrottle::new(fps);
    }

    /// Whether a throttled redraw is waiting.
    pub fn redraw_pending(&self) -> bool {
        self.throttle.is_pending()
    }

    /// Sets the label font size in pixels.
    pub fn set_font_size(&mut self, font_size: f32) {
        self.font_size = font_size.max(4.0);
    }

    /// Turns mouse panning and zooming on or off.
    pub fn set_interactive(&mut self, interactive: bool) {
        self.interactive = interactive;
    }

    /// The plot's left edge and width in window coordinates from the last layout, if drawn.
    pub fn plot_bounds(&self) -> Option<(f32, f32)> {
        self.plot.get()
    }

    fn on_scroll(&mut self, ev: &ScrollWheelEvent, cx: &mut Context<Self>) {
        let Some((left, width)) = self.plot.get().filter(|_| self.interactive) else { return };
        let d = ev.delta.pixel_delta(px(16.0));
        let (dx, dy) = (f32::from(d.x), f32::from(d.y));
        if dx.abs() > dy.abs() {
            self.chart.pan_pixels(dx as f64, width as f64);
        } else if dy != 0.0 {
            let factor = (-(dy as f64) / 300.0).exp();
            self.chart.zoom_at(factor, (f32::from(ev.position.x) - left) as f64, width as f64);
        }
        self.request_redraw(cx);
    }

    fn on_down(&mut self, ev: &MouseDownEvent) {
        if self.interactive {
            self.drag_x = Some(f32::from(ev.position.x));
        }
    }

    fn on_move(&mut self, ev: &MouseMoveEvent, cx: &mut Context<Self>) {
        let (Some(last), Some((_, width))) = (self.drag_x, self.plot.get()) else { return };
        if ev.pressed_button != Some(MouseButton::Left) {
            self.drag_x = None;
            return;
        }
        let x = f32::from(ev.position.x);
        self.chart.pan_pixels((x - last) as f64, width as f64);
        self.drag_x = Some(x);
        self.request_redraw(cx);
    }

    fn on_up(&mut self, _ev: &MouseUpEvent) {
        self.drag_x = None;
    }
}

impl Render for ChartView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.throttle.frame_drawn(Instant::now());
        let entity = cx.entity();
        let font_size = self.font_size;
        let metrics = metrics_for(font_size);
        let plot = self.plot.clone();
        div()
            .size_full()
            .on_scroll_wheel(cx.listener(|this, ev: &ScrollWheelEvent, _, cx| this.on_scroll(ev, cx)))
            .on_mouse_down(MouseButton::Left, cx.listener(|this, ev: &MouseDownEvent, _, _| this.on_down(ev)))
            .on_mouse_move(cx.listener(|this, ev: &MouseMoveEvent, _, cx| this.on_move(ev, cx)))
            .on_mouse_up(MouseButton::Left, cx.listener(|this, ev: &MouseUpEvent, _, _| this.on_up(ev)))
            .child(
                canvas(
                    move |bounds, _window, cx| {
                        let size = Size::new(f32::from(bounds.size.width), f32::from(bounds.size.height));
                        let scene = entity.read(cx).chart.layout(size, &metrics);
                        plot.set(Some((f32::from(bounds.origin.x) + scene.plot.x, scene.plot.w)));
                        scene
                    },
                    move |bounds, scene, window, cx| paint_scene(&scene, bounds.origin, px(font_size), window, cx),
                )
                .size_full(),
            )
    }
}

#[cfg(all(test, feature = "gpui-test-support"))]
mod tests {
    use super::*;
    use crate::{Candle, Pane, YRange, time::MINUTE};
    use gpui::{Modifiers, ScrollDelta, TestAppContext, TouchPhase};

    fn chart() -> (Chart, crate::SeriesId) {
        let mut chart = Chart::new();
        let c = chart.add_candles(Chart::MAIN, "XYZ").unwrap();
        let vp = chart.add_pane(Pane { weight: 1.0, y_range: YRange::AutoFromZero, title: Some("Volume".into()) });
        chart.add_volume(vp, c).unwrap();
        for i in 0..200 {
            let o = 50.0 + (i as f64 / 10.0).sin() * 4.0;
            chart.push_candle(c, Candle::new(i * MINUTE, o, o + 1.0, o - 1.0, o + 0.5, 3.0)).unwrap();
        }
        chart.show_latest(90 * MINUTE);
        (chart, c)
    }

    #[gpui::test]
    fn lays_out_paints_and_throttles(cx: &mut TestAppContext) {
        let (chart, id) = chart();
        let (view, cx) = cx.add_window_view(|_, _| ChartView::new(chart));
        cx.simulate_resize(size(px(800.0), px(500.0)));
        cx.draw(point(px(0.0), px(0.0)), size(px(800.0), px(500.0)), |_, _| div().size_full().child(view.clone()));
        let plot = view.read_with(cx, |v, _| v.plot_bounds());
        let (left, width) = plot.expect("laid out");
        // The plot leaves room on the right for the price axis.
        assert!(left >= 0.0 && width > 600.0 && width < 790.0, "{left} {width}");

        // A burst of live trades: one redraw now, one delayed, then nothing pending.
        view.update(cx, |v, cx| {
            for i in 0..500 {
                v.update_chart(cx, |c| c.apply_trade(id, 200 * MINUTE + i * 100, 51.0, 0.1, MINUTE).unwrap());
            }
            assert!(v.redraw_pending());
        });
        cx.executor().advance_clock(Duration::from_millis(100));
        cx.run_until_parked();
        view.read_with(cx, |v, _| {
            assert!(!v.redraw_pending());
            assert_eq!(v.chart().candles(id).unwrap().len(), 201);
            assert!(v.chart().viewport().is_following());
        });
    }

    #[gpui::test]
    fn scroll_zooms_and_pans(cx: &mut TestAppContext) {
        let (chart, _) = chart();
        let (view, cx) = cx.add_window_view(|_, _| ChartView::new(chart));
        cx.draw(point(px(0.0), px(0.0)), size(px(800.0), px(500.0)), |_, _| div().size_full().child(view.clone()));
        let span = view.read_with(cx, |v, _| v.chart().viewport().span());
        view.update(cx, |v, cx| {
            let ev = ScrollWheelEvent {
                position: point(px(300.0), px(100.0)),
                delta: ScrollDelta::Pixels(point(px(0.0), px(120.0))),
                modifiers: Modifiers::default(),
                touch_phase: TouchPhase::Moved,
            };
            v.on_scroll(&ev, cx);
            assert!(v.chart().viewport().span() < span);
            let ev = ScrollWheelEvent {
                position: point(px(300.0), px(100.0)),
                delta: ScrollDelta::Pixels(point(px(200.0), px(0.0))),
                modifiers: Modifiers::default(),
                touch_phase: TouchPhase::Moved,
            };
            v.on_scroll(&ev, cx);
            assert!(!v.chart().viewport().is_following());
        });
    }
}

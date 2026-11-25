//! A live feed through the public API: trades become candles, the view follows them, and
//! redraws stay at the frame rate.

use std::time::{Duration, Instant};

use weirdnet_chart::time::{MINUTE, SECOND};
use weirdnet_chart::{Chart, FrameThrottle, Metrics, Pane, Primitive, Redraw, Size, YRange};

#[test]
fn trades_follow_and_redraw_at_frame_rate() {
    let mut chart = Chart::new();
    let candles = chart.add_candles(Chart::MAIN, "XYZ").unwrap();
    let vol = chart.add_pane(Pane { weight: 1.0, y_range: YRange::AutoFromZero, title: None });
    chart.add_volume(vol, candles).unwrap();
    chart.show_latest(30 * MINUTE);

    // 10 000 trades over 50 simulated minutes, delivered within 0.5 s of wall time.
    let mut throttle = FrameThrottle::new(60);
    let t0 = Instant::now();
    let mut frames = 0;
    let mut scheduled = 0;
    for i in 0..10_000i64 {
        let time = i * 300;
        let price = 100.0 + ((i as f64) / 500.0).sin() * 10.0;
        chart.apply_trade(candles, time, price, 0.01, MINUTE).unwrap();
        let now = t0 + Duration::from_micros(i as u64 * 50);
        match throttle.request(now) {
            Redraw::Now => frames += 1,
            Redraw::After(_) => scheduled += 1,
            Redraw::Scheduled => {}
        }
        if throttle.poll(now) {
            frames += 1;
        }
    }
    // 0.5 s at 60 fps: about 30 frames, never one per trade.
    assert!(frames <= 32, "{frames} frames");
    assert!(scheduled >= 1);
    assert_eq!(chart.candles(candles).unwrap().len(), 50);
    assert!(chart.viewport().is_following());
    assert!(chart.viewport().end() >= 49 * MINUTE + 59 * SECOND);

    let scene = chart.layout(Size::new(1000.0, 500.0), &Metrics::PIXELS);
    let bodies = scene
        .layers
        .iter()
        .filter(|l| l.clip == Some(scene.panes[0].rect))
        .flat_map(|l| &l.primitives)
        .filter(|p| matches!(p, Primitive::Rect { .. }))
        .count();
    assert!((30..=32).contains(&bodies), "{bodies} candles visible");
}

#[test]
fn ring_buffers_cap_memory() {
    let mut chart = Chart::new();
    chart.set_default_capacity(1_000);
    let line = chart.add_line(Chart::MAIN, "p", weirdnet_chart::Rgba::hex(0xffffff)).unwrap();
    for t in 0..50_000 {
        chart.push_point(line, weirdnet_chart::Point::new(t, t as f64)).unwrap();
    }
    let s = chart.points(line).unwrap();
    assert_eq!(s.len(), 1_000);
    assert_eq!(s.first().unwrap().time, 49_000);
}

//! Renders a candlestick chart with volume into an in-memory Ratatui buffer and prints it,
//! without touching the terminal's state.
//!
//! `cargo run --example terminal_snapshot --features ratatui`

use ratatui_core::{buffer::Buffer, layout::Rect, widgets::Widget};
use weirdnet_chart::time::MINUTE;
use weirdnet_chart::{Candle, Chart, Pane, YRange, ratatui::ChartWidget};

fn main() {
    let mut chart = Chart::new();
    let candles = chart.add_candles(Chart::MAIN, "XYZ").unwrap();
    let volume = chart.add_pane(Pane { weight: 1.0, y_range: YRange::AutoFromZero, title: Some("Volume".into()) });
    chart.add_volume(volume, candles).unwrap();
    let mut price = 100.0f64;
    for i in 0..90 {
        let open = price;
        price += ((i as f64) * 0.7).sin() * 1.5;
        let (hi, lo) = (open.max(price) + 0.6, open.min(price) - 0.6);
        chart.push_candle(candles, Candle::new(i * MINUTE, open, hi, lo, price, 20.0 + (i % 7) as f64 * 5.0)).unwrap();
    }
    chart.show_latest(60 * MINUTE);

    let area = Rect::new(0, 0, 80, 24);
    let mut buf = Buffer::empty(area);
    ChartWidget::new(&chart).render(area, &mut buf);
    for y in 0..area.height {
        let line: String = (0..area.width).map(|x| buf[(x, y)].symbol().to_string()).collect();
        println!("{}", line.trim_end());
    }
}

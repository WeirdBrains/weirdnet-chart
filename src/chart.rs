//! The chart: panes, series, theme, viewport and live updates.

use std::fmt;

use crate::color::Rgba;
use crate::series::{Candle, Change, OutOfOrder, Point, Series, fold_extent};
use crate::time::{TimeZone, Timestamp};
use crate::viewport::Viewport;

/// Identifies a series within its [`Chart`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SeriesId(pub(crate) usize);

/// Identifies a pane within its [`Chart`]. Pane 0 ([`Chart::MAIN`]) always exists.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PaneId(pub(crate) usize);

/// How a series is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mark {
    /// A polyline through points.
    Line,
    /// A polyline with the area under it filled.
    Area,
    /// A bar from zero per point.
    Bars,
    /// Candlesticks.
    Candles,
    /// OHLC bars (a high-low line with open and close ticks).
    Ohlc,
    /// Volume bars from another candle series, coloured by direction.
    Volume,
}

impl Mark {
    fn holds_points(self) -> bool {
        matches!(self, Mark::Line | Mark::Area | Mark::Bars)
    }
}

/// The vertical range of a pane.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum YRange {
    /// Fit the visible data, with a little headroom.
    Auto,
    /// Fit the visible data and always include zero (bars and volume).
    AutoFromZero,
    /// A fixed `(min, max)`.
    Fixed(f64, f64),
}

/// A horizontal band of the chart with its own y axis. All panes share the time axis.
#[derive(Clone, Debug, PartialEq)]
pub struct Pane {
    /// Relative height compared with the other panes.
    pub weight: f32,
    /// The vertical range.
    pub y_range: YRange,
    /// An optional title drawn in the pane's top-left corner.
    pub title: Option<String>,
}

impl Pane {
    /// An auto-ranged pane with relative height `weight`.
    pub fn new(weight: f32) -> Pane {
        Pane { weight, y_range: YRange::Auto, title: None }
    }
}

/// Per-series drawing options.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SeriesStyle {
    /// Line, area and bar colour. Candles, OHLC and volume use the theme's up and down colours.
    pub color: Rgba,
    /// Line width in backend units (pixels for GPUI).
    pub width: f32,
    /// Show the newest value as a tag on the y axis.
    pub show_last_value: bool,
}

impl SeriesStyle {
    /// A one-unit line in `color`, without a last-value tag.
    pub fn new(color: Rgba) -> SeriesStyle {
        SeriesStyle { color, width: 1.5, show_last_value: false }
    }

    /// The same style with a last-value tag.
    pub fn with_last_value(mut self) -> SeriesStyle {
        self.show_last_value = true;
        self
    }

    /// The same style with a different line width.
    pub fn with_width(mut self, width: f32) -> SeriesStyle {
        self.width = width;
        self
    }
}

/// Colours for everything that isn't a series colour.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Theme {
    /// Filled behind the chart when not transparent.
    pub background: Rgba,
    /// Grid lines.
    pub grid: Rgba,
    /// Axis and pane border lines.
    pub axis: Rgba,
    /// Tick labels and titles.
    pub text: Rgba,
    /// Rising candles and volume.
    pub up: Rgba,
    /// Falling candles and volume.
    pub down: Rgba,
    /// Text on last-value tags.
    pub tag_text: Rgba,
}

impl Theme {
    /// Light text on a dark (transparent by default) background.
    pub const fn dark() -> Theme {
        Theme {
            background: Rgba::TRANSPARENT,
            grid: Rgba::hex(0x2a2e39),
            axis: Rgba::hex(0x50535e),
            text: Rgba::hex(0xb2b5be),
            up: Rgba::hex(0x26a69a),
            down: Rgba::hex(0xef5350),
            tag_text: Rgba::hex(0x101114),
        }
    }

    /// Dark text on a white background.
    pub const fn light() -> Theme {
        Theme {
            background: Rgba::hex(0xffffff),
            grid: Rgba::hex(0xe6e8ee),
            axis: Rgba::hex(0xb4b8c2),
            text: Rgba::hex(0x3b3f4a),
            up: Rgba::hex(0x089981),
            down: Rgba::hex(0xf23645),
            tag_text: Rgba::hex(0xffffff),
        }
    }
}

impl Default for Theme {
    fn default() -> Self {
        Theme::dark()
    }
}

/// Why a chart call failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChartError {
    /// No series with that id.
    UnknownSeries(SeriesId),
    /// No pane with that id.
    UnknownPane(PaneId),
    /// Points pushed to a candle series, or candles to a point series; or a volume series
    /// pointed at something that isn't a candle series.
    WrongKind(SeriesId),
    /// The sample is older than the newest one in the series.
    OutOfOrder(OutOfOrder),
}

impl fmt::Display for ChartError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ChartError::UnknownSeries(id) => write!(f, "unknown series {}", id.0),
            ChartError::UnknownPane(id) => write!(f, "unknown pane {}", id.0),
            ChartError::WrongKind(id) => write!(f, "series {} holds a different kind of data", id.0),
            ChartError::OutOfOrder(e) => e.fmt(f),
        }
    }
}

impl std::error::Error for ChartError {}

impl From<OutOfOrder> for ChartError {
    fn from(e: OutOfOrder) -> Self {
        ChartError::OutOfOrder(e)
    }
}

#[derive(Clone, Debug)]
pub(crate) enum Data {
    Points(Series<Point>),
    Candles(Series<Candle>),
    VolumeOf(SeriesId),
}

#[derive(Clone, Debug)]
pub(crate) struct Entry {
    pub(crate) name: String,
    pub(crate) pane: PaneId,
    pub(crate) mark: Mark,
    pub(crate) style: SeriesStyle,
    pub(crate) data: Data,
    pub(crate) visible: bool,
}

/// A renderer-independent chart: panes stacked vertically over one time axis, each with
/// its own series and y axis.
///
/// Push data as it arrives; [`layout`](Chart::layout) turns the visible part into a
/// [`Scene`](crate::Scene) that a backend draws. Every change bumps
/// [`revision`](Chart::revision).
#[derive(Clone, Debug)]
pub struct Chart {
    pub(crate) panes: Vec<Pane>,
    pub(crate) entries: Vec<Entry>,
    pub(crate) viewport: Viewport,
    pub(crate) theme: Theme,
    pub(crate) tz: TimeZone,
    capacity: usize,
    revision: u64,
}

impl Default for Chart {
    fn default() -> Self {
        Chart::new()
    }
}

impl Chart {
    /// The main pane, created with the chart.
    pub const MAIN: PaneId = PaneId(0);

    /// A chart with one auto-ranged pane, the dark theme, UTC labels and 100 000-sample series.
    pub fn new() -> Chart {
        Chart {
            panes: vec![Pane::new(3.0)],
            entries: Vec::new(),
            viewport: Viewport::default(),
            theme: Theme::default(),
            tz: TimeZone::Utc,
            capacity: 100_000,
            revision: 0,
        }
    }

    fn touch(&mut self) {
        self.revision += 1;
    }

    /// Bumped on every change, including viewport and style changes.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// The theme.
    pub fn theme(&self) -> &Theme {
        &self.theme
    }

    /// Changes the theme.
    pub fn set_theme(&mut self, theme: Theme) {
        self.theme = theme;
        self.touch();
    }

    /// The time zone used for time labels.
    pub fn time_zone(&self) -> TimeZone {
        self.tz
    }

    /// Changes the time zone used for time labels.
    pub fn set_time_zone(&mut self, tz: TimeZone) {
        self.tz = tz;
        self.touch();
    }

    /// The capacity given to series added from now on.
    pub fn set_default_capacity(&mut self, capacity: usize) {
        self.capacity = capacity.max(1);
    }

    /// Adds a pane below the existing ones, with relative height `weight` (the main pane has 3).
    pub fn add_pane(&mut self, pane: Pane) -> PaneId {
        self.panes.push(pane);
        self.touch();
        PaneId(self.panes.len() - 1)
    }

    /// A pane's settings.
    pub fn pane(&self, id: PaneId) -> Option<&Pane> {
        self.panes.get(id.0)
    }

    /// Edits a pane's settings.
    pub fn pane_mut(&mut self, id: PaneId) -> Option<&mut Pane> {
        self.touch();
        self.panes.get_mut(id.0)
    }

    /// The number of panes.
    pub fn pane_count(&self) -> usize {
        self.panes.len()
    }

    /// Adds a series. Volume series take their data from `candles` via
    /// [`add_volume`](Self::add_volume) instead.
    pub fn add_series(&mut self, pane: PaneId, name: impl Into<String>, mark: Mark, style: SeriesStyle) -> Result<SeriesId, ChartError> {
        if pane.0 >= self.panes.len() {
            return Err(ChartError::UnknownPane(pane));
        }
        let data = match mark {
            Mark::Line | Mark::Area | Mark::Bars => Data::Points(Series::with_capacity(self.capacity)),
            Mark::Candles | Mark::Ohlc => Data::Candles(Series::with_capacity(self.capacity)),
            Mark::Volume => return Err(ChartError::WrongKind(SeriesId(self.entries.len()))),
        };
        Ok(self.push_entry(Entry { name: name.into(), pane, mark, style, data, visible: true }))
    }

    fn push_entry(&mut self, entry: Entry) -> SeriesId {
        self.entries.push(entry);
        self.touch();
        SeriesId(self.entries.len() - 1)
    }

    /// Adds a line series in `color` to `pane`.
    pub fn add_line(&mut self, pane: PaneId, name: impl Into<String>, color: Rgba) -> Result<SeriesId, ChartError> {
        self.add_series(pane, name, Mark::Line, SeriesStyle::new(color))
    }

    /// Adds a candlestick series to `pane`, with a last-value tag.
    pub fn add_candles(&mut self, pane: PaneId, name: impl Into<String>) -> Result<SeriesId, ChartError> {
        self.add_series(pane, name, Mark::Candles, SeriesStyle::new(self.theme.text).with_last_value())
    }

    /// Adds volume bars for the candle series `candles` to `pane` (usually a pane of its own
    /// with [`YRange::AutoFromZero`]). The volume shares the candles' data.
    pub fn add_volume(&mut self, pane: PaneId, candles: SeriesId) -> Result<SeriesId, ChartError> {
        if pane.0 >= self.panes.len() {
            return Err(ChartError::UnknownPane(pane));
        }
        match self.entries.get(candles.0).map(|e| &e.data) {
            Some(Data::Candles(_)) => {}
            Some(_) => return Err(ChartError::WrongKind(candles)),
            None => return Err(ChartError::UnknownSeries(candles)),
        }
        let name = format!("{} volume", self.entries[candles.0].name);
        let style = SeriesStyle::new(self.theme.text);
        Ok(self.push_entry(Entry { name, pane, mark: Mark::Volume, style, data: Data::VolumeOf(candles), visible: true }))
    }

    /// Every series id, in the order they were added.
    pub fn series_ids(&self) -> impl Iterator<Item = SeriesId> + use<> {
        (0..self.entries.len()).map(SeriesId)
    }

    /// The number of series.
    pub fn series_count(&self) -> usize {
        self.entries.len()
    }

    /// A series' name.
    pub fn name(&self, id: SeriesId) -> Option<&str> {
        self.entries.get(id.0).map(|e| e.name.as_str())
    }

    /// How a series is drawn.
    pub fn mark(&self, id: SeriesId) -> Option<Mark> {
        self.entries.get(id.0).map(|e| e.mark)
    }

    /// A series' style.
    pub fn style(&self, id: SeriesId) -> Option<&SeriesStyle> {
        self.entries.get(id.0).map(|e| &e.style)
    }

    /// Changes a series' style.
    pub fn set_style(&mut self, id: SeriesId, style: SeriesStyle) -> Result<(), ChartError> {
        let e = self.entries.get_mut(id.0).ok_or(ChartError::UnknownSeries(id))?;
        e.style = style;
        self.touch();
        Ok(())
    }

    /// Changes the mark of a series between kinds that hold the same data
    /// (line, area and bars; or candles and OHLC).
    pub fn set_mark(&mut self, id: SeriesId, mark: Mark) -> Result<(), ChartError> {
        let e = self.entries.get_mut(id.0).ok_or(ChartError::UnknownSeries(id))?;
        let ok = match e.data {
            Data::Points(_) => mark.holds_points(),
            Data::Candles(_) => matches!(mark, Mark::Candles | Mark::Ohlc),
            Data::VolumeOf(_) => mark == Mark::Volume,
        };
        if !ok {
            return Err(ChartError::WrongKind(id));
        }
        e.mark = mark;
        self.touch();
        Ok(())
    }

    /// Shows or hides a series.
    pub fn set_visible(&mut self, id: SeriesId, visible: bool) -> Result<(), ChartError> {
        let e = self.entries.get_mut(id.0).ok_or(ChartError::UnknownSeries(id))?;
        e.visible = visible;
        self.touch();
        Ok(())
    }

    /// A point series' data.
    pub fn points(&self, id: SeriesId) -> Option<&Series<Point>> {
        match &self.entries.get(id.0)?.data {
            Data::Points(s) => Some(s),
            _ => None,
        }
    }

    /// A candle series' data (also for a volume series, which shares it).
    pub fn candles(&self, id: SeriesId) -> Option<&Series<Candle>> {
        match &self.entries.get(id.0)?.data {
            Data::Candles(s) => Some(s),
            Data::VolumeOf(src) => self.candles(*src),
            _ => None,
        }
    }

    fn points_series_mut(&mut self, id: SeriesId) -> Result<&mut Series<Point>, ChartError> {
        match &mut self.entries.get_mut(id.0).ok_or(ChartError::UnknownSeries(id))?.data {
            Data::Points(s) => Ok(s),
            _ => Err(ChartError::WrongKind(id)),
        }
    }

    fn candle_series_mut(&mut self, id: SeriesId) -> Result<&mut Series<Candle>, ChartError> {
        match &mut self.entries.get_mut(id.0).ok_or(ChartError::UnknownSeries(id))?.data {
            Data::Candles(s) => Ok(s),
            _ => Err(ChartError::WrongKind(id)),
        }
    }

    /// Edits a point series directly (bulk loads, capacity changes).
    pub fn points_mut(&mut self, id: SeriesId) -> Result<&mut Series<Point>, ChartError> {
        self.touch();
        self.points_series_mut(id)
    }

    /// Edits a candle series directly.
    pub fn candles_mut(&mut self, id: SeriesId) -> Result<&mut Series<Candle>, ChartError> {
        self.touch();
        self.candle_series_mut(id)
    }

    fn after_data_change(&mut self) {
        self.touch();
        if self.viewport.is_following()
            && let Some(latest) = self.latest_time()
        {
            self.viewport.follow_to(latest);
        }
    }

    /// Appends a point (or replaces the newest one with the same time).
    pub fn push_point(&mut self, id: SeriesId, point: Point) -> Result<Change, ChartError> {
        let change = self.points_series_mut(id)?.push(point)?;
        self.after_data_change();
        Ok(change)
    }

    /// Appends a candle (or replaces the newest one with the same time).
    pub fn push_candle(&mut self, id: SeriesId, candle: Candle) -> Result<Change, ChartError> {
        let change = self.candle_series_mut(id)?.push(candle)?;
        self.after_data_change();
        Ok(change)
    }

    /// Folds a trade into a candle series with `interval_ms` bars.
    pub fn apply_trade(&mut self, id: SeriesId, time: Timestamp, price: f64, size: f64, interval_ms: i64) -> Result<Change, ChartError> {
        let change = self.candle_series_mut(id)?.apply_trade(time, price, size, interval_ms)?;
        self.after_data_change();
        Ok(change)
    }

    /// Replaces a point series' contents (history loads); sorts by time.
    pub fn set_points(&mut self, id: SeriesId, points: Vec<Point>) -> Result<(), ChartError> {
        self.points_series_mut(id)?.replace_all(points);
        self.after_data_change();
        Ok(())
    }

    /// Replaces a candle series' contents (history loads); sorts by time.
    pub fn set_candles(&mut self, id: SeriesId, candles: Vec<Candle>) -> Result<(), ChartError> {
        self.candle_series_mut(id)?.replace_all(candles);
        self.after_data_change();
        Ok(())
    }

    /// The newest sample time across all series.
    pub fn latest_time(&self) -> Option<Timestamp> {
        self.entries
            .iter()
            .filter_map(|e| match &e.data {
                Data::Points(s) => s.last().map(|p| p.time),
                Data::Candles(s) => s.last().map(|c| c.time + s.interval().unwrap_or(0)),
                Data::VolumeOf(_) => None,
            })
            .max()
    }

    /// The oldest sample time across all series.
    pub fn earliest_time(&self) -> Option<Timestamp> {
        self.entries
            .iter()
            .filter_map(|e| match &e.data {
                Data::Points(s) => s.first().map(|p| p.time),
                Data::Candles(s) => s.first().map(|c| c.time),
                Data::VolumeOf(_) => None,
            })
            .min()
    }

    /// The viewport.
    pub fn viewport(&self) -> &Viewport {
        &self.viewport
    }

    /// Edits the viewport directly.
    pub fn viewport_mut(&mut self) -> &mut Viewport {
        self.touch();
        &mut self.viewport
    }

    /// Shows the last `span` milliseconds of data and follows new data.
    pub fn show_latest(&mut self, span: i64) {
        let end = self.latest_time().unwrap_or(0);
        self.viewport.set_range(end - span, end);
        self.viewport.set_following(true);
        self.viewport.follow_to(end);
        self.touch();
    }

    /// Shows all data.
    pub fn fit_all(&mut self) {
        if let (Some(a), Some(b)) = (self.earliest_time(), self.latest_time()) {
            self.viewport.set_range(a, b.max(a + 1));
            self.viewport.set_following(true);
            self.viewport.follow_to(b);
        }
        self.touch();
    }

    fn refollow_if_at_edge(&mut self) {
        if let Some(latest) = self.latest_time()
            && self.viewport.end() >= latest
        {
            self.viewport.set_following(true);
            self.viewport.follow_to(latest);
        }
    }

    /// Pans for a drag of `dx` pixels across a plot `width` pixels wide (dragging right shows
    /// earlier data). Panning back to the newest data resumes following.
    pub fn pan_pixels(&mut self, dx: f64, width: f64) {
        self.viewport.pan_pixels(dx, width);
        self.refollow_if_at_edge();
        self.touch();
    }

    /// Zooms by `factor` (below 1 zooms in) around pixel `x` of a plot `width` pixels wide.
    /// While following, the newest data stays at the right edge.
    pub fn zoom_at(&mut self, factor: f64, x: f64, width: f64) {
        if self.viewport.is_following() {
            self.viewport.zoom(factor, 1.0);
            if let Some(latest) = self.latest_time() {
                self.viewport.follow_to(latest);
            }
        } else {
            self.viewport.zoom_at_pixel(factor, x, width);
            self.refollow_if_at_edge();
        }
        self.touch();
    }

    pub(crate) fn entry_extent(&self, e: &Entry, start: Timestamp, end: Timestamp) -> Option<(f64, f64)> {
        match &e.data {
            Data::Points(s) => {
                let ext = s.extent(start, end)?;
                Some(if e.mark == Mark::Bars { (ext.0.min(0.0), ext.1.max(0.0)) } else { ext })
            }
            Data::Candles(s) => s.extent(start, end),
            Data::VolumeOf(src) => {
                let s = self.candles(*src)?;
                fold_extent(s.range(start, end).map(|c| (0.0, c.volume)))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_are_checked() {
        let mut c = Chart::new();
        let line = c.add_line(Chart::MAIN, "price", Rgba::hex(0xffffff)).unwrap();
        let candles = c.add_candles(Chart::MAIN, "BTC").unwrap();
        assert_eq!(c.push_candle(line, Candle::from_trade(0, 1.0, 1.0)), Err(ChartError::WrongKind(line)));
        assert_eq!(c.push_point(candles, Point::new(0, 1.0)), Err(ChartError::WrongKind(candles)));
        assert_eq!(c.add_volume(Chart::MAIN, line), Err(ChartError::WrongKind(line)));
        assert_eq!(c.add_line(PaneId(9), "x", Rgba::hex(0)), Err(ChartError::UnknownPane(PaneId(9))));
        let vol = c.add_volume(Chart::MAIN, candles).unwrap();
        assert_eq!(c.name(vol), Some("BTC volume"));
        assert_eq!(c.set_mark(line, Mark::Candles), Err(ChartError::WrongKind(line)));
        c.set_mark(line, Mark::Area).unwrap();
        c.push_candle(candles, Candle::new(60, 1.0, 2.0, 0.5, 1.5, 7.0)).unwrap();
        assert_eq!(c.candles(vol).unwrap().len(), 1);
    }

    #[test]
    fn live_data_scrolls_while_following() {
        let mut c = Chart::new();
        let line = c.add_line(Chart::MAIN, "p", Rgba::hex(0xffffff)).unwrap();
        c.viewport_mut().set_right_margin(0.0);
        c.show_latest(1000);
        for t in 0..=5000 {
            c.push_point(line, Point::new(t, t as f64)).unwrap();
        }
        assert_eq!((c.viewport().start(), c.viewport().end()), (4000, 5000));
        c.pan_pixels(100.0, 1000.0);
        assert!(!c.viewport().is_following());
        c.push_point(line, Point::new(5001, 0.0)).unwrap();
        assert_eq!(c.viewport().end(), 4900);
        c.pan_pixels(-200.0, 1000.0);
        assert!(c.viewport().is_following());
        assert_eq!(c.viewport().end(), 5001);
    }

    #[test]
    fn revision_bumps() {
        let mut c = Chart::new();
        let r0 = c.revision();
        let id = c.add_line(Chart::MAIN, "p", Rgba::hex(0)).unwrap();
        c.push_point(id, Point::new(1, 1.0)).unwrap();
        assert!(c.revision() > r0);
    }
}

//! A speed graph: about the last minute of speeds as a filled curve, newest at the right edge with
//! a dot, in one colour.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use gpui_kit::{Bounds, Hsla, PathBuilder, Pixels, Point, Window, canvas, fill, point, px};

/// About a minute at one a second.
const KEPT: usize = 60;
const SAMPLE_EVERY: Duration = Duration::from_secs(1);

/// The speeds seen, oldest first, at most one a second.
#[derive(Default)]
pub struct SpeedHistory {
    speeds: VecDeque<u64>,
    last: Option<Instant>,
}

impl SpeedHistory {
    /// Keeps `speed`; within a second of the last sample it replaces that sample.
    pub fn keep(&mut self, speed: u64, now: Instant) {
        let fresh = self
            .last
            .is_none_or(|last| now.duration_since(last) >= SAMPLE_EVERY);
        if !fresh && let Some(latest) = self.speeds.back_mut() {
            *latest = speed;
            return;
        }
        if self.speeds.len() == KEPT {
            self.speeds.pop_front();
        }
        self.speeds.push_back(speed);
        self.last = Some(now);
    }

    /// Enough samples to draw a line.
    pub fn drawable(&self) -> bool {
        self.speeds.len() > 1
    }

    pub fn speeds(&self) -> Vec<u64> {
        self.speeds.iter().copied().collect()
    }
}

/// Draws `speeds` filling the element; the caller sizes it.
pub fn speed_graph(speeds: Vec<u64>, color: Hsla) -> gpui_kit::Canvas<()> {
    canvas(
        |_, _, _| (),
        move |bounds, (), window, _| paint(&speeds, bounds, color, window),
    )
}

fn paint(speeds: &[u64], bounds: Bounds<Pixels>, color: Hsla, window: &mut Window) {
    let dot = px(1.75);
    // Room for the dot, and a quarter above the fastest so a steady speed stays off the top.
    let area = Bounds::new(
        bounds.origin + point(px(0.), dot),
        bounds.size - gpui_kit::size(dot, dot),
    );
    let top = (speeds.iter().copied().max().unwrap_or_default() / 4 * 5).max(1);
    let last = speeds.len().saturating_sub(1).max(1);
    let points: Vec<Point<Pixels>> = speeds
        .iter()
        .enumerate()
        .map(|(index, speed)| {
            let across = thousandths(index as u128, last as u128);
            let up = thousandths(u128::from(*speed), u128::from(top));
            point(
                area.origin.x + area.size.width * across,
                area.origin.y + area.size.height * (1. - up),
            )
        })
        .collect();
    let (Some(&first), Some(&newest)) = (points.first(), points.last()) else {
        return;
    };
    let bottom = bounds.origin.y + bounds.size.height;
    let mut under = PathBuilder::fill();
    under.move_to(point(first.x, bottom));
    under.line_to(first);
    let mut line = PathBuilder::stroke(px(1.3));
    line.move_to(first);
    // Through the midpoints, each corner a control point: a smooth curve on the same samples.
    for pair in points.windows(2) {
        let middle = point((pair[0].x + pair[1].x) / 2., (pair[0].y + pair[1].y) / 2.);
        under.curve_to(middle, pair[0]);
        line.curve_to(middle, pair[0]);
    }
    under.line_to(newest);
    line.line_to(newest);
    under.line_to(point(newest.x, bottom));
    under.close();
    if let Ok(path) = under.build() {
        window.paint_path(path, color.opacity(0.15));
    }
    if let Ok(path) = line.build() {
        window.paint_path(path, color);
    }
    window.paint_quad(
        fill(
            Bounds::new(newest - point(dot, dot), gpui_kit::size(dot * 2., dot * 2.)),
            color,
        )
        .corner_radii(dot),
    );
}

/// `part` of `whole` to a thousandth, as a fraction.
fn thousandths(part: u128, whole: u128) -> f32 {
    let scaled = u16::try_from((part * 1000) / whole.max(1)).unwrap_or(1000);
    f32::from(scaled.min(1000)) / 1000.
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn samples_within_a_second_replace_the_last_one() {
        let start = Instant::now();
        let mut history = SpeedHistory::default();
        history.keep(10, start);
        history.keep(20, start + Duration::from_millis(400));
        history.keep(30, start + Duration::from_secs(1));
        assert_eq!(history.speeds(), [20, 30]);
    }

    #[test]
    fn only_the_last_minute_is_kept() {
        let start = Instant::now();
        let mut history = SpeedHistory::default();
        for second in 0..=KEPT as u64 {
            history.keep(second, start + Duration::from_secs(second));
        }
        assert_eq!(history.speeds().len(), KEPT);
        assert_eq!(history.speeds().first(), Some(&1));
    }
}

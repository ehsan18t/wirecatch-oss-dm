//! The app's mark, drawn in the accent so it follows the theme (D108). Its strokes come from the
//! file the extension's mark and the icons are drawn from.

use std::sync::LazyLock;

use gpui_kit::{
    Hsla, IntoElement, PathBuilder, Pixels, Point, Styled as _, Window, canvas, point, px,
};
use serde::Deserialize;

#[derive(Deserialize)]
struct Mark {
    #[serde(rename = "viewBox")]
    view_box: [f32; 4],
    strokes: Vec<Stroke>,
}

#[derive(Deserialize)]
struct Stroke {
    points: Vec<[f32; 2]>,
    width: f32,
}

static MARK: LazyLock<Option<Mark>> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../../../../packages/ui/src/mark.json"))
        .inspect_err(|e| tracing::error!("the app's mark could not be read: {e}"))
        .ok()
});

/// The mark `height` tall, in `color`.
pub fn mark(height: Pixels, color: Hsla) -> impl IntoElement {
    let [_, _, width, tall] = MARK.as_ref().map_or([0., 0., 1., 1.], |mark| mark.view_box);
    canvas(
        |_, _, _| (),
        move |bounds, (), window, _| paint(bounds.origin, height / tall, color, window),
    )
    .w(height * (width / tall))
    .h(height)
}

/// Each stroke as a line with round ends and corners: GPUI strokes end square, so a dot of the
/// stroke's width sits on every point.
fn paint(origin: Point<Pixels>, scale: Pixels, color: Hsla, window: &mut Window) {
    let Some(mark) = MARK.as_ref() else {
        return;
    };
    let [left, top, _, _] = mark.view_box;
    let at = |[x, y]: [f32; 2]| point(origin.x + scale * (x - left), origin.y + scale * (y - top));
    for stroke in &mark.strokes {
        let mut line = PathBuilder::stroke(scale * stroke.width);
        let mut dots = PathBuilder::fill();
        let radius = scale * (stroke.width / 2.);
        for (index, &spot) in stroke.points.iter().enumerate() {
            let centre = at(spot);
            if index == 0 {
                line.move_to(centre);
            } else {
                line.line_to(centre);
            }
            let side = point(radius, radius);
            dots.move_to(point(centre.x + radius, centre.y));
            dots.arc_to(
                side,
                px(0.),
                false,
                true,
                point(centre.x - radius, centre.y),
            );
            dots.arc_to(
                side,
                px(0.),
                false,
                true,
                point(centre.x + radius, centre.y),
            );
            dots.close();
        }
        for path in [line.build(), dots.build()].into_iter().flatten() {
            window.paint_path(path, color);
        }
    }
}

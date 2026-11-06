//! A rounded square with the picture of what a queue is doing, with a badge in its corner when
//! the queue adds downloads by its rules, and in the rail a ring of how far a running queue got.

use gpui_kit::component::{ActiveTheme as _, Icon, Sizable as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    App, Div, Hsla, InteractiveElement as _, IntoElement, ParentElement as _, PathBuilder, Pixels,
    RenderOnce, SharedString, Styled as _, Window, canvas, div, point, px,
};

use super::tokens;

const SIZE: f32 = 30.;
const BADGE: f32 = 16.;
/// How strongly a tile's tone tints it.
const TINT: f32 = 0.15;
/// The ring's gap from the tile, and its stroke.
const RING_GAP: f32 = 1.;
const RING: f32 = 2.;
/// Points along each rounded corner of the ring.
const CORNER_STEPS: u8 = 8;

/// A toned tile's fill: `tone` mixed once onto `sidebar`, opaque, so a highlighted or hovered
/// item behind it cannot darken it further.
pub fn tinted(sidebar: Hsla, tone: Hsla) -> Hsla {
    sidebar.blend(tone.opacity(TINT))
}

#[derive(IntoElement)]
pub struct QueueTile {
    icon: Icon,
    tone: Option<Hsla>,
    /// Its rules: on, off, or none.
    rules: Option<bool>,
    /// The row's colour, cut around the badge; a hover colour for the row's group.
    behind: Hsla,
    hover: Option<(SharedString, Hsla)>,
    /// How much of a running queue is done, from 0 to 1, drawn as a ring around the tile.
    ring: Option<f32>,
}

impl QueueTile {
    pub fn new(icon: impl Into<Icon>, behind: Hsla) -> Self {
        Self {
            icon: icon.into(),
            tone: None,
            rules: None,
            behind,
            hover: None,
            ring: None,
        }
    }

    /// A ring around the tile, `done` of the way round clockwise from the top.
    pub fn ring(mut self, done: f32) -> Self {
        self.ring = Some(done.clamp(0., 1.));
        self
    }

    /// Tints the tile, as the colour of the queue's state.
    pub fn tone(mut self, tone: Hsla) -> Self {
        self.tone = Some(tone);
        self
    }

    pub fn rules(mut self, rules: Option<bool>) -> Self {
        self.rules = rules;
        self
    }

    /// The badge's cut follows the row's hover colour, for a row in `group`.
    pub fn hover_behind(mut self, group: impl Into<SharedString>, color: Hsla) -> Self {
        self.hover = Some((group.into(), color));
        self
    }
}

/// The corner badge of a queue that adds downloads by its rules, `on` or off, placed by the caller; its border cuts it out of `behind`, or of the hover colour of a row in the group.
fn rules_badge(on: bool, behind: Hsla, hover: Option<(SharedString, Hsla)>, cx: &App) -> Div {
    let theme = cx.theme();
    let (icon, bg, fg) = if on {
        (
            gpui_kit::assets::IconName::Zap,
            theme.primary,
            theme.primary_foreground,
        )
    } else {
        (
            gpui_kit::assets::IconName::ZapOff,
            theme.secondary_active,
            theme.muted_foreground,
        )
    };
    div()
        .absolute()
        .size(px(BADGE))
        .flex()
        .items_center()
        .justify_center()
        .rounded_full()
        .bg(bg)
        .text_color(fg)
        .border_2()
        .border_color(behind)
        .when_some(hover, |badge, (group, color)| {
            badge.group_hover(group, move |style| style.border_color(color))
        })
        .child(Icon::new(icon).with_size(px(BADGE - 7.)))
}

impl RenderOnce for QueueTile {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let badge = self.rules.map(|on| {
            rules_badge(on, self.behind, self.hover.clone(), cx)
                .right(px(-5.))
                .bottom(px(-5.))
        });
        let tone = self.tone;
        let ring = self
            .ring
            .map(|done| progress_ring(done, theme.radius, theme.primary, theme.secondary_active));
        div()
            .relative()
            .size(px(SIZE))
            .flex_shrink_0()
            .flex()
            .items_center()
            .justify_center()
            .rounded(theme.radius)
            .bg(tone.map_or(theme.secondary, |tone| tinted(theme.sidebar, tone)))
            .text_color(tone.unwrap_or(theme.muted_foreground))
            .child(self.icon.with_size(tokens::ICON))
            .children(ring)
            .children(badge)
    }
}

/// The ring around a tile with corners of `radius`: a track all round, and `done` of it in
/// `color`.
fn progress_ring(done: f32, radius: Pixels, color: Hsla, track: Hsla) -> impl IntoElement {
    let out = RING_GAP + RING;
    let side = SIZE + 2. * out;
    let corner = f32::from(radius) + out - RING / 2.;
    canvas(
        |_, _, _| (),
        move |bounds, (), window, _| {
            let at =
                |(x, y): (f32, f32)| bounds.origin + point(px(x + RING / 2.), px(y + RING / 2.));
            for (part, paint) in [(1., track), (done, color)] {
                let points = ring_points(side - RING, corner, part);
                let [first, rest @ ..] = points.as_slice() else {
                    continue;
                };
                let mut line = PathBuilder::stroke(px(RING));
                line.move_to(at(*first));
                for &each in rest {
                    line.line_to(at(each));
                }
                if let Ok(path) = line.build() {
                    window.paint_path(path, paint);
                }
            }
        },
    )
    .absolute()
    .top(px(-out))
    .left(px(-out))
    .size(px(side))
}

/// The points along a rounded square `side` wide with corners of `radius`, clockwise from the
/// middle of its top edge, as far as `done` of the way round.
fn ring_points(side: f32, radius: f32, done: f32) -> Vec<(f32, f32)> {
    use std::f32::consts::FRAC_PI_2;
    let r = radius.min(side / 2.);
    let far = side - r;
    let corners = [
        (far, r, -FRAC_PI_2),
        (far, far, 0.),
        (r, far, FRAC_PI_2),
        (r, r, std::f32::consts::PI),
    ];
    let mut round = vec![(side / 2., 0.)];
    for (cx, cy, from) in corners {
        for step in 0..=CORNER_STEPS {
            let angle = from + FRAC_PI_2 * f32::from(step) / f32::from(CORNER_STEPS);
            round.push((cx + r * angle.cos(), cy + r * angle.sin()));
        }
    }
    round.push((side / 2., 0.));
    let length = |a: (f32, f32), b: (f32, f32)| (b.0 - a.0).hypot(b.1 - a.1);
    let total: f32 = round
        .windows(2)
        .map(|pair| match pair {
            &[from, to] => length(from, to),
            _ => 0.,
        })
        .sum();
    let mut left = total * done;
    let mut drawn = vec![(side / 2., 0.)];
    for pair in round.windows(2) {
        let &[from, to] = pair else { continue };
        if left <= 0. {
            break;
        }
        let step = length(from, to);
        if step <= left {
            drawn.push(to);
            left -= step;
        } else {
            let part = left / step;
            drawn.push((
                from.0 + (to.0 - from.0) * part,
                from.1 + (to.1 - from.1) * part,
            ));
            left = 0.;
        }
    }
    if drawn.len() < 2 { Vec::new() } else { drawn }
}

#[cfg(test)]
mod tests {
    use super::ring_points;

    fn near(a: (f32, f32), b: (f32, f32)) -> bool {
        (a.0 - b.0).abs() < 0.5 && (a.1 - b.1).abs() < 0.5
    }

    #[test]
    fn the_ring_runs_clockwise_from_the_top() {
        assert_eq!(ring_points(34., 9., 0.), Vec::<(f32, f32)>::new());
        let quarter = ring_points(34., 9., 0.25);
        assert!(near(quarter[0], (17., 0.)));
        let ends_at = |done, at| {
            ring_points(34., 9., done)
                .last()
                .is_some_and(|&end| near(end, at))
        };
        assert!(ends_at(0.25, (34., 17.)), "{quarter:?}");
        assert!(ends_at(0.5, (17., 34.)));
        assert!(ends_at(1., (17., 0.)));
    }
}

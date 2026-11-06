//! A path on one line, cut in the middle so its last folder or file name stays in view (D239).

use gpui_kit::{
    App, Bounds, Element, ElementId, GlobalElementId, InspectorElementId, InteractiveElement as _,
    IntoElement, LayoutId, ParentElement as _, Pixels, RenderOnce, ShapedLine, SharedString,
    StatefulInteractiveElement as _, Style, Styled as _, TextAlign, TextStyle, Window, div, px,
    relative,
};

use super::tooltip;

const ELLIPSIS: &str = "…";

#[derive(IntoElement)]
pub struct PathText {
    path: SharedString,
    tooltip: Option<ElementId>,
}

impl PathText {
    pub fn new(path: impl Into<SharedString>) -> Self {
        Self {
            path: path.into(),
            tooltip: None,
        }
    }

    /// Shows the whole path on hover; `id` names the element, which a tooltip needs.
    pub fn tooltip(mut self, id: impl Into<ElementId>) -> Self {
        self.tooltip = Some(id.into());
        self
    }
}

impl RenderOnce for PathText {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let line = PathLine {
            path: self.path.clone(),
        };
        match self.tooltip {
            Some(id) => div()
                .id(id)
                .flex()
                .min_w_0()
                .max_w_full()
                .child(line)
                .tooltip(tooltip::plain(self.path))
                .into_any_element(),
            None => line.into_any_element(),
        }
    }
}

/// `path` split before its last part: `C:\dl` and `\Video`. A path with one part is all tail.
fn split(path: &str) -> (&str, &str) {
    let end = path.trim_end_matches(['\\', '/']).len();
    match path[..end].rfind(['\\', '/']) {
        Some(at) if at > 0 => path.split_at(at),
        _ => ("", path),
    }
}

/// What of `path` fits in `room`, as `measure` sizes a line: the head cut at its end before the
/// last part, and the last part cut in its own middle only once no head is left.
fn fit(path: &str, room: f32, measure: impl Fn(&str) -> f32) -> String {
    if measure(path) <= room {
        return path.to_owned();
    }
    let (head, tail) = split(path);
    let head: Vec<char> = head.chars().collect();
    let with_head = |kept: usize| {
        let mut line: String = head.iter().take(kept).collect();
        line.push_str(ELLIPSIS);
        line.push_str(tail);
        line
    };
    if !head.is_empty() && measure(&with_head(0)) <= room {
        let kept = most_kept(head.len(), |kept| measure(&with_head(kept)) <= room);
        return with_head(kept);
    }
    let tail: Vec<char> = tail.chars().collect();
    let cut = |kept: usize| {
        let front = kept.div_ceil(2);
        let back = kept - front;
        let mut line: String = tail.iter().take(front).collect();
        line.push_str(ELLIPSIS);
        line.extend(tail.iter().skip(tail.len() - back));
        line
    };
    cut(most_kept(tail.len(), |kept| measure(&cut(kept)) <= room))
}

/// The largest count up to `most` that `fits`, given that fitting holds for every smaller count.
fn most_kept(most: usize, fits: impl Fn(usize) -> bool) -> usize {
    let (mut low, mut high) = (0, most);
    while low < high {
        let middle = (low + high).div_ceil(2);
        if fits(middle) {
            low = middle;
        } else {
            high = middle - 1;
        }
    }
    low
}

/// The line itself: as wide as the whole path, shrinking to its box, shaped as one run so the
/// ellipsis sits against the last part.
struct PathLine {
    path: SharedString,
}

struct Measured {
    style: TextStyle,
    font_size: Pixels,
    line_height: Pixels,
}

impl Measured {
    fn shape(&self, text: &str, window: &Window) -> ShapedLine {
        let run = self.style.to_run(text.len());
        window.text_system().shape_line(
            SharedString::from(text.to_owned()),
            self.font_size,
            &[run],
            None,
        )
    }
}

impl IntoElement for PathLine {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for PathLine {
    type RequestLayoutState = Measured;
    type PrepaintState = ShapedLine;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Measured) {
        let style = window.text_style();
        let rem = window.rem_size();
        let measured = Measured {
            font_size: style.font_size.to_pixels(rem),
            line_height: style.line_height_in_pixels(rem),
            style,
        };
        let whole = measured.shape(&self.path, window).width;
        let mut layout = Style::default();
        layout.size.width = whole.into();
        layout.size.height = measured.line_height.into();
        layout.min_size.width = px(0.).into();
        layout.max_size.width = relative(1.).into();
        layout.flex_shrink = 1.;
        (window.request_layout(layout, [], cx), measured)
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        measured: &mut Measured,
        window: &mut Window,
        _: &mut App,
    ) -> ShapedLine {
        let shown = fit(&self.path, f32::from(bounds.size.width), |line| {
            f32::from(measured.shape(line, window).width)
        });
        measured.shape(&shown, window)
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        measured: &mut Measured,
        line: &mut ShapedLine,
        window: &mut Window,
        cx: &mut App,
    ) {
        if let Err(error) = line.paint(
            bounds.origin,
            measured.line_height,
            TextAlign::Left,
            None,
            window,
            cx,
        ) {
            tracing::warn!("a path could not be drawn: {error}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{fit, split};

    fn by_chars(line: &str) -> f32 {
        f32::from(u16::try_from(line.chars().count()).unwrap_or(u16::MAX))
    }

    #[test]
    fn the_last_part_is_kept_with_its_separator() {
        assert_eq!(
            split(r"C:\Users\someone\Downloads\Video"),
            (r"C:\Users\someone\Downloads", r"\Video")
        );
        assert_eq!(
            split("/home/someone/file.zip"),
            ("/home/someone", "/file.zip")
        );
        assert_eq!(split(r"C:\dl\Video\"), (r"C:\dl", r"\Video\"));
    }

    #[test]
    fn a_path_of_one_part_is_all_tail() {
        assert_eq!(split(r"C:\"), ("", r"C:\"));
        assert_eq!(split("/"), ("", "/"));
        assert_eq!(split("/home"), ("", "/home"));
        assert_eq!(split("Download folder"), ("", "Download folder"));
        assert_eq!(split(""), ("", ""));
    }

    #[test]
    fn a_path_that_fits_is_shown_whole() {
        let path = r"C:\dl\Video";
        assert_eq!(fit(path, by_chars(path), by_chars), path);
    }

    #[test]
    fn the_head_is_cut_against_the_last_part() {
        let path = r"C:\Users\someone\Downloads\Video";
        assert_eq!(fit(path, 15., by_chars), r"C:\Users…\Video");
        assert_eq!(fit(path, 7., by_chars), r"…\Video");
    }

    #[test]
    fn only_a_last_part_too_long_alone_is_cut_in_its_middle() {
        let path = r"C:\dl\a-very-long-file-name.zip";
        let shown = fit(path, 12., by_chars);
        assert_eq!(shown, "\\a-ver…e.zip");
        assert!(by_chars(&shown) <= 12.);
        assert_eq!(fit("Download folder", 9., by_chars), "Down…lder");
    }

    #[test]
    fn no_room_leaves_only_the_ellipsis() {
        assert_eq!(fit(r"C:\dl\Video", 1., by_chars), "…");
        assert_eq!(fit(r"C:\dl\Video", 0., by_chars), "…");
    }
}

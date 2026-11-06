//! A path on one line, cut in the middle so its last folder or file name stays in view (D239).

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    App, ElementId, InteractiveElement as _, IntoElement, ParentElement as _, RenderOnce,
    SharedString, StatefulInteractiveElement as _, Styled as _, Window, div,
};

use super::tooltip;

/// Lets the last part give way only once the head is cut to nothing.
const TAIL_SHRINK: f32 = 0.001;

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

/// `path` split before its last part: `C:\dl` and `\Video`. A path with one part is all tail.
fn split(path: &str) -> (&str, &str) {
    let end = path.trim_end_matches(['\\', '/']).len();
    match path[..end].rfind(['\\', '/']) {
        Some(at) if at > 0 => path.split_at(at),
        _ => ("", path),
    }
}

impl RenderOnce for PathText {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let (head, tail) = split(&self.path);
        let (head, tail) = (
            SharedString::from(head.to_owned()),
            SharedString::from(tail.to_owned()),
        );
        let line = div()
            .flex()
            .min_w_0()
            .max_w_full()
            .overflow_hidden()
            .whitespace_nowrap()
            .when(!head.is_empty(), |line| {
                line.child(div().min_w_0().truncate().child(head))
            })
            .child(
                div()
                    .min_w_0()
                    .flex_shrink(TAIL_SHRINK)
                    .overflow_hidden()
                    .text_ellipsis_middle()
                    .child(tail),
            );
        match self.tooltip {
            Some(id) => line
                .id(id)
                .tooltip(tooltip::plain(self.path))
                .into_any_element(),
            None => line.into_any_element(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::split;

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
}

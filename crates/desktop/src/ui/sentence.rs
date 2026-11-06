//! Running text whose queue names stand out in the accent, so a name someone made up reads as a
//! name and not as a word of the sentence (D240).

use std::ops::Range;

use gpui_kit::component::ActiveTheme as _;
use gpui_kit::{App, FontWeight, HighlightStyle, IntoElement, RenderOnce, StyledText, Window};

#[derive(IntoElement, Clone, Debug, Default, PartialEq, Eq)]
pub struct Sentence {
    text: String,
    names: Vec<Range<usize>>,
}

impl Sentence {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn text(mut self, said: &str) -> Self {
        self.text.push_str(said);
        self
    }

    pub fn queue(mut self, name: &str) -> Self {
        let start = self.text.len();
        self.text.push_str(name);
        self.names.push(start..self.text.len());
        self
    }

    /// `names` joined by commas, each one standing out.
    pub fn queues<'a>(mut self, names: impl IntoIterator<Item = &'a str>) -> Self {
        for (index, name) in names.into_iter().enumerate() {
            if index > 0 {
                self = self.text(", ");
            }
            self = self.queue(name);
        }
        self
    }
}

impl RenderOnce for Sentence {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let name = HighlightStyle {
            color: Some(cx.theme().primary),
            font_weight: Some(FontWeight::MEDIUM),
            ..HighlightStyle::default()
        };
        StyledText::new(self.text).with_highlights(self.names.into_iter().map(|at| (at, name)))
    }
}

#[cfg(test)]
mod tests {
    use super::Sentence;

    #[test]
    fn each_queue_name_is_marked_where_it_stands() {
        let said = Sentence::new()
            .text("They go to 2 queues: ")
            .queues(["Main", "Vidéos"])
            .text(".");
        assert_eq!(said.text, "They go to 2 queues: Main, Vidéos.");
        let marked: Vec<&str> = said.names.iter().map(|at| &said.text[at.clone()]).collect();
        assert_eq!(marked, ["Main", "Vidéos"]);
    }
}

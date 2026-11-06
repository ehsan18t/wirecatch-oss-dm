//! A file name field whose text starts as the name the app proposes, so a person edits it rather
//! than retyping it, and which tells a name they chose from one they left alone (D215).

use gpui_kit::component::input::InputState;
use gpui_kit::{AnyWindowHandle, App, AppContext as _, Entity, Window};

pub struct ProposedName {
    field: Entity<InputState>,
    /// The name the app last proposed.
    proposed: Option<String>,
}

impl ProposedName {
    /// An empty field, for a name that arrives through [`Self::propose`].
    pub fn empty(window: &mut Window, cx: &mut App) -> Self {
        Self {
            field: cx.new(|cx| InputState::new(window, cx)),
            proposed: None,
        }
    }

    /// A field holding `name`; emptied, it shows `name` as the hint.
    pub fn holding(name: String, window: &mut Window, cx: &mut App) -> Self {
        let proposed = Some(name.clone());
        let field = cx.new(|cx| {
            let mut state = InputState::new(window, cx).placeholder(name.clone());
            state.set_value(name, window, cx);
            state
        });
        Self { field, proposed }
    }

    pub fn field(&self) -> &Entity<InputState> {
        &self.field
    }

    /// The app has a name: a field the person has not changed takes it, and an emptied one shows it
    /// as the hint. `window` is the field's own: a question's window can open behind another, so the
    /// active one may be neither.
    pub fn propose(&mut self, name: &str, window: AnyWindowHandle, cx: &mut App) {
        let takes_it = {
            let field = self.field.read(cx).value();
            core_name_applies(&field, self.proposed.as_deref(), name)
        };
        self.proposed = Some(name.to_owned());
        let (state, name) = (self.field.clone(), name.to_owned());
        cx.defer(move |cx| {
            crate::closing::may_have_closed(window.update(cx, |_, window, cx| {
                state.update(cx, |state, cx| {
                    state.set_placeholder(name.clone(), window, cx);
                    if takes_it {
                        state.set_value(name, window, cx);
                    }
                });
            }));
        });
    }

    /// What the person chose, if anything.
    pub fn chosen(&self, cx: &App) -> Option<String> {
        chosen_name(&self.field.read(cx).value(), self.proposed.as_deref())
    }
}

/// A name left as proposed, or cleared, is no choice, so the server's answer may still improve it.
fn chosen_name(field: &str, proposed: Option<&str>) -> Option<String> {
    let name = field.trim();
    (!name.is_empty() && Some(name) != proposed).then(|| name.to_owned())
}

/// Whether the core's name replaces what the field holds: never a name the person typed, nor a field
/// they cleared, which may be about to get one.
fn core_name_applies(field: &str, last: Option<&str>, core: &str) -> bool {
    last.is_none_or(|last| last != core && field.trim() == last)
}

#[cfg(test)]
mod tests {
    use gpui_kit::{Context, IntoElement, Render, TestAppContext, VisualTestContext, div};

    use super::*;

    #[test]
    fn only_a_name_the_person_changed_counts_as_chosen() {
        assert_eq!(chosen_name("get", Some("get")), None);
        assert_eq!(chosen_name(" get ", Some("get")), None);
        assert_eq!(chosen_name("", Some("get")), None);
        assert_eq!(
            chosen_name("report.pdf", Some("get")).as_deref(),
            Some("report.pdf")
        );
        assert_eq!(chosen_name("get", None).as_deref(), Some("get"));
    }

    #[test]
    fn a_name_the_person_typed_outlasts_the_question_being_read_again() {
        assert!(core_name_applies("", None, "report.pdf"));
        assert!(!core_name_applies(
            "my name.pdf",
            Some("report.pdf"),
            "report.pdf"
        ));
        assert!(!core_name_applies(
            "report.pdf",
            Some("report.pdf"),
            "report.pdf"
        ));
        // The core found a better name, and the person has not changed the field.
        assert!(core_name_applies(
            "report.pdf",
            Some("report.pdf"),
            "final.pdf"
        ));
        assert!(core_name_applies(
            " report.pdf ",
            Some("report.pdf"),
            "final.pdf"
        ));
        assert!(!core_name_applies(
            "my name.pdf",
            Some("report.pdf"),
            "final.pdf"
        ));
        assert!(!core_name_applies("", Some("report.pdf"), "final.pdf"));
    }

    struct Name(ProposedName);

    impl Render for Name {
        fn render(&mut self, _: &mut Window, _: &mut Context<'_, Self>) -> impl IntoElement {
            div()
        }
    }

    fn propose(name: &Entity<Name>, proposed: &str, cx: &mut VisualTestContext) {
        cx.update(|window, cx| {
            let window = window.window_handle();
            name.update(cx, |name, cx| name.0.propose(proposed, window, cx));
        });
        cx.run_until_parked();
    }

    fn type_in(name: &Entity<Name>, text: &str, cx: &mut VisualTestContext) {
        let field = name.read_with(cx, |name, _| name.0.field().clone());
        let text = text.to_owned();
        cx.update(|window, cx| {
            field.update(cx, |state, cx| state.set_value(text, window, cx));
        });
    }

    fn text(name: &Entity<Name>, cx: &mut VisualTestContext) -> String {
        name.read_with(cx, |name, cx| {
            name.0.field().read(cx).value().as_ref().to_owned()
        })
    }

    fn chosen(name: &Entity<Name>, cx: &mut VisualTestContext) -> Option<String> {
        name.read_with(cx, |name, cx| name.0.chosen(cx))
    }

    fn open(cx: &mut TestAppContext) -> (Entity<Name>, &mut VisualTestContext) {
        cx.update(gpui_kit::component::init);
        cx.add_window_view(|window, cx| Name(ProposedName::empty(window, cx)))
    }

    #[gpui_kit::test]
    fn a_window_behind_the_others_still_gets_its_name(cx: &mut TestAppContext) {
        let (name, cx) = open(cx);
        cx.deactivate_window();
        propose(&name, "report.pdf", cx);
        assert_eq!(text(&name, cx), "report.pdf");
        assert_eq!(chosen(&name, cx), None);
    }

    #[gpui_kit::test]
    fn a_name_left_alone_follows_the_core(cx: &mut TestAppContext) {
        let (name, cx) = open(cx);
        propose(&name, "get", cx);
        propose(&name, "report.pdf", cx);
        assert_eq!(text(&name, cx), "report.pdf");
        assert_eq!(chosen(&name, cx), None);
    }

    #[gpui_kit::test]
    fn an_edited_name_stays_when_the_core_finds_another(cx: &mut TestAppContext) {
        let (name, cx) = open(cx);
        propose(&name, "get", cx);
        type_in(&name, "mine.pdf", cx);
        propose(&name, "report.pdf", cx);
        assert_eq!(text(&name, cx), "mine.pdf");
        assert_eq!(chosen(&name, cx).as_deref(), Some("mine.pdf"));
        // Going back to the name proposed before is now a choice, and the newest is not.
        type_in(&name, "get", cx);
        assert_eq!(chosen(&name, cx).as_deref(), Some("get"));
        type_in(&name, "report.pdf", cx);
        assert_eq!(chosen(&name, cx), None);
    }

    #[gpui_kit::test]
    fn a_cleared_field_stays_empty_and_means_the_newest_proposal(cx: &mut TestAppContext) {
        let (name, cx) = open(cx);
        propose(&name, "get", cx);
        type_in(&name, "", cx);
        propose(&name, "report.pdf", cx);
        assert_eq!(text(&name, cx), "");
        assert_eq!(chosen(&name, cx), None);
    }
}

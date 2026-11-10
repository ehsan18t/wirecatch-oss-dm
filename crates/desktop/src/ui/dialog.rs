//! Every dialog of the app opens through here, so all of them move alike and head alike (D260).

use std::rc::Rc;

use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants as _};
use gpui_kit::component::dialog::{Dialog, DialogEntrance};
use gpui_kit::component::input::InputState;
use gpui_kit::component::{WindowExt as _, h_flex, v_flex, window_paddings};
use gpui_kit::{
    AnyElement, App, AppContext as _, Bounds, Entity, InteractiveElement as _, IntoElement,
    ParentElement as _, Pixels, RenderOnce, SharedString, Styled as _, Window, div, point, px,
};

use crate::ui::layout::{self, Rule};
use crate::ui::{PathText, Role, Typed as _};

/// The default slides down from the window top for 250 ms; this settles in place.
const ENTRANCE: DialogEntrance = DialogEntrance::FadeSlide;
/// gpui-kit's padding on each side of a dialog, which the header's rule reaches past.
const DIALOG_PADDING: f32 = 16.;
const QUESTION_WIDTH: f32 = 440.;

/// A dialog's title over a rule across the whole dialog, with a fixed gap before its content.
#[derive(IntoElement)]
pub struct Header {
    title: SharedString,
    description: Option<AnyElement>,
}

impl Header {
    pub fn new(title: impl Into<SharedString>) -> Self {
        Self {
            title: title.into(),
            description: None,
        }
    }

    pub fn description(mut self, description: impl IntoElement) -> Self {
        self.description = Some(description.into_any_element());
        self
    }
}

impl RenderOnce for Header {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let role = if self.description.is_some() {
            Role::Title
        } else {
            Role::PageTitle
        };
        layout::section(Rule::Below, cx)
            .mx(px(-DIALOG_PADDING))
            .pt_0()
            .mb_2()
            .child(
                v_flex()
                    .pr_8()
                    .min_w_0()
                    .gap_1()
                    .child(
                        div()
                            .min_w_0()
                            .debug_selector(|| "dialog-title".to_owned())
                            .text_role(role, cx)
                            .child(PathText::new(self.title).tooltip("dialog-title-tooltip")),
                    )
                    .children(self.description),
            )
    }
}

/// Opens a dialog headed by what `title` says, read at each draw.
pub fn open(
    window: &mut Window,
    cx: &mut App,
    title: impl Fn(&App) -> SharedString + 'static,
    build: impl Fn(Dialog, &mut Window, &mut App) -> Dialog + 'static,
) {
    window.open_dialog(cx, move |dialog, window, cx| {
        let header = Header::new(title(cx));
        build(dialog.entrance(ENTRANCE).title(header), window, cx)
    });
}

/// Like [`open`], but beside `row` when it is known, so a question never covers the row it asks
/// about (D261).
pub fn open_beside(
    row: Option<Bounds<Pixels>>,
    window: &mut Window,
    cx: &mut App,
    title: impl Fn(&App) -> SharedString + 'static,
    build: impl Fn(Dialog, &mut Window, &mut App) -> Dialog + 'static,
) {
    // gpui-kit places a dialog inside the window's client-drawn frame on Linux, so its top counts
    // from below that inset, and only the room inside it is the dialog's.
    let inset = window_paddings(window);
    let viewport = window.viewport_size();
    let area = Bounds::from_corners(
        point(inset.left, inset.top),
        point(viewport.width - inset.right, viewport.height - inset.bottom),
    );
    let top = row.and_then(|row| top_beside(row, area));
    open(window, cx, title, move |dialog, window, cx| {
        let dialog = build(dialog, window, cx);
        match top {
            Some(top) => dialog.margin_top(top),
            None => dialog,
        }
    });
}

/// The room a question about downloads is given, so one placed above its row ends before it:
/// the tallest, Remove with a folded note, a partial file and the trash box, with a line spare.
const QUESTION_HEIGHT: f32 = 320.;
/// Between a row and the question beside it.
const ROW_GAP: f32 = 8.;

/// Where a question's top goes, counted from the top of `area`, the room dialogs take: under
/// `row` when it fits there, else above it; `None` leaves the usual place, when neither fits.
fn top_beside(row: Bounds<Pixels>, area: Bounds<Pixels>) -> Option<Pixels> {
    let below = row.bottom() + px(ROW_GAP);
    if below + px(QUESTION_HEIGHT) <= area.bottom() {
        return Some(below - area.top());
    }
    let above = row.top() - px(ROW_GAP + QUESTION_HEIGHT);
    (above >= area.top()).then(|| above - area.top())
}

/// Opens a dialog with no header, for one that is a single field, such as the command palette.
pub fn open_headless(
    window: &mut Window,
    cx: &mut App,
    build: impl Fn(Dialog, &mut Window, &mut App) -> Dialog + 'static,
) {
    window.open_dialog(cx, move |dialog, window, cx| {
        build(dialog.entrance(ENTRANCE), window, cx)
    });
}

/// Enter gives the main answer, `main` styled as `variant`, unless it is a danger one: then Enter
/// leaves the question open (D260.3).
pub fn enter_answers(
    dialog: Dialog,
    variant: ButtonVariant,
    main: impl Fn(&mut Window, &mut App) + 'static,
) -> Dialog {
    dialog.on_ok(move |_, window, cx| {
        if variant != ButtonVariant::Danger {
            main(window, cx);
        }
        false
    })
}

/// A question whose answer cannot be taken back: what it is about, and its two answers.
pub struct Question {
    pub title: String,
    pub body: String,
    pub keep: &'static str,
    pub go: &'static str,
}

/// Asks `question`, its body under the header as the other questions have theirs; `go` runs
/// once the person picks that answer.
pub fn ask(
    question: Question,
    window: &mut Window,
    cx: &mut App,
    go: impl Fn(&mut Window, &mut App) + 'static,
) {
    let Question {
        title,
        body,
        keep,
        go: go_label,
    } = question;
    let title = SharedString::from(title);
    let body = SharedString::from(body);
    let go = Rc::new(go);
    open(
        window,
        cx,
        move |_| title.clone(),
        move |dialog, _, cx| {
            let go = go.clone();
            enter_answers(dialog, ButtonVariant::Danger, |_, _| {})
                .w(px(QUESTION_WIDTH))
                .child(div().text_role(Role::Secondary, cx).child(body.clone()))
                .footer(
                    h_flex()
                        .w_full()
                        .gap_2()
                        .justify_end()
                        .child(
                            Button::new("keep")
                                .label(keep)
                                .on_click(|_, window, cx| window.close_dialog(cx)),
                        )
                        .child(Button::new("go").danger().label(go_label).on_click(
                            move |_, window, cx| {
                                window.close_dialog(cx);
                                go(window, cx);
                            },
                        )),
                )
        },
    );
}

/// Gives the keyboard to `field` once the dialog just opened has taken it for itself.
pub fn focus_after_opening<F: gpui_kit::Focusable + 'static>(
    field: &gpui_kit::Entity<F>,
    window: &mut Window,
    cx: &mut App,
) {
    let field = field.clone();
    window.defer(cx, move |window, cx| {
        field.read(cx).focus_handle(cx).focus(window, cx);
    });
}

/// A form's one-line field, holding `value`.
pub fn text_field(
    value: impl Into<String>,
    placeholder: &'static str,
    window: &mut Window,
    cx: &mut App,
) -> Entity<InputState> {
    let value = value.into();
    cx.new(|cx| {
        let mut state = InputState::new(window, cx).placeholder(placeholder);
        state.set_value(value, window, cx);
        state
    })
}

#[cfg(test)]
mod tests {
    use gpui_kit::{Context, Render, TestAppContext, size};

    use super::*;

    #[gpui_kit::test]
    fn a_long_title_leaves_room_for_the_close_button(cx: &mut TestAppContext) {
        struct Heading;
        impl Render for Heading {
            fn render(&mut self, _: &mut Window, _: &mut Context<'_, Self>) -> impl IntoElement {
                div().w(px(360.)).p_4().child(Header::new(
                    "a-very-long-file-name-without-spaces-that-must-not-cover-close.zip",
                ))
            }
        }
        cx.update(gpui_kit::component::init);
        let (_, cx) = cx.add_window_view(|_, _| Heading);
        cx.run_until_parked();
        let title = cx.debug_bounds("dialog-title").expect("the title is drawn");
        assert!(
            title.right() <= px(312.),
            "the title occupies the close button's space: {title:?}"
        );
    }

    fn row_at(top: f32) -> Bounds<Pixels> {
        Bounds::new(point(px(0.), px(top)), size(px(900.), px(40.)))
    }

    fn window(height: f32, inset: f32) -> Bounds<Pixels> {
        Bounds::from_corners(
            point(px(inset), px(inset)),
            point(px(1000.), px(height - inset)),
        )
    }

    #[test]
    fn a_question_opens_under_its_row_when_it_fits() {
        assert_eq!(top_beside(row_at(100.), window(800., 0.)), Some(px(148.)));
    }

    #[test]
    fn a_question_opens_above_a_row_low_in_the_window() {
        assert_eq!(top_beside(row_at(700.), window(800., 0.)), Some(px(372.)));
    }

    #[test]
    fn a_window_too_short_for_either_keeps_the_usual_place() {
        assert_eq!(top_beside(row_at(200.), window(400., 0.)), None);
    }

    #[test]
    fn a_client_drawn_frame_is_left_out_of_the_room() {
        assert_eq!(top_beside(row_at(100.), window(800., 20.)), Some(px(128.)));
        assert_eq!(top_beside(row_at(440.), window(800., 20.)), Some(px(92.)));
    }
}

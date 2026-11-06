//! Every dialog of the app opens through here, so all of them move alike and head alike (D260).

use std::rc::Rc;

use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants as _};
use gpui_kit::component::dialog::{Dialog, DialogEntrance};
use gpui_kit::component::input::InputState;
use gpui_kit::component::{ActiveTheme as _, WindowExt as _, h_flex, v_flex};
use gpui_kit::{
    App, AppContext as _, Entity, IntoElement, ParentElement as _, RenderOnce, SharedString,
    Styled as _, Window, div, px,
};

use crate::ui::{Role, Typed as _};

/// The default slides down from the window top for 250 ms; this settles in place.
const ENTRANCE: DialogEntrance = DialogEntrance::FadeSlide;
/// gpui-kit's padding on each side of a dialog, which the header's rule reaches past.
const DIALOG_PADDING: f32 = 16.;
const QUESTION_WIDTH: f32 = 440.;

/// A dialog's title over a rule across the whole dialog, with a fixed gap before its content.
#[derive(IntoElement)]
struct Header {
    title: SharedString,
}

impl RenderOnce for Header {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        v_flex()
            .mx(px(-DIALOG_PADDING))
            .px(px(DIALOG_PADDING))
            .pb_3()
            .mb_2()
            .border_b_1()
            .border_color(cx.theme().border)
            .child(div().text_role(Role::PageTitle, cx).child(self.title))
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
        let header = Header { title: title(cx) };
        build(dialog.entrance(ENTRANCE).title(header), window, cx)
    });
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

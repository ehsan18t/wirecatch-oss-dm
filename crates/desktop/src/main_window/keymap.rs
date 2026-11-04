//! Every keyboard shortcut of the main window, in one table: it binds the keys, and the Keyboard
//! shortcuts page lists them, so a shortcut changes in one place.

use std::rc::Rc;

use gpui_kit::component::setting::{SettingGroup, SettingItem, SettingPage};
use gpui_kit::component::{ActiveTheme as _, h_flex};
use gpui_kit::{
    Action, App, KeyBinding, KeyBindingContextPredicate, ParentElement as _, Styled as _, div, px,
};

use super::{
    CursorDown, CursorFirst, CursorLast, CursorPageDown, CursorPageUp, CursorUp, Dismiss,
    ExtendDown, ExtendUp, FocusSearch, KEY_CONTEXT, NewDownload, NextRegion, NudgeDown, NudgeUp,
    OpenMenu, OpenPalette, OpenSelected, PasteLinks, PauseOrResume, PreviousRegion, RemoveSelected,
    SearchToList, SelectAll, ShowDetails, ShowShortcuts, ToggleSettings,
};
use crate::ui::{Role, Typed as _};

/// One shortcut: its keys as GPUI writes them, what it does, where it works, and what the
/// shortcuts page says (keys with the same words share a line; `None` is left out).
struct Shortcut {
    keys: &'static str,
    action: fn() -> Box<dyn Action>,
    context: &'static str,
    help: Option<&'static str>,
}

const MOVE: &str = "Move through the list";
const EXTEND: &str = "Select several downloads";
const JUMP: &str = "Jump through the list";
const NUDGE: &str = "Move the selection up or down in its queue";
const MENU: &str = "Open the menu of the selected downloads";
const REGION: &str = "Move between the search and the list";

/// Every shortcut, in the order the page lists them.
fn shortcuts() -> [Shortcut; 26] {
    let anywhere = |keys, action: fn() -> Box<dyn Action>, help| Shortcut {
        keys,
        action,
        context: KEY_CONTEXT,
        help,
    };
    let on = |keys, action: fn() -> Box<dyn Action>, help| Shortcut {
        keys,
        action,
        context: OUTSIDE_FIELDS,
        help,
    };
    [
        anywhere(
            "ctrl-shift-p",
            || Box::new(OpenPalette),
            Some("Find any action or setting"),
        ),
        on(
            "ctrl-v",
            || Box::new(PasteLinks),
            Some("Download the link you copied"),
        ),
        anywhere("ctrl-n", || Box::new(NewDownload), Some("New download")),
        anywhere("ctrl-k", || Box::new(FocusSearch), Some("Search the list")),
        anywhere("ctrl-,", || Box::new(ToggleSettings), Some("Open settings")),
        on("up", || Box::new(CursorUp), Some(MOVE)),
        on("down", || Box::new(CursorDown), Some(MOVE)),
        on("shift-up", || Box::new(ExtendUp), Some(EXTEND)),
        on("shift-down", || Box::new(ExtendDown), Some(EXTEND)),
        on("pageup", || Box::new(CursorPageUp), Some(JUMP)),
        on("pagedown", || Box::new(CursorPageDown), Some(JUMP)),
        on("home", || Box::new(CursorFirst), Some(JUMP)),
        on("end", || Box::new(CursorLast), Some(JUMP)),
        on(
            "ctrl-a",
            || Box::new(SelectAll),
            Some("Select every download in the list"),
        ),
        on(
            "space",
            || Box::new(PauseOrResume),
            Some("Pause or resume the selected downloads"),
        ),
        on(
            "enter",
            || Box::new(OpenSelected),
            Some("Open the selected file when it is finished"),
        ),
        on(
            "alt-enter",
            || Box::new(ShowDetails),
            Some("Show the details of the selected download"),
        ),
        on(
            "delete",
            || Box::new(RemoveSelected),
            Some("Remove the selected downloads"),
        ),
        on("alt-up", || Box::new(NudgeUp), Some(NUDGE)),
        on("alt-down", || Box::new(NudgeDown), Some(NUDGE)),
        on("menu", || Box::new(OpenMenu), Some(MENU)),
        on("shift-f10", || Box::new(OpenMenu), Some(MENU)),
        anywhere(
            "escape",
            || Box::new(Dismiss),
            Some("Close a dialog, go back to the downloads, or clear the selection"),
        ),
        anywhere(
            "f1",
            || Box::new(ShowShortcuts),
            Some("Show keyboard shortcuts"),
        ),
        anywhere("f6", || Box::new(NextRegion), Some(REGION)),
        anywhere("shift-f6", || Box::new(PreviousRegion), Some(REGION)),
    ]
}

/// The shortcut the page does not list: Down leaving the search, where the field's own Down
/// would otherwise win.
fn quiet() -> [Shortcut; 1] {
    [Shortcut {
        keys: "down",
        action: || Box::new(SearchToList),
        context: SEARCH_INPUT,
        help: None,
    }]
}

/// The list's keys, in the window but not while a text field has the keyboard: a field passes on
/// keys it does not use, such as Enter. GPUI's `!` reads the whole context path, not one level.
const OUTSIDE_FIELDS: &str = "MainWindow && !Input";

/// The search field's input, inside the search's own context.
const SEARCH_INPUT: &str = "MainSearch > Input";

/// Binds every shortcut. Call once at start, after the components bind theirs.
pub fn bind(cx: &mut App) {
    let bindings: Vec<KeyBinding> = shortcuts()
        .into_iter()
        .chain(quiet())
        .filter_map(|shortcut| {
            let context = KeyBindingContextPredicate::parse(shortcut.context)
                .ok()
                .map(Rc::new);
            KeyBinding::load(shortcut.keys, (shortcut.action)(), context, false, None, cx)
                .inspect_err(|e| tracing::error!("a shortcut could not be bound: {e}"))
                .ok()
        })
        .collect();
    cx.bind_keys(bindings);
}

/// Keys as people write them: `Ctrl+Shift+P`.
fn label(keys: &str) -> String {
    keys.split('-')
        .map(|part| match part {
            "ctrl" => "Ctrl".to_owned(),
            "shift" => "Shift".to_owned(),
            "alt" => "Alt".to_owned(),
            "pageup" => "Page Up".to_owned(),
            "pagedown" => "Page Down".to_owned(),
            "escape" => "Esc".to_owned(),
            other => {
                let mut chars = other.chars();
                chars.next().map_or_else(String::new, |first| {
                    first.to_uppercase().chain(chars).collect()
                })
            }
        })
        .collect::<Vec<_>>()
        .join("+")
}

/// What the page lists: each line's words with every key that does it.
fn lines() -> Vec<(&'static str, Vec<String>)> {
    let mut lines: Vec<(&'static str, Vec<String>)> = Vec::new();
    for shortcut in shortcuts() {
        let Some(help) = shortcut.help else {
            continue;
        };
        let keys = label(shortcut.keys);
        match lines.iter_mut().find(|(said, _)| *said == help) {
            Some((_, all)) => all.push(keys),
            None => lines.push((help, vec![keys])),
        }
    }
    lines
}

/// The Keyboard shortcuts page.
pub(super) fn page() -> SettingPage {
    let rows = lines().into_iter().map(|(help, keys)| {
        let search = keys.join(" ");
        SettingItem::render(move |_, _, cx| {
            h_flex()
                .w_full()
                .gap_4()
                .child(div().flex_1().text_role(Role::Body, cx).child(help))
                .child(h_flex().gap_1().children(keys.iter().map(|key| {
                    div()
                        .px_1p5()
                        .rounded(px(4.))
                        .bg(cx.theme().muted)
                        .text_role(Role::Meta, cx)
                        .child(key.clone())
                })))
        })
        .keywords([help.to_owned(), search])
    });
    SettingPage::new("Keyboard shortcuts")
        .resettable(false)
        .group(SettingGroup::new().items(rows))
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::component::input::{Input, InputState};
    use gpui_kit::component::v_flex;
    use gpui_kit::{
        AppContext as _, Context, Entity, FocusHandle, InteractiveElement as _, IntoElement,
        Render, TestAppContext, Window,
    };

    /// The tree `MainWindow::render` and `command_bar` build around the search field.
    struct SearchInWindow {
        search: Entity<InputState>,
        focus: FocusHandle,
        pauses: usize,
        opens: usize,
        helps: usize,
    }

    impl Render for SearchInWindow {
        fn render(&mut self, _: &mut Window, cx: &mut Context<'_, Self>) -> impl IntoElement {
            v_flex()
                .id("main-window")
                .key_context(KEY_CONTEXT)
                .track_focus(&self.focus)
                .on_action(cx.listener(|this, _: &PauseOrResume, _, _| this.pauses += 1))
                .on_action(cx.listener(|this, _: &OpenSelected, _, _| this.opens += 1))
                .on_action(cx.listener(|this, _: &ShowShortcuts, _, _| this.helps += 1))
                .child(
                    h_flex()
                        .key_context(super::super::SEARCH_CONTEXT)
                        .child(Input::new(&self.search)),
                )
        }
    }

    /// What the keys did: the search field's text, then Space's, Enter's and F1's commands.
    fn press(
        bound: bool,
        in_field: bool,
        keys: &str,
        cx: &mut TestAppContext,
    ) -> (String, usize, usize, usize) {
        cx.update(|cx| {
            gpui_kit::component::init(cx);
            if bound {
                bind(cx);
            }
        });
        let (root, cx) = cx.add_window_view(|window, cx| SearchInWindow {
            search: cx.new(|cx| InputState::new(window, cx)),
            focus: cx.focus_handle(),
            pauses: 0,
            opens: 0,
            helps: 0,
        });
        cx.run_until_parked();
        let (search, focus) =
            root.read_with(cx, |root, _| (root.search.clone(), root.focus.clone()));
        let focusing = search.clone();
        cx.update(|window, cx| {
            if in_field {
                focusing.update(cx, |search, cx| search.focus(window, cx));
            } else {
                focus.focus(window, cx);
            }
        });
        cx.run_until_parked();
        cx.simulate_keystrokes(keys);
        let value = search.read_with(cx, |search, _| search.value().to_string());
        root.read_with(cx, |root, _| (value, root.pauses, root.opens, root.helps))
    }

    #[gpui_kit::test]
    fn without_the_window_keys_a_space_reaches_the_search_field(cx: &mut TestAppContext) {
        assert_eq!(
            press(false, true, "a space b", cx),
            ("a b".to_owned(), 0, 0, 0)
        );
    }

    #[gpui_kit::test]
    fn a_space_typed_in_the_search_field_is_text_and_not_a_command(cx: &mut TestAppContext) {
        assert_eq!(
            press(true, true, "a space b", cx),
            ("a b".to_owned(), 0, 0, 0)
        );
    }

    #[gpui_kit::test]
    fn enter_in_the_search_field_does_not_open_the_selected_file(cx: &mut TestAppContext) {
        assert_eq!(press(true, true, "a enter", cx), ("a".to_owned(), 0, 0, 0));
    }

    #[gpui_kit::test]
    fn the_list_keys_still_work_outside_a_field(cx: &mut TestAppContext) {
        assert_eq!(
            press(true, false, "space enter", cx),
            (String::new(), 1, 1, 0)
        );
    }

    #[gpui_kit::test]
    fn a_window_key_still_works_from_a_field(cx: &mut TestAppContext) {
        assert_eq!(press(true, true, "f1", cx), (String::new(), 0, 0, 1));
    }

    #[test]
    fn down_leaves_the_search_field_of_the_main_search() {
        assert_eq!(
            SEARCH_INPUT,
            format!("{} > Input", super::super::SEARCH_CONTEXT)
        );
    }

    #[test]
    fn keys_read_as_people_write_them() {
        assert_eq!(label("ctrl-shift-p"), "Ctrl+Shift+P");
        assert_eq!(label("pagedown"), "Page Down");
        assert_eq!(label("ctrl-,"), "Ctrl+,");
        assert_eq!(label("shift-f10"), "Shift+F10");
    }

    #[test]
    fn keys_with_the_same_words_share_a_line() {
        let lines = lines();
        let moving = lines.iter().find(|(help, _)| *help == MOVE).unwrap();
        assert_eq!(moving.1, ["Up", "Down"]);
        let mut said: Vec<&str> = lines.iter().map(|(help, _)| *help).collect();
        said.dedup();
        assert_eq!(said.len(), lines.len(), "each line once");
    }
}

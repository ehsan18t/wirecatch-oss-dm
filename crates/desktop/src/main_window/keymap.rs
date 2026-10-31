//! Every keyboard shortcut of the main window, in one table: it binds the keys, and the Keyboard
//! shortcuts dialog lists them, so a shortcut changes in one place.

use std::rc::Rc;

use gpui_kit::component::{ActiveTheme as _, h_flex, v_flex};
use gpui_kit::{
    Action, App, KeyBinding, KeyBindingContextPredicate, ParentElement as _, Styled as _, Window,
    div, px,
};

use super::{
    CursorDown, CursorFirst, CursorLast, CursorPageDown, CursorPageUp, CursorUp, Dismiss,
    ExtendDown, ExtendUp, FocusSearch, KEY_CONTEXT, NewDownload, NextRegion, NudgeDown, NudgeUp,
    OpenMenu, OpenPalette, OpenSelected, PasteLinks, PauseOrResume, PreviousRegion, RemoveSelected,
    SearchToList, SelectAll, ShowDetails, ShowShortcuts, ToggleSettings,
};
use crate::ui::{Role, Typed as _};

const WIDTH: f32 = 520.;

/// One shortcut: its keys as GPUI writes them, what it does, where it works, and what the
/// shortcuts dialog says (keys with the same words share a line; `None` is left out).
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

/// Every shortcut, in the order the dialog lists them.
fn shortcuts() -> [Shortcut; 26] {
    let on = |keys, action: fn() -> Box<dyn Action>, help| Shortcut {
        keys,
        action,
        context: KEY_CONTEXT,
        help,
    };
    [
        on(
            "ctrl-shift-p",
            || Box::new(OpenPalette),
            Some("Find any action or setting"),
        ),
        on(
            "ctrl-v",
            || Box::new(PasteLinks),
            Some("Download the link you copied"),
        ),
        on("ctrl-n", || Box::new(NewDownload), Some("New download")),
        on("ctrl-k", || Box::new(FocusSearch), Some("Search the list")),
        on("ctrl-,", || Box::new(ToggleSettings), Some("Open settings")),
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
        on(
            "escape",
            || Box::new(Dismiss),
            Some("Close a dialog, or clear the selection"),
        ),
        on(
            "f1",
            || Box::new(ShowShortcuts),
            Some("Show these shortcuts"),
        ),
        on("f6", || Box::new(NextRegion), Some(REGION)),
        on("shift-f6", || Box::new(PreviousRegion), Some(REGION)),
    ]
}

/// The shortcut the dialog does not list: Down leaving the search, where the field's own Down
/// would otherwise win.
fn quiet() -> [Shortcut; 1] {
    [Shortcut {
        keys: "down",
        action: || Box::new(SearchToList),
        context: SEARCH_INPUT,
        help: None,
    }]
}

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

/// What the dialog lists: each line's words with every key that does it.
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

/// The Keyboard shortcuts dialog.
pub fn open(window: &mut Window, cx: &mut App) {
    crate::ui::dialog::open(window, cx, |dialog, _, cx| {
        let rows = lines().into_iter().map(|(help, keys)| {
            h_flex()
                .gap_4()
                .py_1()
                .child(div().flex_1().text_role(Role::Body, cx).child(help))
                .child(h_flex().gap_1().children(keys.into_iter().map(|key| {
                    div()
                        .px_1p5()
                        .rounded(px(4.))
                        .bg(cx.theme().muted)
                        .text_role(Role::Meta, cx)
                        .child(key)
                })))
        });
        dialog
            .title("Keyboard shortcuts")
            .w(px(WIDTH))
            .child(v_flex().children(rows))
    });
}

#[cfg(test)]
mod tests {
    use super::*;

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

//! Leaving the queue page (D255): every way off it comes through here, so changes not saved are
//! never dropped without asking.

use std::cell::Cell;
use std::rc::Rc;

use gpui_kit::component::button::ButtonVariant;
use gpui_kit::{App, Context, Window};

use super::{MainWindow, Screen};
use crate::closing::may_have_closed;

type Then = Box<dyn FnOnce(&mut MainWindow, &mut Window, &mut Context<'_, MainWindow>)>;

impl MainWindow {
    /// Runs `then` once the queue page may be left: at once, unless the page holds changes, which
    /// the person is first asked to discard. Off the page, `then` runs at once.
    pub(super) fn leave_queue_page_then(
        &mut self,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
        then: impl FnOnce(&mut Self, &mut Window, &mut Context<'_, Self>) + 'static,
    ) {
        let changed = self.page.screen == Screen::QueuePage
            && self
                .queue_page
                .as_ref()
                .is_some_and(|form| form.read(cx).has_changes(cx));
        if !changed {
            return then(self, window, cx);
        }
        let then: Rc<Cell<Option<Then>>> = Rc::new(Cell::new(Some(Box::new(then))));
        let main = cx.entity().downgrade();
        crate::ui::dialog::open_alert(window, cx, move |alert, _, _| {
            let (then, main) = (then.clone(), main.clone());
            alert
                .title("Discard changes?")
                .description("The changes to this queue are not saved.")
                .confirm()
                .ok_text("Discard")
                .ok_variant(ButtonVariant::Danger)
                .cancel_text("Keep editing")
                .on_ok(move |_, window, cx| {
                    discard(&main, then.take(), window, cx);
                    true
                })
        });
    }
}

fn discard(
    main: &gpui_kit::WeakEntity<MainWindow>,
    then: Option<Then>,
    window: &mut Window,
    cx: &mut App,
) {
    let Some(then) = then else {
        return;
    };
    may_have_closed(main.update(cx, |main, cx| {
        main.queue_page = None;
        then(main, window, cx);
    }));
}

//! A window's step before it closes, run however it closes: by the system, or by the title bar's
//! Close button, which on Linux takes the window away itself and so skips
//! `on_window_should_close`. Also the one place an update of a window or view that may have
//! closed meanwhile is let go.

use std::collections::HashMap;
use std::panic::Location;
use std::rc::Rc;

use gpui_kit::{App, Global, Subscription, Window, WindowId};

type Step = Rc<dyn Fn(&mut Window, &mut App) -> bool>;

/// Each open window's step; a window that closed takes its step with it.
struct Steps {
    steps: HashMap<WindowId, Step>,
    _closed: Subscription,
}

impl Global for Steps {}

/// Runs `step` before `window` closes; the window stays open when it answers false.
pub fn on_close(
    window: &Window,
    cx: &mut App,
    step: impl Fn(&mut Window, &mut App) -> bool + 'static,
) {
    let step: Step = Rc::new(step);
    if !cx.has_global::<Steps>() {
        let closed = cx.on_window_closed(|cx, id| {
            cx.global_mut::<Steps>().steps.remove(&id);
        });
        cx.set_global(Steps {
            steps: HashMap::new(),
            _closed: closed,
        });
    }
    cx.global_mut::<Steps>()
        .steps
        .insert(window.window_handle().window_id(), Rc::clone(&step));
    window.on_window_should_close(cx, move |window, cx| step(window, cx));
}

/// What a title bar's Close button does: the window's step, then the window goes unless the step
/// keeps it.
pub fn close(window: &mut Window, cx: &mut App) {
    let step = cx
        .try_global::<Steps>()
        .and_then(|steps| steps.steps.get(&window.window_handle().window_id()))
        .cloned();
    if step.is_none_or(|step| step(window, cx)) {
        window.remove_window();
    }
}

/// Lets go of an update whose window or view may have closed while work for it ran: the work's
/// result has nowhere to go, so it is logged with the caller's place and dropped.
#[track_caller]
pub fn may_have_closed<T>(update: gpui_kit::Result<T>) {
    if let Err(error) = update {
        let at = Location::caller();
        tracing::debug!(%at, "update skipped, its window or view is gone: {error:#}");
    }
}

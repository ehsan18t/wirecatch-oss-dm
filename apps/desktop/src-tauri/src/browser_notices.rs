//! Notices in the browser (D65), through `wirecatch_shell::browser_notices`: a notice no browser
//! shows in time becomes a system notification.

use std::time::Duration;

use tauri::{AppHandle, Manager as _, Runtime};
use wirecatch_protocol::app::UiEvent;
use wirecatch_protocol::extension::{NoticeId, NoticeKind, NoticePressed};
use wirecatch_shell::browser_notices::answered;
pub use wirecatch_shell::browser_notices::{ANSWER_WAIT, BrowserNotices};
use wirecatch_shell::notifications::Notification;

use crate::host::Core;
use crate::system_notifications;

/// Tells `notification` in a notice in the browser in front, of `kind`, which stays `stays` (`None` until
/// it is closed), or in a system notification when no browser shows it.
pub fn tell<R: Runtime>(
    app: &AppHandle<R>,
    notification: Notification,
    kind: NoticeKind,
    stays: Option<Duration>,
) {
    let (Some(notices), Some(manager)) = (
        app.try_state::<BrowserNotices>(),
        app.try_state::<Core>().and_then(|core| core.get().cloned()),
    ) else {
        system_notifications::show(app, notification);
        return;
    };
    let Some((id, answer)) = notices.ask(&manager, &notification, kind, stays) else {
        system_notifications::show(app, notification);
        return;
    };
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        if answered(answer).await {
            return;
        }
        if let Some(notices) = app.try_state::<BrowserNotices>() {
            notices.give_up(id);
        }
        system_notifications::show(&app, notification);
    });
}

/// Takes back the notices `event` made outdated, wherever a browser shows them.
pub fn follow<R: Runtime>(app: &AppHandle<R>, event: &UiEvent) {
    if let (Some(notices), Some(manager)) = (
        app.try_state::<BrowserNotices>(),
        app.try_state::<Core>().and_then(|core| core.get().cloned()),
    ) {
        notices.follow(&manager, event);
    }
}

/// A browser showed the notice `id`.
pub fn shown<R: Runtime>(app: &AppHandle<R>, id: NoticeId) {
    if let Some(notices) = app.try_state::<BrowserNotices>() {
        notices.shown(id);
    }
}

/// The person pressed one of a notice's buttons, or the notice itself: does what the same press
/// on the system notification would.
pub fn pressed<R: Runtime>(app: &AppHandle<R>, pressed: &NoticePressed) {
    if let Some(act) = app
        .try_state::<BrowserNotices>()
        .and_then(|notices| notices.pressed(pressed))
    {
        system_notifications::act(app, act);
    }
}

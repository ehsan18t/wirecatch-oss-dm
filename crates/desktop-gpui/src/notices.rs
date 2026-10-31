//! Tells the person what started, finished or failed (D52, D63, D65): in a notice in the browser
//! in front, or in the system's own notification, the way the settings say. A click does what it
//! says; on the notice's text, it brings the main window up on its downloads (D98).

use std::sync::Arc;

use gpui_kit::{App, Global};
use tokio::sync::broadcast::error::RecvError;
use tokio::sync::mpsc;
use tokio::time::Instant;
use wirecatch_core::Manager;
use wirecatch_protocol::app::{AppSettings, UiEvent};
use wirecatch_protocol::extension::NoticeKind;
use wirecatch_protocol::wire::ProtocolError;
use wirecatch_shell::activity::Activity;
use wirecatch_shell::api::{self, Reveal, ToOpen};
use wirecatch_shell::browser_notices::{self, BrowserNotices};
use wirecatch_shell::host::{self, Unopened};
use wirecatch_shell::notifications::{self, Act, Notification};
use wirecatch_shell::system_notifications::{Identity, Notifier};
use wirecatch_shell::teller::{self, Tell, Teller, Telling};

use crate::backend::Backend;
use crate::files;
use crate::library::Asked;
use crate::main_window;

/// The icon the system shows beside the app's name.
const ICON: &[u8] = include_bytes!("../../../apps/desktop/src-tauri/icons/128x128.png");

/// The two ways of telling, shared by the watch and the notices it waits on, and the way to the
/// app's own thread.
struct Tellers {
    system: Notifier,
    browser: BrowserNotices,
    to_app: mpsc::UnboundedSender<ToApp>,
}

/// What the watch hands to the app's own thread.
enum ToApp {
    /// The person clicked a notification or pressed a browser notice.
    Act(Act),
    /// Downloads ended, told in the download complete window.
    Finished(Tell),
}

/// The tellers, for what the app's own thread tells.
struct Kept(Arc<Tellers>);

impl Global for Kept {}

/// Starts telling, once the manager opens, or tells why it could not. Clicks come back to the
/// app's own thread.
pub fn start(cx: &mut App) {
    let (to_app, mut from_watch) = mpsc::unbounded_channel::<ToApp>();
    let folder = dirs::cache_dir()
        .unwrap_or_default()
        .join(host::CHANNEL.identifier());
    let identity = Identity {
        name: host::CHANNEL.name(),
        identifier: host::CHANNEL.identifier(),
        icon: ICON,
        icon_folder: &folder,
    };
    let clicked = to_app.clone();
    let tellers = Arc::new(Tellers {
        system: Notifier::new(
            &identity,
            Arc::new(move |act| {
                if clicked.send(ToApp::Act(act)).is_err() {
                    tracing::debug!("a notification was clicked while the app was closing");
                }
            }),
        ),
        browser: BrowserNotices::default(),
        to_app,
    });
    cx.set_global(Kept(Arc::clone(&tellers)));
    let unopened = Arc::clone(&tellers);
    // Runs until the manager stops, or ends at once when it could not open.
    let watching = Backend::run(cx, move |manager| watch(manager, tellers));
    cx.spawn(async move |_| {
        if let Ok(Err(why)) = watching.await
            && !why.another_copy
        {
            let title = format!("{} could not open its download list", host::CHANNEL.name());
            unopened
                .system
                .show(notifications::plain(title, why.reason.message), true);
        }
    })
    .detach();
    cx.spawn(async move |cx| {
        while let Some(next) = from_watch.recv().await {
            cx.update(|cx| match next {
                ToApp::Act(act) => carry_out(act, cx),
                ToApp::Finished(tell) => crate::download_windows::finished::add(tell, cx),
            });
        }
    })
    .detach();
}

/// Follows the manager's events, telling each as the settings say when it is due.
async fn watch(manager: Manager, tellers: Arc<Tellers>) {
    let mut events = manager.subscribe();
    let mut activity = Activity::default();
    let mut teller = Teller::default();
    let mut settings = reload(&manager, &mut activity, &mut teller);
    for notice in manager.startup_notices() {
        tracing::warn!("{}: {}", notice.title, notice.message);
        show(
            &tellers,
            &settings,
            notifications::plain(notice.title.clone(), notice.message.clone()),
        );
    }
    loop {
        tokio::select! {
            event = events.recv() => match event {
                Ok(event) => {
                    tellers.browser.follow(&manager, &event);
                    teller.ended(activity.apply(&event), Instant::now());
                    match event {
                        UiEvent::SettingsChanged(new) => settings = *new,
                        UiEvent::QueuesChanged(queues) => teller.queues(&queues),
                        UiEvent::QueueFinished(report) => {
                            if let Some(now) = teller.queue_finished(&report) {
                                tell(&manager, &tellers, &settings, &now);
                            }
                        }
                        UiEvent::Captured(row) if teller::told_as_started(&row) => {
                            tell_added(&manager, &tellers, &settings, |stays| {
                                notifications::started(&row, stays)
                            });
                        }
                        UiEvent::MediaCaptured { title, .. } => {
                            tell_added(&manager, &tellers, &settings, |stays| {
                                notifications::video_added(&title, stays)
                            });
                        }
                        UiEvent::NoticeShown(id) => tellers.browser.shown(id),
                        UiEvent::NoticePressed(pressed) => {
                            if let Some(act) = tellers.browser.pressed(&pressed)
                                && tellers.to_app.send(ToApp::Act(act)).is_err()
                            {
                                return;
                            }
                        }
                        _ => {}
                    }
                }
                Err(RecvError::Lagged(_)) => {
                    settings = reload(&manager, &mut activity, &mut teller);
                }
                Err(RecvError::Closed) => return,
            },
            () = due(teller.due()) => {
                for now in teller.take_due(Instant::now()) {
                    tell(&manager, &tellers, &settings, &now);
                }
            }
        }
    }
}

/// Reads again what the watch follows, and the settings.
fn reload(manager: &Manager, activity: &mut Activity, teller: &mut Teller) -> AppSettings {
    activity.load(&manager.list_downloads());
    teller.queues(&manager.list_queues());
    manager.get_settings()
}

async fn due(at: Option<Instant>) {
    match at {
        Some(at) => tokio::time::sleep_until(at).await,
        None => std::future::pending().await,
    }
}

/// Tells `now` the way `settings` say.
fn tell(manager: &Manager, tellers: &Arc<Tellers>, settings: &AppSettings, now: &Tell) {
    match now.telling(settings) {
        Telling::Nothing => {}
        Telling::System(notification) => show(tellers, settings, notification),
        Telling::Window => {
            if tellers.to_app.send(ToApp::Finished(now.clone())).is_err() {
                tracing::debug!("downloads ended while the app was closing");
            }
        }
        Telling::Browser {
            notification,
            kind,
            stays,
        } => ask_browser(manager, tellers, settings, notification, kind, stays),
    }
}

/// Tells the start notice `made` gives, while the settings tell what started.
fn tell_added(
    manager: &Manager,
    tellers: &Arc<Tellers>,
    settings: &AppSettings,
    made: impl FnOnce(std::time::Duration) -> Notification,
) {
    if let Some((notification, stays)) = teller::start_notice(settings, made) {
        ask_browser(
            manager,
            tellers,
            settings,
            notification,
            NoticeKind::Started,
            Some(stays),
        );
    }
}

/// Tells `notification` in the browser in front, or in a system notification when no browser
/// shows it in time.
fn ask_browser(
    manager: &Manager,
    tellers: &Arc<Tellers>,
    settings: &AppSettings,
    notification: Notification,
    kind: NoticeKind,
    stays: Option<std::time::Duration>,
) {
    let Some((id, answer)) = tellers.browser.ask(manager, &notification, kind, stays) else {
        return show(tellers, settings, notification);
    };
    let (tellers, through) = (
        Arc::clone(tellers),
        through_do_not_disturb(settings, &notification),
    );
    tokio::spawn(async move {
        if !browser_notices::answered(answer).await {
            tellers.browser.give_up(id);
            tellers.system.show(notification, through);
        }
    });
}

fn show(tellers: &Tellers, settings: &AppSettings, notification: Notification) {
    let through = through_do_not_disturb(settings, &notification);
    tellers.system.show(notification, through);
}

/// An important notification asks to show during Do Not Disturb while the person allows it (D64).
fn through_do_not_disturb(settings: &AppSettings, notification: &Notification) -> bool {
    notification.important && settings.notify_during_do_not_disturb
}

/// Does what the person clicked. What cannot be done is told in a notification of its own, since
/// no window asked.
fn carry_out(act: Act, cx: &mut App) {
    match act {
        Act::Show(reveal) => main_window::bring_up(&Asked::Reveal(reveal), cx),
        Act::Open(id) => {
            let found = Backend::run(
                cx,
                move |manager| async move { api::file_to_open(&manager, id) },
            );
            cx.spawn(async move |cx| {
                let found = answer(found.await);
                cx.update(|cx| match found {
                    Some(Ok(ToOpen::File(path))) => cx.open_with_system(&path),
                    Some(Ok(ToOpen::Program { path, warning })) => {
                        main_window::bring_up_with(cx, |window, cx| {
                            files::ask_to_open(path, warning, window, cx);
                        });
                    }
                    Some(Err(refused)) => could_not(refused, cx),
                    None => {}
                });
            })
            .detach();
        }
        Act::ShowInFolder(id) => {
            let found = Backend::run(cx, move |manager| async move {
                api::item_to_reveal(&manager, id)
            });
            cx.spawn(async move |cx| {
                let found = answer(found.await);
                cx.update(|cx| match found {
                    Some(Ok(Reveal::File(path))) => cx.reveal_path(&path),
                    Some(Ok(Reveal::Folder(folder))) => cx.open_with_system(&folder),
                    Some(Err(refused)) => could_not(refused, cx),
                    None => {}
                });
            })
            .detach();
        }
        Act::Retry(id) => {
            let tried = Backend::run(cx, move |manager| async move { api::retry(&manager, id) });
            cx.spawn(async move |cx| {
                if let Some(Err(refused)) = answer(tried.await) {
                    cx.update(|cx| could_not(refused, cx));
                }
            })
            .detach();
        }
    }
}

/// What the manager answered; `None` when the work stopped before it could.
fn answer<T>(
    done: Result<Result<Result<T, ProtocolError>, Unopened>, tokio::task::JoinError>,
) -> Option<Result<T, ProtocolError>> {
    match done {
        Ok(Ok(answered)) => Some(answered),
        Ok(Err(unopened)) => Some(Err(unopened.reason)),
        Err(e) => {
            tracing::error!("a notification's button stopped unexpectedly: {e}");
            None
        }
    }
}

/// The download list could not be saved: told in a notification while no main window shows it.
pub fn storage_failed(reason: &str, cx: &mut App) {
    if main_window::is_open(cx) {
        return;
    }
    let title = format!("{} could not save its download list", host::CHANNEL.name());
    if let Some(Kept(tellers)) = cx.try_global::<Kept>() {
        tellers
            .system
            .show(notifications::plain(title, reason.to_owned()), true);
    }
}

fn could_not(refused: ProtocolError, cx: &mut App) {
    let title = format!("{} could not do that", host::CHANNEL.name());
    let (manager, _) = Backend::now(cx);
    let through = manager.is_none_or(|m| m.get_settings().notify_during_do_not_disturb);
    if let Some(Kept(tellers)) = cx.try_global::<Kept>() {
        tellers
            .system
            .show(notifications::plain(title, refused.message), through);
    }
}

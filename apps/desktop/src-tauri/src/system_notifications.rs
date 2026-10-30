//! Shows [`Notification`]s as the system's own notifications (`wirecatch_shell::system_notifications`)
//! and does what the person clicks (D52).

use std::sync::{Arc, Mutex, PoisonError};

use crate::host::Core;
use tauri::async_runtime::JoinHandle;
use tauri::{AppHandle, Manager as _, Runtime};
use wirecatch_shell::notifications::{Act, Notification, plain};
use wirecatch_shell::system_notifications::{Identity, Notifier};

/// The notifications shown, recorded instead when this is managed (tests do; the app does not).
#[derive(Default)]
pub struct ShownNotifications(pub Mutex<Vec<Shown>>);

/// A notification as it was shown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shown {
    /// What it said and did.
    pub notification: Notification,
    /// Whether it asked to show during the system's Do Not Disturb (D64).
    pub through_do_not_disturb: bool,
}

/// The system's notifications, managed by the app: tests run without them.
pub struct SystemNotifications {
    notifier: Notifier,
    /// What clicks started: each ends by itself, and is stopped when the app quits by dropping
    /// the runtime.
    tasks: Mutex<Vec<JoinHandle<()>>>,
}

/// The icon the system shows beside the app's name.
const ICON: &[u8] = include_bytes!("../icons/128x128.png");

impl SystemNotifications {
    /// Readies the notifications of the app named `name`, with the identity `identifier`.
    #[must_use]
    pub fn new<R: Runtime>(app: &AppHandle<R>, name: &str, identifier: &str) -> Self {
        let icon_folder = app.path().app_cache_dir().unwrap_or_default();
        let identity = Identity {
            name,
            identifier,
            icon: ICON,
            icon_folder: &icon_folder,
        };
        let clicked = app.clone();
        Self {
            notifier: Notifier::new(&identity, Arc::new(move |chosen| act(&clicked, chosen))),
            tasks: Mutex::default(),
        }
    }

    fn keep(&self, task: JoinHandle<()>) {
        let mut tasks = self.tasks.lock().unwrap_or_else(PoisonError::into_inner);
        tasks.retain(|t| !t.inner().is_finished());
        tasks.push(task);
    }
}

/// Whether `notification` asks to show during the system's Do Not Disturb (D64): an important one does
/// while the person allows it, which the settings say once the download list is open and their
/// default says before.
fn through_do_not_disturb<R: Runtime>(app: &AppHandle<R>, notification: &Notification) -> bool {
    notification.important
        && app
            .try_state::<Core>()
            .and_then(|core| {
                core.get()
                    .map(|m| m.get_settings().notify_during_do_not_disturb)
            })
            .unwrap_or(true)
}

/// Shows `notification`. Does nothing more than log when the app has no notifications (tests).
pub fn show<R: Runtime>(app: &AppHandle<R>, notification: Notification) {
    let through = through_do_not_disturb(app, &notification);
    if let Some(shown) = app.try_state::<ShownNotifications>() {
        shown
            .0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(Shown {
                notification,
                through_do_not_disturb: through,
            });
        return;
    }
    let Some(system) = app.try_state::<SystemNotifications>() else {
        tracing::info!(
            "notification (no notification support): {}: {}",
            notification.title,
            notification.body
        );
        return;
    };
    system.notifier.show(notification, through);
}

/// Does `act`, the person's click on a notification. A failure is told in a notification of its
/// own, since no window asked.
pub fn act<R: Runtime>(app: &AppHandle<R>, act: Act) {
    let task_app = app.clone();
    let task = tauri::async_runtime::spawn(async move {
        if let Err(message) = carry_out(&task_app, act).await {
            show(
                &task_app,
                plain("Wirecatch could not do that".to_owned(), message),
            );
        }
    });
    match app.try_state::<SystemNotifications>() {
        Some(system) => system.keep(task),
        None => drop(task),
    }
}

async fn carry_out<R: Runtime>(app: &AppHandle<R>, act: Act) -> Result<(), String> {
    if let Act::Show(reveal) = act {
        crate::window::reveal(app, &reveal);
        return Ok(());
    }
    let core = app
        .try_state::<Core>()
        .ok_or_else(|| "Wirecatch is still starting. Try again in a moment.".to_owned())?;
    let manager = core.manager().await.map_err(|e| e.message)?;
    let result = match act {
        Act::Open(id) => crate::files::open_download(app, manager, id, None).await,
        Act::ShowInFolder(id) => crate::files::show_in_folder(app, manager, id),
        Act::Retry(id) => crate::api::retry(manager, id),
        Act::Show(_) => Ok(()),
    };
    result.map_err(|e| e.message)
}

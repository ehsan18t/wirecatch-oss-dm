//! Shows [`Notice`]s as the system's own notifications, with their buttons, and does what the
//! person clicks (D52). Windows shows them as toasts under the app's own identity, registered
//! here for the current user so a debug build is named and shown too; Linux sends them to the
//! desktop's notification service, with actions where it offers them. A brief notice expires on
//! Windows, which takes it out of the notification centre, and is transient on Linux, which keeps
//! it out of the desktop's history (D63). On Windows an important notice asks to show during Do
//! Not Disturb while the person allows it (D64).

use std::sync::{Mutex, PoisonError};

use crate::host::Core;
use crate::notices::{Act, Lasts, Notice};
use tauri::async_runtime::JoinHandle;
use tauri::{AppHandle, Manager as _, Runtime};

/// The notifications shown, recorded instead when this is managed (tests do; the app does not).
#[derive(Default)]
pub struct ShownNotices(pub Mutex<Vec<Shown>>);

/// A notification as it was shown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shown {
    /// What it said and did.
    pub notice: Notice,
    /// Whether it asked to show during the system's Do Not Disturb (D64).
    pub through_do_not_disturb: bool,
}

/// The system's notifications, managed by the app: tests run without them.
pub struct SystemNotices {
    platform: platform::Notifier,
    /// What clicks started and what waits for a click: each ends by itself, and is stopped when
    /// the app quits by dropping the runtime.
    tasks: Mutex<Vec<JoinHandle<()>>>,
}

impl SystemNotices {
    /// Readies the notifications of the app named `name`, with the identity `identifier`.
    #[must_use]
    pub fn new<R: Runtime>(app: &AppHandle<R>, name: &str, identifier: &str) -> Self {
        Self {
            platform: platform::Notifier::new(app, name, identifier),
            tasks: Mutex::default(),
        }
    }

    fn keep(&self, task: JoinHandle<()>) {
        let mut tasks = self.tasks.lock().unwrap_or_else(PoisonError::into_inner);
        tasks.retain(|t| !t.inner().is_finished());
        tasks.push(task);
    }
}

/// A notification with words only about something the person must know; a click shows the main
/// window.
#[must_use]
pub fn plain(title: String, body: String) -> Notice {
    Notice {
        title,
        body,
        buttons: Vec::new(),
        click: Act::Show(wirecatch_protocol::desktop::Reveal {
            queue: None,
            ids: Vec::new(),
            browser_settings: false,
            proxy_settings: false,
        }),
        lasts: Lasts::Kept,
        important: true,
    }
}

/// Whether `notice` asks to show during the system's Do Not Disturb (D64): an important one does
/// while the person allows it, which the settings say once the download list is open and their
/// default says before.
fn through_do_not_disturb<R: Runtime>(app: &AppHandle<R>, notice: &Notice) -> bool {
    notice.important
        && app
            .try_state::<Core>()
            .and_then(|core| {
                core.get()
                    .map(|m| m.get_settings().notify_during_do_not_disturb)
            })
            .unwrap_or(true)
}

/// Shows `notice`. Does nothing more than log when the app has no notifications (tests).
pub fn show<R: Runtime>(app: &AppHandle<R>, notice: Notice) {
    let through = through_do_not_disturb(app, &notice);
    if let Some(shown) = app.try_state::<ShownNotices>() {
        shown
            .0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(Shown {
                notice,
                through_do_not_disturb: through,
            });
        return;
    }
    let Some(system) = app.try_state::<SystemNotices>() else {
        tracing::info!(
            "notification (no notification support): {}: {}",
            notice.title,
            notice.body
        );
        return;
    };
    system.platform.show(app, notice, through);
}

/// Picks what a click did: a button by its place, or the notification itself.
pub(crate) fn chosen(notice: &Notice, button: Option<usize>) -> Option<Act> {
    match button {
        None => Some(notice.click.clone()),
        Some(at) => notice.buttons.get(at).map(|b| b.act.clone()),
    }
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
    match app.try_state::<SystemNotices>() {
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

/// Escapes text for a notification's markup: file names come from websites. Only what markup
/// needs, since a service that shows a body as plain text would show any other entity as written.
fn escape_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(c),
        }
    }
    out
}

/// What a button sends back when clicked: its place among the buttons.
fn button_argument(at: usize) -> String {
    at.to_string()
}

/// The button a clicked notification sent back, or `None` for a click on the notification
/// itself (which sends nothing).
fn clicked_button(argument: &str) -> Option<usize> {
    argument.parse().ok()
}

#[cfg(windows)]
mod platform {
    //! Windows toasts, through the `windows` crate's projection of `WinRT`.

    use std::collections::VecDeque;
    use std::sync::{Mutex, PoisonError};
    use std::time::{SystemTime, UNIX_EPOCH};

    use tauri::{AppHandle, Manager as _, Runtime};
    use windows::Data::Xml::Dom::XmlDocument;
    use windows::Foundation::{DateTime, IReference, PropertyValue, TypedEventHandler};
    use windows::UI::Notifications::{
        ToastActivatedEventArgs, ToastNotification, ToastNotificationManager,
    };
    use windows_core::{HSTRING, IInspectable, Interface as _};

    use super::{button_argument, chosen, clicked_button, escape_text};
    use crate::notices::{Lasts, Notice};

    /// Escapes text for the toast's XML, quotes included, so it is safe inside an attribute too.
    fn escape(text: &str) -> String {
        escape_text(text)
            .replace('"', "&quot;")
            .replace('\'', "&apos;")
    }

    /// Toasts kept alive so their click handlers still run when the person clicks them in the
    /// notification centre, this many of each [`Lasts`]: brief ones apart, so a run of them never
    /// drops a kept one. The oldest go first.
    const KEPT_TOASTS: usize = 32;

    /// Where the current user's app identities are registered, so toasts show the app's name
    /// and icon without a Start menu shortcut.
    const IDENTITIES_KEY: &str = r"Software\Classes\AppUserModelId";

    /// The first Windows build that shows an important toast during Do Not Disturb. Before it, a
    /// toast is never marked important.
    const IMPORTANT_TOASTS_FROM: u32 = 19041;

    /// The icon file written for the identity: the system reads it from disk.
    const ICON_FILE: &str = "notification-icon.png";
    const ICON: &[u8] = include_bytes!("../icons/128x128.png");

    pub struct Notifier {
        identifier: HSTRING,
        /// Whether this Windows shows important toasts.
        important_toasts: bool,
        kept: Mutex<VecDeque<ToastNotification>>,
        brief: Mutex<VecDeque<ToastNotification>>,
    }

    impl Notifier {
        pub fn new<R: Runtime>(app: &AppHandle<R>, name: &str, identifier: &str) -> Self {
            if let Err(e) = register(app, name, identifier) {
                tracing::warn!("could not register the app for notifications: {e}");
            }
            Self {
                identifier: HSTRING::from(identifier),
                important_toasts: build().is_some_and(|b| b >= IMPORTANT_TOASTS_FROM),
                kept: Mutex::default(),
                brief: Mutex::default(),
            }
        }

        pub fn show<R: Runtime>(&self, app: &AppHandle<R>, notice: Notice, through: bool) {
            let held = match notice.lasts {
                Lasts::Kept => &self.kept,
                Lasts::Brief(_) => &self.brief,
            };
            match self.toast(app, notice, through && self.important_toasts) {
                Ok(toast) => {
                    let mut kept = held.lock().unwrap_or_else(PoisonError::into_inner);
                    if kept.len() >= KEPT_TOASTS {
                        kept.pop_front();
                    }
                    kept.push_back(toast);
                }
                Err(e) => tracing::warn!("could not show a notification: {e}"),
            }
        }

        fn toast<R: Runtime>(
            &self,
            app: &AppHandle<R>,
            notice: Notice,
            important: bool,
        ) -> windows_core::Result<ToastNotification> {
            let xml = XmlDocument::new()?;
            xml.LoadXml(&HSTRING::from(toast_xml(&notice, important)))?;
            let toast = ToastNotification::CreateToastNotification(&xml)?;
            if let Lasts::Brief(stays) = notice.lasts {
                let expires = PropertyValue::CreateDateTime(DateTime {
                    UniversalTime: ticks(SystemTime::now() + stays),
                })?;
                toast.SetExpirationTime(&expires.cast::<IReference<DateTime>>()?)?;
            }
            let app = app.clone();
            toast.Activated(&TypedEventHandler::<ToastNotification, IInspectable>::new(
                move |_, args| {
                    let argument = args
                        .as_ref()
                        .and_then(|a| a.cast::<ToastActivatedEventArgs>().ok())
                        .and_then(|a| a.Arguments().ok())
                        .map(|a| a.to_string_lossy())
                        .unwrap_or_default();
                    if let Some(act) = chosen(&notice, clicked_button(&argument)) {
                        super::act(&app, act);
                    }
                    Ok(())
                },
            ))?;
            ToastNotificationManager::CreateToastNotifierWithId(&self.identifier)?.Show(&toast)?;
            Ok(toast)
        }
    }

    /// Windows' time for `at`: 100-nanosecond ticks since 1601.
    fn ticks(at: SystemTime) -> i64 {
        /// The ticks from 1601 to 1970.
        const BEFORE_1970: i64 = 116_444_736_000_000_000;
        let since_1970 = at.duration_since(UNIX_EPOCH).unwrap_or_default();
        i64::try_from(since_1970.as_nanos() / 100)
            .map_or(i64::MAX, |ticks| BEFORE_1970.saturating_add(ticks))
    }

    /// The build of the running Windows, from the registry.
    fn build() -> Option<u32> {
        let key = winreg::RegKey::predef(winreg::enums::HKEY_LOCAL_MACHINE)
            .open_subkey(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion")
            .ok()?;
        key.get_value::<String, _>("CurrentBuildNumber")
            .ok()?
            .parse()
            .ok()
    }

    /// The toast's XML; an `important` one shows during Do Not Disturb (the `urgent` scenario).
    fn toast_xml(notice: &Notice, important: bool) -> String {
        let mut actions = String::new();
        for (at, button) in notice.buttons.iter().enumerate() {
            actions.push_str(r#"<action content=""#);
            actions.push_str(&escape(button.label));
            actions.push_str(r#"" arguments=""#);
            actions.push_str(&button_argument(at));
            actions.push_str(r#"" activationType="foreground"/>"#);
        }
        let scenario = if important {
            r#" scenario="urgent""#
        } else {
            ""
        };
        format!(
            r#"<toast{scenario}><visual><binding template="ToastGeneric"><text>{}</text><text>{}</text></binding></visual><actions>{actions}</actions></toast>"#,
            escape(&notice.title),
            escape(&notice.body)
        )
    }

    /// Registers `identifier` for the current user with the app's name and icon, which the
    /// system shows on its toasts. An installed app's Start menu shortcut carries the same
    /// identity; a debug build has no shortcut, so this is what makes its toasts appear.
    fn register<R: Runtime>(
        app: &AppHandle<R>,
        name: &str,
        identifier: &str,
    ) -> Result<(), String> {
        let icon = app
            .path()
            .app_cache_dir()
            .map_err(|e| format!("no folder for the icon ({e})"))?;
        std::fs::create_dir_all(&icon).map_err(|e| format!("{} ({e})", icon.display()))?;
        let icon = icon.join(ICON_FILE);
        if std::fs::read(&icon).ok().as_deref() != Some(ICON) {
            std::fs::write(&icon, ICON).map_err(|e| format!("{} ({e})", icon.display()))?;
        }
        let (key, _) = winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER)
            .create_subkey(format!(r"{IDENTITIES_KEY}\{identifier}"))
            .map_err(|e| format!("the registry refused ({e})"))?;
        key.set_value("DisplayName", &name)
            .and_then(|()| key.set_value("IconUri", &icon.to_string_lossy().into_owned()))
            .map_err(|e| format!("the registry refused ({e})"))
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use crate::notices::{Act, Button};
        use wirecatch_protocol::DownloadId;

        #[test]
        fn text_from_websites_cannot_change_the_toast() {
            let notice = Notice {
                title: "Download finished".into(),
                body: "a</text><action content='x'/>&.zip".into(),
                buttons: vec![Button {
                    label: "Open",
                    act: Act::Open(DownloadId(1)),
                }],
                click: Act::Open(DownloadId(1)),
                lasts: crate::notices::Lasts::Kept,
                important: true,
            };
            let xml = toast_xml(&notice, false);
            assert!(!xml.contains("scenario"), "{xml}");
            assert!(toast_xml(&notice, true).starts_with(r#"<toast scenario="urgent">"#));
            assert!(
                xml.contains("a&lt;/text&gt;&lt;action content=&apos;x&apos;/&gt;&amp;.zip"),
                "{xml}"
            );
            assert_eq!(xml.matches("<action ").count(), 1);
            assert!(xml.contains(r#"arguments="0""#));
        }

        #[test]
        fn the_running_windows_says_its_build() {
            assert!(build().is_some_and(|b| b > 0));
        }

        #[test]
        fn windows_time_counts_ticks_from_1601() {
            assert_eq!(ticks(UNIX_EPOCH), 116_444_736_000_000_000);
            let later = UNIX_EPOCH + std::time::Duration::from_secs(1);
            assert_eq!(ticks(later) - ticks(UNIX_EPOCH), 10_000_000);
        }
    }
}

#[cfg(target_os = "linux")]
mod platform {
    //! The desktop's notification service (the freedesktop.org interface), through notify-rust.

    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use tauri::{AppHandle, Manager as _, Runtime};

    use super::{button_argument, chosen, clicked_button, escape_text};
    use crate::notices::{Lasts, Notice};

    /// The action a notification service sends for a click on the notification itself.
    const CLICK: &str = "default";

    /// Notifications whose click is waited for at once. Waiting holds a thread until the
    /// notification closes, which a desktop that keeps notifications in its history may never
    /// do; past this many, a notification shows without buttons and nothing waits for it.
    const MAX_WAITING: usize = 16;

    pub struct Notifier {
        name: String,
        waiting: Arc<AtomicUsize>,
    }

    impl Notifier {
        pub fn new<R: Runtime>(_app: &AppHandle<R>, name: &str, _identifier: &str) -> Self {
            Self {
                name: name.to_owned(),
                waiting: Arc::default(),
            }
        }

        /// Shows `notice`. Nothing is marked to show during Do Not Disturb (D64.3): the desktops
        /// that let a critical notification through also keep it on screen until it is clicked,
        /// which every finished download would then do.
        pub fn show<R: Runtime>(&self, app: &AppHandle<R>, notice: Notice, _through: bool) {
            let waits = self.waiting.fetch_add(1, Ordering::SeqCst) < MAX_WAITING;
            if !waits {
                self.waiting.fetch_sub(1, Ordering::SeqCst);
            }
            let name = self.name.clone();
            let waiting = self.waiting.clone();
            let task_app = app.clone();
            // Showing talks to the service over D-Bus and waiting blocks: neither on the async threads.
            let task = tauri::async_runtime::spawn_blocking(move || {
                show_and_wait(&task_app, &name, &notice, waits);
                if waits {
                    waiting.fetch_sub(1, Ordering::SeqCst);
                }
            });
            if let Some(system) = app.try_state::<super::SystemNotices>() {
                system.keep(task);
            }
        }
    }

    fn show_and_wait<R: Runtime>(app: &AppHandle<R>, name: &str, notice: &Notice, waits: bool) {
        let mut notification = notify_rust::Notification::new();
        notification
            .appname(name)
            .summary(&notice.title)
            .body(&escape_text(&notice.body));
        if let Lasts::Brief(stays) = notice.lasts {
            notification
                .timeout(stays)
                .hint(notify_rust::Hint::Transient(true));
        }
        if waits {
            notification.action(CLICK, "");
            for (at, button) in notice.buttons.iter().enumerate() {
                notification.action(&button_argument(at), button.label);
            }
        }
        let handle = match notification.show() {
            Ok(handle) => handle,
            Err(e) => {
                tracing::warn!("could not show a notification: {e}");
                return;
            }
        };
        if !waits {
            return;
        }
        handle.wait_for_action(|action| {
            let picked = match action {
                CLICK => chosen(notice, None),
                other => clicked_button(other).and_then(|at| chosen(notice, Some(at))),
            };
            if let Some(act) = picked {
                super::act(app, act);
            }
        });
    }
}

#[cfg(not(any(windows, target_os = "linux")))]
mod platform {
    //! Wirecatch ships for Windows and Linux; elsewhere notifications are only logged.

    use tauri::{AppHandle, Runtime};

    use crate::notices::Notice;

    pub struct Notifier;

    impl Notifier {
        pub fn new<R: Runtime>(_app: &AppHandle<R>, _name: &str, _identifier: &str) -> Self {
            Self
        }

        pub fn show<R: Runtime>(&self, _app: &AppHandle<R>, notice: Notice, _through: bool) {
            tracing::info!("notification: {}: {}", notice.title, notice.body);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notices::Button;
    use wirecatch_protocol::DownloadId;

    #[test]
    fn a_click_picks_a_button_by_its_place_or_the_notification_itself() {
        let notice = Notice {
            title: String::new(),
            body: String::new(),
            buttons: vec![
                Button {
                    label: "Open",
                    act: Act::Open(DownloadId(1)),
                },
                Button {
                    label: "Show in folder",
                    act: Act::ShowInFolder(DownloadId(1)),
                },
            ],
            click: Act::Retry(DownloadId(1)),
            lasts: Lasts::Kept,
            important: true,
        };
        let pick = |argument: &str| chosen(&notice, clicked_button(argument));
        assert_eq!(
            pick(&button_argument(1)),
            Some(Act::ShowInFolder(DownloadId(1)))
        );
        assert_eq!(pick(""), Some(Act::Retry(DownloadId(1))));
        assert_eq!(pick("7"), None, "no such button");
    }
}

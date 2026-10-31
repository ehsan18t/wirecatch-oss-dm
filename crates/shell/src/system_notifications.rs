//! Shows [`Notification`]s as the system's own notifications, with their buttons, and hands what
//! the person clicks to the app (D52). Windows shows them as toasts under the app's own identity,
//! registered here for the current user so a debug build is named and shown too; Linux sends them
//! to the desktop's notification service, with actions where it offers them. A brief notification
//! expires on Windows, which takes it out of the notification centre, and is transient on Linux,
//! which keeps it out of the desktop's history (D63). On Windows an important notification asks
//! to show during Do Not Disturb while the person allows it (D64).

use std::path::Path;
use std::sync::Arc;

use crate::notifications::{Act, Notification};

/// What the app does with a click: a button's act, or the notification's own.
pub type OnAct = Arc<dyn Fn(Act) + Send + Sync>;

/// Who the notifications are from.
pub struct Identity<'a> {
    /// The app's name, shown on every notification.
    pub name: &'a str,
    /// The app's identifier, under which Windows keeps its notifications.
    pub identifier: &'a str,
    /// The PNG icon Windows shows beside the name, written to `icon_folder` for the system to read.
    pub icon: &'static [u8],
    /// A folder of the app's own for the icon file.
    pub icon_folder: &'a Path,
}

/// The system's notifications of one app.
pub struct Notifier {
    platform: platform::Notifier,
}

impl Notifier {
    /// Readies the notifications of `identity`; `on_act` runs for each click, on any thread.
    #[must_use]
    pub fn new(identity: &Identity<'_>, on_act: OnAct) -> Self {
        Self {
            platform: platform::Notifier::new(identity, on_act),
        }
    }

    /// Shows `notification`; `through_do_not_disturb` asks to show it during the system's Do Not
    /// Disturb (D64).
    pub fn show(&self, notification: Notification, through_do_not_disturb: bool) {
        self.platform.show(notification, through_do_not_disturb);
    }
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

    use windows::Data::Xml::Dom::XmlDocument;
    use windows::Foundation::{DateTime, IReference, PropertyValue, TypedEventHandler};
    use windows::UI::Notifications::{
        ToastActivatedEventArgs, ToastNotification, ToastNotificationManager,
    };
    use windows_core::{HSTRING, IInspectable, Interface as _};

    use super::{OnAct, button_argument, clicked_button, escape_text};
    use crate::notifications::{Lasts, Notification, chosen};

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

    pub struct Notifier {
        identifier: HSTRING,
        on_act: OnAct,
        /// Whether this Windows shows important toasts.
        important_toasts: bool,
        kept: Mutex<VecDeque<ToastNotification>>,
        brief: Mutex<VecDeque<ToastNotification>>,
    }

    impl Notifier {
        pub fn new(identity: &super::Identity<'_>, on_act: OnAct) -> Self {
            if let Err(e) = register(identity) {
                tracing::warn!("could not register the app for notifications: {e}");
            }
            Self {
                identifier: HSTRING::from(identity.identifier),
                on_act,
                important_toasts: build().is_some_and(|b| b >= IMPORTANT_TOASTS_FROM),
                kept: Mutex::default(),
                brief: Mutex::default(),
            }
        }

        pub fn show(&self, notification: Notification, through: bool) {
            let held = match notification.lasts {
                Lasts::Kept => &self.kept,
                Lasts::Brief(_) => &self.brief,
            };
            match self.toast(notification, through && self.important_toasts) {
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

        fn toast(
            &self,
            notification: Notification,
            important: bool,
        ) -> windows_core::Result<ToastNotification> {
            let xml = XmlDocument::new()?;
            xml.LoadXml(&HSTRING::from(toast_xml(&notification, important)))?;
            let toast = ToastNotification::CreateToastNotification(&xml)?;
            if let Lasts::Brief(stays) = notification.lasts {
                let expires = PropertyValue::CreateDateTime(DateTime {
                    UniversalTime: ticks(SystemTime::now() + stays),
                })?;
                toast.SetExpirationTime(&expires.cast::<IReference<DateTime>>()?)?;
            }
            let on_act = self.on_act.clone();
            toast.Activated(&TypedEventHandler::<ToastNotification, IInspectable>::new(
                move |_, args| {
                    let argument = args
                        .as_ref()
                        .and_then(|a| a.cast::<ToastActivatedEventArgs>().ok())
                        .and_then(|a| a.Arguments().ok())
                        .map(|a| a.to_string_lossy())
                        .unwrap_or_default();
                    if let Some(act) = chosen(&notification, clicked_button(&argument)) {
                        on_act(act);
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
    fn toast_xml(notification: &Notification, important: bool) -> String {
        let mut actions = String::new();
        for (at, button) in notification.buttons.iter().enumerate() {
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
            escape(&notification.title),
            escape(&notification.body)
        )
    }

    /// Registers `identifier` for the current user with the app's name and icon, which the
    /// system shows on its toasts. An installed app's Start menu shortcut carries the same
    /// identity; a debug build has no shortcut, so this is what makes its toasts appear.
    fn register(identity: &super::Identity<'_>) -> Result<(), RegisterError> {
        let folder = identity.icon_folder;
        std::fs::create_dir_all(folder).map_err(|source| RegisterError::Icon {
            path: folder.to_owned(),
            source,
        })?;
        let icon = folder.join(ICON_FILE);
        if std::fs::read(&icon).ok().as_deref() != Some(identity.icon) {
            std::fs::write(&icon, identity.icon).map_err(|source| RegisterError::Icon {
                path: icon.clone(),
                source,
            })?;
        }
        let (key, _) = winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER)
            .create_subkey(format!(r"{IDENTITIES_KEY}\{}", identity.identifier))
            .map_err(RegisterError::Registry)?;
        key.set_value("DisplayName", &identity.name)
            .and_then(|()| key.set_value("IconUri", &icon.to_string_lossy().into_owned()))
            .map_err(RegisterError::Registry)
    }

    /// Why the app's notification identity could not be registered.
    #[derive(Debug, thiserror::Error)]
    enum RegisterError {
        #[error("{} ({source})", .path.display())]
        Icon {
            path: std::path::PathBuf,
            source: std::io::Error,
        },
        #[error("the registry refused ({0})")]
        Registry(#[source] std::io::Error),
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use crate::notifications::{Act, Button};
        use wirecatch_protocol::DownloadId;

        #[test]
        fn text_from_websites_cannot_change_the_toast() {
            let notification = Notification {
                title: "Download finished".into(),
                body: "a</text><action content='x'/>&.zip".into(),
                buttons: vec![Button {
                    label: "Open",
                    act: Act::Open(DownloadId(1)),
                }],
                click: Act::Open(DownloadId(1)),
                lasts: Lasts::Kept,
                important: true,
            };
            let xml = toast_xml(&notification, false);
            assert!(!xml.contains("scenario"), "{xml}");
            assert!(toast_xml(&notification, true).starts_with(r#"<toast scenario="urgent">"#));
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

    use super::{OnAct, button_argument, clicked_button, escape_text};
    use crate::notifications::{Lasts, Notification, chosen};

    /// The action a notification service sends for a click on the notification itself.
    const CLICK: &str = "default";

    /// Notifications whose click is waited for at once. Waiting holds a thread until the
    /// notification closes, which a desktop that keeps notifications in its history may never
    /// do; past this many, a notification shows without buttons and nothing waits for it.
    const MAX_WAITING: usize = 16;

    pub struct Notifier {
        name: String,
        on_act: OnAct,
        waiting: Arc<AtomicUsize>,
    }

    impl Notifier {
        pub fn new(identity: &super::Identity<'_>, on_act: OnAct) -> Self {
            Self {
                name: identity.name.to_owned(),
                on_act,
                waiting: Arc::default(),
            }
        }

        /// Shows `notification`. Nothing is marked to show during Do Not Disturb (D64.3): the desktops
        /// that let a critical notification through also keep it on screen until it is clicked,
        /// which every finished download would then do.
        pub fn show(&self, notification: Notification, _through: bool) {
            let waits = self.waiting.fetch_add(1, Ordering::SeqCst) < MAX_WAITING;
            if !waits {
                self.waiting.fetch_sub(1, Ordering::SeqCst);
            }
            let name = self.name.clone();
            let waiting = self.waiting.clone();
            let on_act = self.on_act.clone();
            // Showing talks to the service over D-Bus and waiting blocks: neither on the async threads.
            let shown = std::thread::Builder::new()
                .name("wirecatch-notification".into())
                .spawn(move || {
                    show_and_wait(&on_act, &name, &notification, waits);
                    if waits {
                        waiting.fetch_sub(1, Ordering::SeqCst);
                    }
                });
            if let Err(e) = shown {
                tracing::warn!("could not show a notification: {e}");
            }
        }
    }

    fn show_and_wait(on_act: &OnAct, name: &str, notification: &Notification, waits: bool) {
        let mut shown = notify_rust::Notification::new();
        shown
            .appname(name)
            .summary(&notification.title)
            .body(&escape_text(&notification.body));
        if let Lasts::Brief(stays) = notification.lasts {
            shown
                .timeout(stays)
                .hint(notify_rust::Hint::Transient(true));
        }
        if waits {
            shown.action(CLICK, "");
            for (at, button) in notification.buttons.iter().enumerate() {
                shown.action(&button_argument(at), button.label);
            }
        }
        let handle = match shown.show() {
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
                CLICK => chosen(notification, None),
                other => clicked_button(other).and_then(|at| chosen(notification, Some(at))),
            };
            if let Some(act) = picked {
                on_act(act);
            }
        });
    }
}

#[cfg(not(any(windows, target_os = "linux")))]
mod platform {
    //! Wirecatch ships for Windows and Linux; elsewhere notifications are only logged.

    use super::OnAct;
    use crate::notifications::Notification;

    pub struct Notifier;

    impl Notifier {
        pub fn new(_identity: &super::Identity<'_>, _on_act: OnAct) -> Self {
            Self
        }

        pub fn show(&self, notification: Notification, _through: bool) {
            tracing::info!(
                "notification: {}: {}",
                notification.title,
                notification.body
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notifications::{Act, Button, Lasts, chosen};
    use wirecatch_protocol::DownloadId;

    #[test]
    fn a_click_picks_a_button_by_its_place_or_the_notification_itself() {
        let notification = Notification {
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
        let pick = |argument: &str| chosen(&notification, clicked_button(argument));
        assert_eq!(
            pick(&button_argument(1)),
            Some(Act::ShowInFolder(DownloadId(1)))
        );
        assert_eq!(pick(""), Some(Act::Retry(DownloadId(1))));
        assert_eq!(pick("7"), None, "no such button");
    }
}

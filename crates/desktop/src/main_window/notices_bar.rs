//! What the window must tell before anything else: that the download list could not be saved,
//! above the list until it saves again or is dismissed, and this start's notices, once.

use gpui_kit::component::Sizable as _;
use gpui_kit::component::WindowExt as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::progress::Progress as ProgressBar;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, Context, Entity, IntoElement, ParentElement as _, Styled as _, Window, div,
};
use wirecatch_core::AppUpdate;
use wirecatch_engine::Progress;
use wirecatch_protocol::app::AppSettingsPatch;
use wirecatch_shell::{api, host};

use super::MainWindow;
use crate::app_update::{Stage, Updates};
use crate::backend::{Backend, act};
use crate::ui::Callout;

impl MainWindow {
    /// A newer version of the app, until it is installed or put aside, with its download and why
    /// it failed (D111.4).
    pub(super) fn update_banner(cx: &App) -> Option<AnyElement> {
        let updates = crate::app_update::entity(cx);
        let read = updates.read(cx);
        let put_aside =
            |update: &AppUpdate| read.dismissed.as_deref() == Some(update.version.as_str());
        let callout = match read.stage.clone() {
            Stage::Available(update) if !put_aside(&update) => update_available(&update, &updates),
            Stage::Downloading {
                update, progress, ..
            } => update_downloading(&update, &progress),
            Stage::InstallFailed { update, message } if !put_aside(&update) => {
                update_failed(update, message, &updates)
            }
            _ => return None,
        };
        Some(div().px_5().pb_2().child(callout).into_any_element())
    }

    /// The banner while the list could not be saved: Try again saves again, Dismiss hides it.
    pub(super) fn storage_banner(&self, cx: &mut Context<'_, Self>) -> Option<AnyElement> {
        let problem = self.library.read(cx).storage_problem.clone()?;
        let app = host::CHANNEL.name();
        let library = self.library.clone();
        Some(
            div()
                .px_5()
                .pb_2()
                .child(
                    Callout::danger(format!(
                        "{problem} Recent changes may be lost when {app} closes."
                    ))
                    .title(format!("{app} could not save its download list"))
                    .action(
                        Button::new("storage-retry")
                            .small()
                            .outline()
                            .label("Try again")
                            .on_click({
                                let library = library.clone();
                                move |_, window, cx| {
                                    library.update(cx, |library, cx| {
                                        library.storage_problem = None;
                                        cx.notify();
                                    });
                                    // An empty change goes through the save like any other.
                                    act(window.window_handle(), cx, |manager| {
                                        api::change_settings(manager, AppSettingsPatch::default())
                                    });
                                }
                            }),
                    )
                    .action(
                        Button::new("storage-dismiss")
                            .small()
                            .ghost()
                            .label("Dismiss")
                            .on_click(move |_, _, cx| {
                                library.update(cx, |library, cx| {
                                    library.storage_problem = None;
                                    cx.notify();
                                });
                            }),
                    ),
                )
                .into_any_element(),
        )
    }

    /// This start's notices, once, as notifications in the window; a damaged list set aside
    /// offers to show the old one in its folder.
    pub(super) fn tell_startup_notices(&mut self, window: &mut Window, cx: &mut Context<'_, Self>) {
        let notices = self.library.update(cx, |library, _| {
            std::mem::take(&mut library.startup_notices)
        });
        for notice in notices {
            let mut shown =
                Notification::warning(notice.message.clone()).title(notice.title.clone());
            if notice.set_aside {
                shown = shown.action(|_, _, _| {
                    Button::new("show-set-aside")
                        .small()
                        .label("Show the old list")
                        .on_click(|_, window, cx| show_set_aside(window, cx))
                });
            }
            window.push_notification(shown, cx);
        }
    }
}

fn update_available(update: &AppUpdate, updates: &Entity<Updates>) -> Callout {
    let app = host::CHANNEL.name();
    let installs = update.download.is_some();
    let page = update.page.clone();
    let chosen = update.clone();
    Callout::info(if installs {
        format!("{app} restarts to finish installing it.")
    } else {
        "Install it with your package manager.".to_owned()
    })
    .title(format!("{app} {} is available", update.version))
    .actions_beside()
    .action(later(update, updates))
    .when(installs, |callout| {
        callout.action(
            Button::new("update-notes")
                .small()
                .ghost()
                .label("What's new")
                .on_click(move |_, _, cx| cx.open_url(page.as_str())),
        )
    })
    .action(
        Button::new("update-install")
            .small()
            .primary()
            .label(crate::app_update::install_label(update))
            .on_click(move |_, _, cx| crate::app_update::install(&chosen, cx)),
    )
}

fn update_downloading(update: &AppUpdate, progress: &Progress) -> Callout {
    let app = host::CHANNEL.name();
    let done = crate::app_update::percent(progress);
    Callout::info(done.map_or_else(
        || format!("Downloading {app} {}", update.version),
        |done| format!("Downloading {app} {}, {done:.0}%", update.version),
    ))
    .under(
        ProgressBar::new("update-progress")
            .loading(done.is_none())
            .value(done.unwrap_or_default()),
    )
    .actions_beside()
    .action(
        Button::new("update-cancel")
            .small()
            .outline()
            .label("Cancel")
            .on_click(|_, _, cx| crate::app_update::cancel(cx)),
    )
}

fn update_failed(update: AppUpdate, message: String, updates: &Entity<Updates>) -> Callout {
    let app = host::CHANNEL.name();
    let later = later(&update, updates);
    Callout::danger(message)
        .title(format!("{app} {} could not be installed", update.version))
        .actions_beside()
        .action(later)
        .action(
            Button::new("update-retry")
                .small()
                .outline()
                .label("Try again")
                .on_click(move |_, _, cx| crate::app_update::install(&update, cx)),
        )
}

/// Puts the banner aside for this version.
fn later(update: &AppUpdate, updates: &Entity<Updates>) -> Button {
    let version = update.version.clone();
    let updates = updates.clone();
    Button::new("update-later")
        .small()
        .ghost()
        .label("Later")
        .on_click(move |_, _, cx| {
            updates.update(cx, |updates, cx| {
                updates.dismissed = Some(version.clone());
                cx.notify();
            });
        })
}

/// Shows the download list set aside at this start in its folder.
fn show_set_aside(window: &mut Window, cx: &mut App) {
    let found = Backend::run(cx, |manager| async move {
        manager.set_aside_list().map(std::path::Path::to_path_buf)
    });
    window
        .spawn(cx, async move |cx| {
            let Ok(Ok(Some(path))) = found.await else {
                return;
            };
            _ = cx.update(|_, cx| cx.reveal_path(&path));
        })
        .detach();
}

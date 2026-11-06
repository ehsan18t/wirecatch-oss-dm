//! What the window must tell before anything else, above the list so it covers nothing: that the
//! download list could not be saved, until it saves again or is dismissed, and this start's
//! notices, until dismissed (D190).

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::progress::Progress as ProgressBar;
use gpui_kit::component::{Sizable as _, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, Context, Entity, InteractiveElement as _, IntoElement, ParentElement as _,
    StatefulInteractiveElement as _, Styled as _, Window, div, relative,
};
use wirecatch_core::AppUpdate;
use wirecatch_engine::Progress;
use wirecatch_shell::host;

use super::MainWindow;
use crate::app_update::{Stage, Updates};
use crate::backend::{Backend, act};
use crate::ui::Callout;

impl MainWindow {
    /// Every banner above the list, in the order they stack (D190). Past half the height they scroll, so the list or empty page keeps the rest.
    pub(super) fn banner_stack(&self, cx: &mut Context<'_, Self>) -> Option<AnyElement> {
        let mut banners: Vec<_> = self.storage_banner(cx).into_iter().collect();
        banners.extend(self.startup_banners(cx));
        banners.extend(Self::update_banner(cx));
        (!banners.is_empty()).then(|| {
            v_flex()
                .id("banners")
                .flex_shrink_0()
                .max_h(relative(0.5))
                .overflow_y_scroll()
                .children(banners)
                .into_any_element()
        })
    }

    /// A newer version of the app, until it is installed or put aside, with its download and why
    /// it failed (D111.4).
    fn update_banner(cx: &App) -> Option<AnyElement> {
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

    /// The banner while the list could not be saved. It goes by itself once saving recovers; Try again retries at once, and Dismiss only hides it (D167).
    fn storage_banner(&self, cx: &mut Context<'_, Self>) -> Option<AnyElement> {
        let read = self.library.read(cx);
        let problem = read.storage_problem.clone()?;
        let retrying = read.storage_retrying;
        let app = host::CHANNEL.name();
        let library = self.library.clone();
        Some(
            div()
                .px_5()
                .pb_2()
                .child(
                    Callout::danger(problem)
                        .title(format!("{app} could not save its download list"))
                        .action(
                            Button::new("storage-retry")
                                .small()
                                .outline()
                                .label("Try again")
                                .loading(retrying)
                                .on_click({
                                    let library = library.clone();
                                    move |_, window, cx| {
                                        library.update(cx, |library, cx| {
                                            library.storage_retrying = true;
                                            cx.notify();
                                        });
                                        act(window.window_handle(), cx, |manager| {
                                            manager.retry_saving();
                                            Ok(())
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

    /// This start's notices, one banner each, in the core's order; a damaged list set aside
    /// offers to show the old one in its folder (D190).
    fn startup_banners(&self, cx: &App) -> Vec<AnyElement> {
        let library = &self.library;
        library
            .read(cx)
            .startup_notices
            .iter()
            .enumerate()
            .map(|(index, notice)| {
                let library = library.clone();
                div()
                    .px_5()
                    .pb_2()
                    .child(
                        Callout::warning(notice.message.clone())
                            .title(notice.title.clone())
                            .when(notice.set_aside, |callout| {
                                callout.action(
                                    Button::new("show-set-aside")
                                        .small()
                                        .outline()
                                        .label("Show the old list")
                                        .on_click(|_, window, cx| show_set_aside(window, cx)),
                                )
                            })
                            .action(
                                Button::new(("startup-notice-dismiss", index))
                                    .small()
                                    .ghost()
                                    .label("Dismiss")
                                    .on_click(move |_, _, cx| {
                                        library.update(cx, |library, cx| {
                                            library.dismiss_startup_notice(index);
                                            cx.notify();
                                        });
                                    }),
                            ),
                    )
                    .into_any_element()
            })
            .collect()
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
            crate::closing::may_have_closed(cx.update(|_, cx| cx.reveal_path(&path)));
        })
        .detach();
}

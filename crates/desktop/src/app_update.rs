//! Updates of the app itself (D111): a check a minute after the app starts and once a day, or
//! when the person asks, and installing the update the person chose: the core downloads and
//! checks it, the shell starts it, and the app quits once the setup can replace it.

use std::sync::Arc;
use std::time::Duration;

use gpui_kit::{App, AppContext as _, Entity, Global};
use wirecatch_core::{AppUpdate, UpdateDownload};
use wirecatch_engine::{CancellationToken, Progress};
use wirecatch_shell::{api, host, updates};

use wirecatch_shell::host::Unopened;

use crate::backend::Backend;
use crate::library::{Library, SettingsChanged};
use crate::lifecycle;

/// How long after the app starts it first looks, so a start is never slowed by a check.
const FIRST_CHECK: Duration = Duration::from_mins(1);
/// How often it looks again while the app runs.
const CHECK_EVERY: Duration = Duration::from_hours(24);
/// How often the download's progress is shown.
const PROGRESS_EVERY: Duration = Duration::from_millis(250);

/// Where the app's update stands.
#[derive(Debug, Clone, Default)]
pub enum Stage {
    #[default]
    Unchecked,
    Checking,
    Current,
    Available(AppUpdate),
    Downloading {
        update: AppUpdate,
        progress: Arc<Progress>,
        cancel: CancellationToken,
    },
    /// Started; the app runs on until the setup has its approval, then quits for it.
    Installing,
    /// The check the person asked for failed.
    Failed(String),
    /// The update the person chose could not be downloaded or started.
    InstallFailed {
        update: AppUpdate,
        message: String,
    },
}

/// The update's stage, shown in Settings and over the downloads.
pub struct Updates {
    pub stage: Stage,
    /// The person put the banner aside for this version.
    pub dismissed: Option<String>,
}

struct Global_(Entity<Updates>);

impl Global for Global_ {}

/// Why a build does not update itself (D111.5).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NoUpdates {
    DevBuild,
    /// Built without the update key, so not from an official release.
    Unofficial,
}

/// Why this build does not update itself; `None` for a release build with the update key.
#[must_use]
pub fn no_updates() -> Option<NoUpdates> {
    if host::DEV_BUILD {
        Some(NoUpdates::DevBuild)
    } else if wirecatch_product::UPDATER_PUBLIC_KEY.starts_with("TODO-PRODUCT-") {
        Some(NoUpdates::Unofficial)
    } else {
        None
    }
}

/// Whether this build can update itself.
#[must_use]
pub fn set_up() -> bool {
    no_updates().is_none()
}

/// Keeps the update's stage and looks for one in the background, and again when the choice of
/// pre-release versions changes.
pub fn start(library: &Entity<Library>, cx: &mut App) {
    let updates = cx.new(|_| Updates {
        stage: Stage::Unchecked,
        dismissed: None,
    });
    cx.set_global(Global_(updates));
    if !set_up() {
        return;
    }
    let mut prereleases = None;
    cx.subscribe(library, move |library, _: &SettingsChanged, cx| {
        let chosen = library
            .read(cx)
            .settings
            .as_ref()
            .map(|s| s.prerelease_updates);
        if changed(&mut prereleases, chosen) {
            check(false, cx);
        }
    })
    .detach();
    cx.spawn(async move |cx| {
        let mut wait = FIRST_CHECK;
        loop {
            cx.background_executor().timer(wait).await;
            wait = CHECK_EVERY;
            cx.update(|cx| check(true, cx));
        }
    })
    .detach();
}

/// Whether `now` differs from the last value seen; the first value is not a change.
fn changed(last: &mut Option<bool>, now: Option<bool>) -> bool {
    let was = std::mem::replace(last, now.or(*last));
    matches!((was, now), (Some(was), Some(now)) if was != now)
}

/// The update's stage, which views observe.
pub fn entity(cx: &App) -> Entity<Updates> {
    cx.global::<Global_>().0.clone()
}

fn set(stage: Stage, cx: &mut App) {
    entity(cx).update(cx, |updates, cx| {
        updates.stage = stage;
        cx.notify();
    });
}

fn stage(cx: &App) -> Stage {
    entity(cx).read(cx).stage.clone()
}

/// Looks for a newer version. A `quiet` check keeps a failure to itself and leaves an update
/// already found or downloading alone.
pub fn check(quiet: bool, cx: &mut App) {
    let busy = matches!(
        stage(cx),
        Stage::Checking | Stage::Downloading { .. } | Stage::Installing
    );
    let waiting_on_person = matches!(stage(cx), Stage::Available(_) | Stage::InstallFailed { .. });
    if busy || (quiet && waiting_on_person) {
        return;
    }
    if !set_up() {
        return;
    }
    set(Stage::Checking, cx);
    let found = Backend::run(cx, |manager| async move {
        api::check_app_update(&manager).await
    });
    cx.spawn(async move |cx| {
        let stage = match answered(found.await) {
            Ok(Some(update)) => Stage::Available(update),
            Ok(None) => Stage::Current,
            Err(message) if quiet => {
                tracing::warn!("the update check failed: {message}");
                Stage::Unchecked
            }
            Err(message) => Stage::Failed(message),
        };
        cx.update(|cx| set(stage, cx));
    })
    .detach();
}

/// The button that takes `update`, the same wherever it is offered.
#[must_use]
pub fn install_label(update: &AppUpdate) -> &'static str {
    if update.download.is_some() {
        "Install and restart"
    } else {
        "Open release page"
    }
}

/// Installs `update`: downloads and checks its installer, starts it, and quits once it can go
/// ahead; a setup refused its approval leaves the app running with the failure shown. A system
/// package, which the app cannot install, opens the release page instead.
pub fn install(update: &AppUpdate, cx: &mut App) {
    let (Some(download), Some(kind)) = (update.download.clone(), updates::install_kind()) else {
        cx.open_url(update.page.as_str());
        return;
    };
    let progress = Arc::new(Progress::default());
    let cancel = CancellationToken::new();
    set(
        Stage::Downloading {
            update: update.clone(),
            progress: Arc::clone(&progress),
            cancel: cancel.clone(),
        },
        cx,
    );
    let fetched = fetch(&download, &progress, &cancel, cx);
    let update = update.clone();
    cx.spawn(async move |cx| {
        let shown = cx.spawn(async move |cx| {
            loop {
                cx.background_executor().timer(PROGRESS_EVERY).await;
                cx.update(|cx| entity(cx).update(cx, |_, cx| cx.notify()));
            }
        });
        let result = answered(fetched.await);
        drop(shown);
        if cancel.is_cancelled() {
            cx.update(|cx| set(Stage::Available(update), cx));
            return;
        }
        let path = match result {
            Ok(path) => path,
            Err(message) => {
                cx.update(|cx| set(Stage::InstallFailed { update, message }, cx));
                return;
            }
        };
        cx.update(|cx| set(Stage::Installing, cx));
        // Blocks until the setup has its approval, which waits on the person.
        let started = cx
            .background_executor()
            .spawn(async move { updates::install(&path, kind) })
            .await;
        cx.update(|cx| match started {
            Ok(()) if kind == updates::FileKind::AppImage => lifecycle::quit_and_start_again(cx),
            Ok(()) => lifecycle::quit(cx),
            Err(e) => {
                let message = e.to_string();
                set(Stage::InstallFailed { update, message }, cx);
            }
        });
    })
    .detach();
}

/// Stops the download of an update, which goes back to being offered.
pub fn cancel(cx: &App) {
    if let Stage::Downloading { cancel, .. } = stage(cx) {
        cancel.cancel();
    }
}

/// How much of the download is done, in percent, once its size is known.
#[must_use]
pub fn percent(progress: &Progress) -> Option<f32> {
    // In tenths of a percent, as whole numbers.
    progress
        .total()
        .filter(|total| *total > 0)
        .and_then(|total| u16::try_from(progress.done().min(total) * 1000 / total).ok())
        .map(|tenths| f32::from(tenths) / 10.)
}

fn fetch(
    download: &UpdateDownload,
    progress: &Arc<Progress>,
    cancel: &CancellationToken,
    cx: &App,
) -> tokio::task::JoinHandle<
    Result<Result<std::path::PathBuf, wirecatch_protocol::wire::ProtocolError>, Unopened>,
> {
    let (download, progress, cancel) = (download.clone(), Arc::clone(progress), cancel.clone());
    Backend::run(cx, move |manager| async move {
        api::fetch_app_update(&manager, &download, progress, cancel).await
    })
}

/// The answer of a manager call, or the message of why there is none.
fn answered<T>(
    done: Result<
        Result<Result<T, wirecatch_protocol::wire::ProtocolError>, Unopened>,
        tokio::task::JoinError,
    >,
) -> Result<T, String> {
    match done {
        Ok(Ok(answer)) => answer.map_err(|e| e.message),
        Ok(Err(unopened)) => Err(unopened.reason.message),
        Err(e) => Err(format!("The update stopped unexpectedly ({e}).")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_later_different_choice_is_a_change() {
        let mut last = None;
        assert!(!changed(&mut last, None), "the settings have not arrived");
        assert!(!changed(&mut last, Some(false)), "the first value");
        assert!(!changed(&mut last, Some(false)), "the same value");
        assert!(changed(&mut last, Some(true)));
        assert!(changed(&mut last, Some(false)));
    }

    #[test]
    fn a_download_of_unknown_size_has_no_percent() {
        assert_eq!(percent(&Progress::default()), None);
    }
}

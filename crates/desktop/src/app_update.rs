//! Updates of the app itself (D111): a check a minute after the app starts and once a day, or
//! when the person asks, and installing the update the person chose: the core downloads and
//! checks it, the shell starts it, and the app quits so it can be replaced.

use std::sync::Arc;
use std::time::Duration;

use gpui_kit::{App, AppContext as _, Entity, Global};
use wirecatch_core::{AppUpdate, UpdateDownload};
use wirecatch_engine::{CancellationToken, Progress};
use wirecatch_shell::{api, host, updates};

use wirecatch_shell::host::Unopened;

use crate::backend::Backend;
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
    },
    /// Started; the app is quitting so the installer can replace it.
    Installing,
    Failed(String),
}

/// The update's stage, shown in Settings and over the downloads.
pub struct Updates {
    pub stage: Stage,
    /// The person put the banner aside for this version.
    pub dismissed: Option<String>,
}

struct Global_(Entity<Updates>);

impl Global for Global_ {}

/// Whether this build can update itself: a release build with the update key.
#[must_use]
pub fn set_up() -> bool {
    !host::DEV_BUILD && !wirecatch_product::UPDATER_PUBLIC_KEY.starts_with("TODO-PRODUCT-")
}

/// Keeps the update's stage and looks for one in the background.
pub fn start(cx: &mut App) {
    let updates = cx.new(|_| Updates {
        stage: Stage::Unchecked,
        dismissed: None,
    });
    cx.set_global(Global_(updates));
    if !set_up() {
        return;
    }
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

/// The update's stage, which views observe.
pub fn entity(cx: &App) -> Option<Entity<Updates>> {
    cx.try_global::<Global_>().map(|global| global.0.clone())
}

fn set(stage: Stage, cx: &mut App) {
    if let Some(updates) = entity(cx) {
        updates.update(cx, |updates, cx| {
            updates.stage = stage;
            cx.notify();
        });
    }
}

fn stage(cx: &App) -> Stage {
    entity(cx)
        .map(|updates| updates.read(cx).stage.clone())
        .unwrap_or_default()
}

/// Looks for a newer version. A `quiet` check keeps a failure to itself and leaves an update
/// already found or downloading alone.
pub fn check(quiet: bool, cx: &mut App) {
    let busy = matches!(
        stage(cx),
        Stage::Checking | Stage::Downloading { .. } | Stage::Installing
    );
    if busy || (quiet && matches!(stage(cx), Stage::Available(_))) {
        return;
    }
    if !set_up() {
        set(
            Stage::Failed(format!(
                "This build of {} does not update itself. Download a newer version from its website.",
                host::CHANNEL.name()
            )),
            cx,
        );
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

/// Installs `update`: downloads and checks its installer, starts it and quits. A system
/// package, which the app cannot install, opens the release page instead.
pub fn install(update: &AppUpdate, cx: &mut App) {
    let (Some(download), Some(kind)) = (update.download.clone(), updates::install_kind()) else {
        cx.open_url(&update.page);
        return;
    };
    let progress = Arc::new(Progress::default());
    set(
        Stage::Downloading {
            update: update.clone(),
            progress: Arc::clone(&progress),
        },
        cx,
    );
    let fetched = fetch(&download, &progress, cx);
    cx.spawn(async move |cx| {
        let shown = cx.spawn(async move |cx| {
            loop {
                cx.background_executor().timer(PROGRESS_EVERY).await;
                cx.update(|cx| {
                    if let Some(updates) = entity(cx) {
                        updates.update(cx, |_, cx| cx.notify());
                    }
                });
            }
        });
        let result = answered(fetched.await);
        drop(shown);
        cx.update(
            |cx| match result.and_then(|path| updates::install(&path, kind)) {
                Ok(()) => {
                    set(Stage::Installing, cx);
                    lifecycle::quit(cx);
                }
                Err(message) => set(Stage::Failed(message), cx),
            },
        );
    })
    .detach();
}

fn fetch(
    download: &UpdateDownload,
    progress: &Arc<Progress>,
    cx: &App,
) -> tokio::task::JoinHandle<
    Result<Result<std::path::PathBuf, wirecatch_protocol::wire::ProtocolError>, Unopened>,
> {
    let (download, progress) = (download.clone(), Arc::clone(progress));
    Backend::run(cx, move |manager| async move {
        api::fetch_app_update(&manager, &download, progress, CancellationToken::new()).await
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

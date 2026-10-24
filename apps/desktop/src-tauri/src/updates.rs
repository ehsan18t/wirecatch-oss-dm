//! Updates (D19), switched on only when `plugins.updater` is configured with a public key.
//!
//! Shortly after start, and then once a day, the app asks the configured endpoint whether a newer
//! version exists. If so it downloads it in the background, where the updater checks its
//! signature against the configured public key, and says so in a notification. The update
//! installs when the person quits Wirecatch, so it never interrupts a download.

use std::sync::{Mutex, PoisonError};
use std::time::Duration;

use tauri::{AppHandle, Manager as _, Runtime};
use tauri_plugin_updater::{Update, UpdaterExt as _};

use crate::notices::Notice;

/// How long after start the first check waits, so it never competes with startup.
pub const FIRST_CHECK_AFTER: Duration = Duration::from_secs(60);

/// How often the app checks again while it keeps running.
pub const CHECK_EVERY: Duration = Duration::from_hours(24);

/// A downloaded, verified update waiting to install when the app quits.
#[derive(Default)]
pub struct PendingUpdate(Mutex<Option<(Update, Vec<u8>)>>);

impl PendingUpdate {
    fn lock(&self) -> std::sync::MutexGuard<'_, Option<(Update, Vec<u8>)>> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The version waiting, if any.
    pub fn version(&self) -> Option<String> {
        self.lock().as_ref().map(|(u, _)| u.version.clone())
    }
}

/// Asks for a newer version and, if there is one, downloads it (the updater refuses it unless
/// its signature matches the configured public key) and keeps it for [`install_pending`].
/// Returns the new version.
///
/// # Errors
///
/// The updater is not set up, the update server cannot be reached or answers with something
/// that is not an update manifest, or the update cannot be downloaded or does not match its
/// signature.
pub async fn check_and_download<R: Runtime>(app: &AppHandle<R>) -> Result<Option<String>, String> {
    let updater = app.updater().map_err(|e| e.to_string())?;
    let Some(update) = updater.check().await.map_err(|e| e.to_string())? else {
        return Ok(None);
    };
    let bytes = update
        .download(|_, _| {}, || {})
        .await
        .map_err(|e| format!("Could not download Wirecatch {}: {e}", update.version))?;
    let version = update.version.clone();
    if let Some(pending) = app.try_state::<PendingUpdate>() {
        *pending.lock() = Some((update, bytes));
    }
    Ok(Some(version))
}

/// The notification that an update is downloaded, naming the running app. It is news, so it
/// waits for the person to finish what Do Not Disturb is for (D64).
fn update_ready_notice(version: &str) -> Notice {
    let name = crate::host::CHANNEL.name();
    Notice {
        important: false,
        ..crate::system_notices::plain(
            format!("{name} {version} is ready"),
            format!("It installs the next time you quit {name}."),
        )
    }
}

/// Checks now and then for as long as the app runs; see the module documentation.
pub async fn keep_checking<R: Runtime>(app: AppHandle<R>) {
    tokio::time::sleep(FIRST_CHECK_AFTER).await;
    loop {
        let waiting = app.try_state::<PendingUpdate>().and_then(|p| p.version());
        if waiting.is_none() {
            match check_and_download(&app).await {
                Ok(Some(version)) => {
                    crate::system_notices::show(&app, update_ready_notice(&version));
                }
                Ok(None) => {}
                Err(e) => tracing::warn!("could not check for updates: {e}"),
            }
        }
        tokio::time::sleep(CHECK_EVERY).await;
    }
}

/// Installs a downloaded update. Called at the very end of quitting, once every download has
/// stopped; on Windows the installer then replaces the app.
pub fn install_pending<R: Runtime>(app: &AppHandle<R>) {
    let Some(pending) = app.try_state::<PendingUpdate>() else {
        return;
    };
    let Some((update, bytes)) = pending.lock().take() else {
        return;
    };
    if let Err(e) = update.install(bytes) {
        tracing::warn!("could not install Wirecatch {}: {e}", update.version);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_update_notice_names_the_running_app() {
        let name = crate::host::CHANNEL.name();
        let notice = update_ready_notice("1.2.3");
        assert_eq!(
            (notice.title, notice.body),
            (
                format!("{name} 1.2.3 is ready"),
                format!("It installs the next time you quit {name}.")
            )
        );
        assert!(!notice.important, "news waits out Do Not Disturb");
    }
}

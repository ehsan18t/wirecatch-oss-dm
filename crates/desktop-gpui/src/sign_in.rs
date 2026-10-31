//! Starts the app at sign-in, in the background, while the setting says so
//! (`wirecatch_shell::autostart`).

use gpui_kit::App;
use tokio::sync::broadcast::error::RecvError;
use wirecatch_core::Manager;
use wirecatch_protocol::app::UiEvent;
use wirecatch_shell::autostart::{self, Entry};
use wirecatch_shell::host;

use crate::backend::Backend;

/// Follows the setting once the manager opens, until it stops.
pub fn start(cx: &App) {
    drop(Backend::run(cx, follow));
}

async fn follow(manager: Manager) {
    let mut events = manager.subscribe();
    let mut wanted = manager.get_settings().start_with_system;
    sync(&manager, wanted, false).await;
    loop {
        let now = match events.recv().await {
            Ok(UiEvent::SettingsChanged(settings)) => settings.start_with_system,
            Ok(_) => continue,
            Err(RecvError::Lagged(_)) => manager.get_settings().start_with_system,
            Err(RecvError::Closed) => return,
        };
        if now != wanted {
            wanted = now;
            sync(&manager, wanted, wanted).await;
        }
    }
}

/// Makes the entry match `wanted`, off the async threads: it touches the registry or the disk.
async fn sync(manager: &Manager, wanted: bool, just_turned_on: bool) {
    let manager = manager.clone();
    let synced = tokio::task::spawn_blocking(move || match autostart::program() {
        Ok(program) => Entry {
            name: host::CHANNEL.name(),
            program: &program,
        }
        .sync(&manager, wanted, just_turned_on),
        Err(e) => tracing::warn!("could not find the program to start at sign-in: {e}"),
    })
    .await;
    if let Err(e) = synced {
        tracing::error!("following the start-at-sign-in setting stopped unexpectedly: {e}");
    }
}

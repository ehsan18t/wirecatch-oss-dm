//! What the windows show of the download manager: the downloads, the queues and the settings,
//! loaded once and kept current from the manager's events, in the order the manager lists them.

use gpui_kit::{AppContext as _, Context, Entity, EventEmitter};
use tokio::sync::broadcast::error::RecvError;
use wirecatch_protocol::QueueId;
use wirecatch_protocol::app::{AppSettings, DownloadSummary, Queue, UiEvent};
use wirecatch_protocol::wire::ProtocolError;
use wirecatch_shell::api;

use crate::backend::Backend;

/// Whether the manager has answered yet.
pub enum Loaded {
    Loading,
    Ready,
    /// It could not open; the reason is worded for a person.
    Failed(ProtocolError),
}

/// Everything the main window lists.
pub struct Library {
    pub loaded: Loaded,
    pub downloads: Vec<DownloadSummary>,
    pub queues: Vec<Queue>,
    pub settings: Option<AppSettings>,
}

/// The settings changed: the theme may have.
pub struct SettingsChanged;

impl EventEmitter<SettingsChanged> for Library {}

impl Library {
    /// Starts loading from the manager.
    pub fn new(cx: &mut gpui_kit::App) -> Entity<Self> {
        cx.new(|cx| {
            Self::load(cx);
            Self {
                loaded: Loaded::Loading,
                downloads: Vec::new(),
                queues: Vec::new(),
                settings: None,
            }
        })
    }

    /// Reads everything again, then follows the manager's events. Also the way back after
    /// events were missed.
    pub fn load(cx: &mut Context<'_, Self>) {
        let snapshot = Backend::run(cx, |manager| async move {
            let events = manager.subscribe();
            let downloads = api::list_downloads(&manager);
            let queues = api::list_queues(&manager);
            let settings = api::get_settings(&manager);
            (events, downloads, queues, settings)
        });
        // Ends once the library is gone, or when it reloads after missing events.
        cx.spawn(async move |this, cx| {
            let Ok(opened) = snapshot.await else {
                tracing::error!("reading the download list stopped unexpectedly");
                return;
            };
            let mut events = match opened {
                Ok((events, downloads, queues, settings)) => {
                    let updated = this.update(cx, |library, cx| {
                        library.downloads = downloads;
                        library.queues = queues;
                        library.settings = Some(settings);
                        library.loaded = Loaded::Ready;
                        cx.emit(SettingsChanged);
                        cx.notify();
                    });
                    if updated.is_err() {
                        return;
                    }
                    events
                }
                Err(unopened) => {
                    _ = this.update(cx, |library, cx| {
                        library.loaded = Loaded::Failed(unopened.reason);
                        cx.notify();
                    });
                    return;
                }
            };
            loop {
                let next = events.recv().await;
                let stops = next.is_err();
                let alive = this.update(cx, |library, cx| match next {
                    Ok(event) => library.apply(event, cx),
                    Err(RecvError::Lagged(_)) => Self::load(cx),
                    Err(RecvError::Closed) => {}
                });
                if alive.is_err() || stops {
                    return;
                }
            }
        })
        .detach();
    }

    fn apply(&mut self, event: UiEvent, cx: &mut Context<'_, Self>) {
        match event {
            UiEvent::DownloadsChanged(rows) => {
                for row in rows {
                    match self.downloads.iter_mut().find(|known| known.id == row.id) {
                        Some(known) => *known = row,
                        None => self.downloads.push(row),
                    }
                }
            }
            UiEvent::DownloadsRemoved(ids) => self.downloads.retain(|row| !ids.contains(&row.id)),
            UiEvent::QueuesChanged(queues) => self.queues = queues,
            UiEvent::SettingsChanged(settings) => {
                self.settings = Some(*settings);
                cx.emit(SettingsChanged);
            }
            _ => return,
        }
        cx.notify();
    }

    pub fn queue(&self, id: QueueId) -> Option<&Queue> {
        self.queues.iter().find(|queue| queue.id == id)
    }
}

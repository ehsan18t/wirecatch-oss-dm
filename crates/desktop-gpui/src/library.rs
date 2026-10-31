//! What the windows show of the download manager: the downloads, the queues, the proxy servers,
//! the add-ons and the settings,
//! loaded once and kept current from the manager's events, in the order the manager lists them.

use gpui_kit::{AppContext as _, Context, Entity, EventEmitter};
use tokio::sync::broadcast::error::RecvError;
use wirecatch_protocol::app::{
    AddonInfo, AppSettings, DownloadSummary, Queue, StartupNotice, UiEvent,
};
use wirecatch_protocol::desktop::Reveal;
use wirecatch_protocol::proxy::ProxyServer;
use wirecatch_protocol::wire::ProtocolError;
use wirecatch_protocol::{DownloadId, QuestionId, QueueId};
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
    pub proxies: Vec<ProxyServer>,
    pub addons: Vec<AddonInfo>,
    pub settings: Option<AppSettings>,
    /// Why the download list could not be saved, until it saves again or is dismissed.
    pub storage_problem: Option<String>,
    /// What the person must know about this start, such as a damaged list set aside.
    pub startup_notices: Vec<StartupNotice>,
    /// The questions the core asks about browser downloads, oldest first.
    pub open_questions: Vec<QuestionId>,
}

/// Everything read at once, with the events that follow it.
struct Snapshot {
    events: tokio::sync::broadcast::Receiver<UiEvent>,
    downloads: Vec<DownloadSummary>,
    queues: Vec<Queue>,
    proxies: Vec<ProxyServer>,
    addons: Vec<AddonInfo>,
    settings: AppSettings,
    startup_notices: Vec<StartupNotice>,
    open_questions: Vec<QuestionId>,
}

/// The settings changed: the theme may have.
pub struct SettingsChanged;

impl EventEmitter<SettingsChanged> for Library {}

/// Something outside the window asked to see the app: the browser, a second launch (D103), or a
/// notification (D98).
pub enum Asked {
    Window,
    Links(Vec<String>),
    BrowserSettings,
    Reveal(Reveal),
}

impl EventEmitter<Asked> for Library {}

impl Library {
    /// Starts loading from the manager.
    pub fn new(cx: &mut gpui_kit::App) -> Entity<Self> {
        cx.new(|cx| {
            Self::load(cx);
            Self {
                loaded: Loaded::Loading,
                downloads: Vec::new(),
                queues: Vec::new(),
                proxies: Vec::new(),
                addons: Vec::new(),
                settings: None,
                storage_problem: None,
                startup_notices: Vec::new(),
                open_questions: Vec::new(),
            }
        })
    }

    /// Reads everything again, then follows the manager's events. Also the way back after
    /// events were missed.
    pub fn load(cx: &mut Context<'_, Self>) {
        let snapshot = Backend::run(cx, |manager| async move {
            let events = manager.subscribe();
            Snapshot {
                downloads: api::list_downloads(&manager),
                queues: api::list_queues(&manager),
                proxies: api::list_proxy_servers(&manager),
                addons: api::list_addons(&manager),
                settings: api::get_settings(&manager),
                startup_notices: manager.startup_notices().to_vec(),
                open_questions: wirecatch_shell::download_windows::open_questions(&manager),
                events,
            }
        });
        // Ends once the library is gone, or when it reloads after missing events.
        cx.spawn(async move |this, cx| {
            let Ok(opened) = snapshot.await else {
                tracing::error!("reading the download list stopped unexpectedly");
                return;
            };
            let mut events = match opened {
                Ok(snapshot) => {
                    let Snapshot {
                        events,
                        downloads,
                        queues,
                        proxies,
                        addons,
                        settings,
                        startup_notices,
                        open_questions,
                    } = snapshot;
                    let updated = this.update(cx, |library, cx| {
                        library.downloads = downloads;
                        library.queues = queues;
                        library.proxies = proxies;
                        library.addons = addons;
                        library.settings = Some(settings);
                        library.startup_notices = startup_notices;
                        library.loaded = Loaded::Ready;
                        cx.emit(SettingsChanged);
                        cx.notify();
                        crate::download_windows::restore(&open_questions, cx);
                        library.open_questions = open_questions;
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
        crate::download_windows::follow(&event, cx);
        if let Some(asked) = asked_question(&event)
            && !self.open_questions.contains(&asked)
        {
            self.open_questions.push(asked);
            cx.notify();
        }
        match event {
            UiEvent::ConfirmationClosed { id } => {
                self.open_questions.retain(|open| *open != id);
            }
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
            UiEvent::ProxyServersChanged(proxies) => self.proxies = proxies,
            UiEvent::AddonsChanged(addons) => self.addons = addons,
            UiEvent::SettingsChanged(settings) => {
                self.settings = Some(*settings);
                cx.emit(SettingsChanged);
            }
            UiEvent::WindowRequested => return cx.emit(Asked::Window),
            UiEvent::LinksRequested(links) => return cx.emit(Asked::Links(links)),
            UiEvent::BrowserSettingsRequested => return cx.emit(Asked::BrowserSettings),
            UiEvent::StorageFailed(error) => {
                crate::notices::storage_failed(&error.message, cx);
                self.storage_problem = Some(error.message);
            }
            _ => return,
        }
        cx.notify();
    }

    /// Puts `ids_in_order` in that order among the places they hold, once the manager took the
    /// same order: no event carries it.
    pub fn reorder(&mut self, ids_in_order: &[DownloadId]) {
        let places: Vec<usize> = self
            .downloads
            .iter()
            .enumerate()
            .filter(|(_, row)| ids_in_order.contains(&row.id))
            .map(|(index, _)| index)
            .collect();
        let rows: Vec<DownloadSummary> = ids_in_order
            .iter()
            .filter_map(|id| self.downloads.iter().find(|row| row.id == *id).cloned())
            .collect();
        if rows.len() != places.len() {
            return;
        }
        for (place, row) in places.into_iter().zip(rows) {
            self.downloads[place] = row;
        }
    }

    pub fn queue(&self, id: QueueId) -> Option<&Queue> {
        self.queues.iter().find(|queue| queue.id == id)
    }
}

/// The question `event` asks, or moves on, if it asks one.
fn asked_question(event: &UiEvent) -> Option<QuestionId> {
    match event {
        UiEvent::ConfirmationRequested(capture) => Some(capture.id),
        UiEvent::MediaAnalysing { id, .. } | UiEvent::AddonRequired { id, .. } => Some(*id),
        UiEvent::MediaChoiceRequested(choice) => Some(choice.id),
        UiEvent::FileExistsRequested(question) => Some(question.id),
        _ => None,
    }
}

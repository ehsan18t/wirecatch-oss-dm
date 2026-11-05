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
use crate::rows::Rows;

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
    pub downloads: Rows,
    pub queues: Vec<Queue>,
    pub proxies: Vec<ProxyServer>,
    pub addons: Vec<AddonInfo>,
    pub settings: Option<AppSettings>,
    pub revisions: Revisions,
    order: OrderReading,
    /// Why the download list could not be saved, until it saves again or is dismissed.
    pub storage_problem: Option<String>,
    /// Try again was pressed and its answer has not come.
    pub storage_retrying: bool,
    /// What the person must know about this start, such as a damaged list set aside.
    pub startup_notices: Vec<StartupNotice>,
    notices_kept: bool,
    /// The questions the core asks about browser downloads, oldest first.
    pub open_questions: Vec<QuestionId>,
}

/// Reading the manager's order, one read at a time.
#[derive(Default)]
struct OrderReading {
    running: bool,
    /// The order changed during the read, so what it answers may be out of date.
    stale: bool,
}

impl OrderReading {
    /// Whether a read may start; with one running, that read's answer is out of date.
    fn begin(&mut self) -> bool {
        if self.running {
            self.stale = true;
            return false;
        }
        self.running = true;
        true
    }

    /// The order changed here without a read being asked for.
    fn changed(&mut self) {
        self.stale = self.running;
    }

    /// Ends the read. Whether its answer holds; otherwise the order must be read again.
    fn end(&mut self) -> bool {
        self.running = false;
        !std::mem::take(&mut self.stale)
    }
}

/// Moves as each part changes. Every change notifies, up to ten times a second while downloads
/// run, so an observer of one part compares its revision before redoing work.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub struct Revisions {
    pub queues: u64,
    pub proxies: u64,
    pub settings: u64,
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
            Self::empty()
        })
    }

    pub(crate) fn empty() -> Self {
        Self {
            loaded: Loaded::Loading,
            downloads: Rows::default(),
            queues: Vec::new(),
            proxies: Vec::new(),
            addons: Vec::new(),
            settings: None,
            revisions: Revisions::default(),
            order: OrderReading::default(),
            storage_problem: None,
            storage_retrying: false,
            startup_notices: Vec::new(),
            notices_kept: false,
            open_questions: Vec::new(),
        }
    }

    /// Keeps what this start must tell, once: a reload after missed events reads the same notices.
    fn keep_startup_notices(&mut self, notices: Vec<StartupNotice>) {
        if !std::mem::replace(&mut self.notices_kept, true) {
            self.startup_notices = notices;
        }
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
                        library.downloads.replace(downloads);
                        library.queues = queues;
                        library.proxies = proxies;
                        library.revisions.queues += 1;
                        library.revisions.proxies += 1;
                        library.revisions.settings += 1;
                        library.addons = addons;
                        crate::text::set_clock(settings.clock);
                        library.settings = Some(settings);
                        library.keep_startup_notices(startup_notices);
                        library.loaded = Loaded::Ready;
                        cx.emit(SettingsChanged);
                        cx.notify();
                        crate::download_windows::finished::reload(&library.downloads, cx);
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
                    Err(RecvError::Lagged(_)) => {
                        // The lost event may be a retry's answer.
                        library.storage_retrying = false;
                        Self::load(cx);
                    }
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
        // Opening a window draws it, and its first draw reads this library: not during this update.
        let opening = event.clone();
        cx.defer(move |cx| crate::download_windows::follow(&opening, cx));
        crate::download_windows::finished::follow(&event, cx);
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
                let mut moved = false;
                for row in rows {
                    moved |= self.downloads.put(row);
                }
                if moved {
                    self.read_order(cx);
                }
            }
            UiEvent::DownloadsRemoved(ids) => self.downloads.remove(&ids),
            UiEvent::QueuesChanged(queues) => {
                let same_order = self
                    .queues
                    .iter()
                    .map(|queue| queue.id)
                    .eq(queues.iter().map(|queue| queue.id));
                self.queues = queues;
                self.revisions.queues += 1;
                if !same_order {
                    self.read_order(cx);
                }
            }
            UiEvent::ProxyServersChanged(proxies) => {
                self.proxies = proxies;
                self.revisions.proxies += 1;
            }
            UiEvent::AddonsChanged(addons) => self.addons = addons,
            UiEvent::SettingsChanged(settings) => {
                crate::text::set_clock(settings.clock);
                self.settings = Some(*settings);
                self.revisions.settings += 1;
                cx.emit(SettingsChanged);
            }
            UiEvent::WindowRequested => return cx.emit(Asked::Window),
            UiEvent::LinksRequested(links) => return cx.emit(Asked::Links(links)),
            UiEvent::BrowserSettingsRequested => return cx.emit(Asked::BrowserSettings),
            UiEvent::StorageFailed(error) => {
                crate::notices::storage_failed(&error.message, cx);
                self.storage_problem = Some(error.message);
                self.storage_retrying = false;
            }
            UiEvent::StorageRecovered => {
                self.storage_problem = None;
                self.storage_retrying = false;
            }
            _ => return,
        }
        cx.notify();
    }

    /// Reads the manager's order of the downloads, which no event carries.
    fn read_order(&mut self, cx: &mut Context<'_, Self>) {
        if !self.order.begin() {
            return;
        }
        let read = Backend::run(cx, |manager| async move {
            let rows = api::list_downloads(&manager);
            rows.iter().map(|row| row.id).collect::<Vec<DownloadId>>()
        });
        cx.spawn(async move |this, cx| {
            let read = read.await;
            _ = this.update(cx, |library, cx| {
                if !library.order.end() {
                    library.read_order(cx);
                } else if let Ok(Ok(ids)) = read {
                    library.downloads.take_order(&ids);
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// Puts `ids_in_order` in that order among the places they hold, once the manager took the
    /// same order: no event carries it.
    pub fn reorder(&mut self, ids_in_order: &[DownloadId]) {
        self.order.changed();
        self.downloads.reorder(ids_in_order);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_reload_does_not_bring_back_a_notice_already_told() {
        let notice = || StartupNotice {
            title: "Title".to_owned(),
            message: "Message".to_owned(),
            set_aside: false,
        };
        let mut library = Library::empty();
        library.keep_startup_notices(vec![notice()]);
        assert_eq!(library.startup_notices.len(), 1);
        library.startup_notices.clear();
        library.keep_startup_notices(vec![notice()]);
        assert_eq!(library.startup_notices.len(), 0);
    }

    #[test]
    fn only_one_order_read_runs_and_a_second_ask_voids_its_answer() {
        let mut order = OrderReading::default();
        assert!(order.begin());
        assert!(!order.begin());
        assert!(!order.end(), "read again");
        assert!(order.begin());
        assert!(order.end(), "nothing changed during this one");
    }

    #[test]
    fn a_reorder_during_a_read_voids_its_answer() {
        let mut order = OrderReading::default();
        order.changed();
        assert!(order.begin());
        assert!(order.end(), "the reorder came before the read");
        assert!(order.begin());
        order.changed();
        assert!(!order.end());
    }
    use gpui_kit::TestAppContext;
    use wirecatch_protocol::app::DownloadStatus;
    use wirecatch_test_support::downloads::download;

    #[gpui_kit::test]
    fn a_progress_tick_moves_no_revision_and_a_new_proxy_list_moves_its_own(
        cx: &mut TestAppContext,
    ) {
        let library = cx.update(|cx| cx.new(|_| Library::empty()));
        let running = |done| DownloadSummary {
            status: DownloadStatus::Downloading,
            done,
            ..download(1)
        };
        let read = |cx: &mut TestAppContext| {
            library.read_with(cx, |library, _| {
                (library.revisions, library.downloads.revision())
            })
        };
        let apply = |event, cx: &mut TestAppContext| {
            library.update(cx, |library, cx| library.apply(event, cx));
            cx.run_until_parked();
        };
        apply(UiEvent::DownloadsChanged(vec![running(1)]), cx);
        let before = read(cx);
        apply(UiEvent::DownloadsChanged(vec![running(2)]), cx);
        assert!(read(cx) == before);
        apply(UiEvent::ProxyServersChanged(Vec::new()), cx);
        let (revisions, rows) = read(cx);
        assert_eq!(rows, before.1);
        assert_eq!(revisions.proxies, before.0.proxies + 1);
        assert_eq!(revisions.settings, before.0.settings);
    }
}

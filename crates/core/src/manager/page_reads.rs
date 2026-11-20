//! The browsers that make page reads for media reads (D282): each connection whose hello offers them, the reads waiting for their answers, and which browser a run goes through.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use base64::Engine as _;
use tokio::sync::{mpsc, oneshot};
use wirecatch_engine::Url;
use wirecatch_media::ytdlp::{
    BrowserAnswer, BrowserFailed, BrowserReader, BrowserReading, BrowserRequest, BrowserRoute,
};
use wirecatch_protocol::Secret;
use wirecatch_protocol::extension::Header;
use wirecatch_protocol::page_read::{
    MAX_PAGE_READ_LEN, PAGE_READ_TIMEOUT_MS, PageRead, PageReadCancelled, PageReadFailure,
    PageReadId,
};
use wirecatch_protocol::wire::{Call, Event};

use super::Manager;

/// Request headers the browser sets itself, or that describe yt-dlp's own connection: `fetch()` refuses or ignores them, and the browser's own go instead.
const BROWSER_SETS: [&str; 17] = [
    "accept-charset",
    "accept-encoding",
    "connection",
    "content-length",
    "cookie",
    "date",
    "dnt",
    "expect",
    "host",
    "keep-alive",
    "origin",
    "referer",
    "te",
    "trailer",
    "transfer-encoding",
    "upgrade",
    "user-agent",
];

/// The connected browsers that make page reads, and the reads they owe.
#[derive(Default)]
pub(crate) struct PageReaders {
    state: Mutex<Readers>,
}

#[derive(Default)]
struct Readers {
    next_reader: u64,
    next_read: u64,
    /// In the order they connected.
    connected: Vec<Reader>,
    waiting: HashMap<u64, Waiting>,
}

impl Readers {
    /// The browser for a download with `user_agent`: the one with that user agent, else the last to connect. Chosen again at each read, so a browser that reconnects keeps a run's reads.
    fn chosen(&self, user_agent: Option<&str>) -> Option<&Reader> {
        self.connected
            .iter()
            .rfind(|r| user_agent == Some(r.user_agent.as_str()))
            .or_else(|| self.connected.last())
    }
}

struct Reader {
    id: u64,
    user_agent: String,
    events: mpsc::UnboundedSender<Event>,
}

struct Waiting {
    reader: u64,
    body: Vec<u8>,
    answer: oneshot::Sender<Result<BrowserAnswer, BrowserFailed>>,
}

/// One connection's page reads: dropping it, when the connection ends, fails the reads its browser still owed.
pub struct PageReaderClaim {
    readers: Arc<PageReaders>,
    id: u64,
}

impl PageReaders {
    fn lock(&self) -> MutexGuard<'_, Readers> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The browser a run's reads of `site` go through: the one with the download's `user_agent`, else the last to connect.
    pub(crate) fn route(
        self: &Arc<Self>,
        user_agent: Option<&str>,
        site: String,
    ) -> Option<BrowserRoute> {
        self.lock().chosen(user_agent)?;
        Some(BrowserRoute {
            reader: Arc::new(ReaderLink {
                readers: self.clone(),
                user_agent: user_agent.map(str::to_owned),
            }),
            site,
        })
    }

    /// Hands `answer` to the read `id` of `reader`, when it still waits.
    fn settle(&self, reader: u64, id: u64, answer: Result<BrowserAnswer, BrowserFailed>) {
        let mut readers = self.lock();
        if readers.waiting.get(&id).is_some_and(|w| w.reader == reader)
            && let Some(waiting) = readers.waiting.remove(&id)
        {
            // The run may have stopped waiting meanwhile.
            let (Ok(()) | Err(_)) = waiting.answer.send(answer);
        }
    }
}

impl Manager {
    /// A connection's browser makes page reads (D282): the events for it come out of the receiver, and the claim takes its answers.
    #[must_use]
    pub fn connect_page_reader(
        &self,
        user_agent: String,
    ) -> (PageReaderClaim, mpsc::UnboundedReceiver<Event>) {
        self.inner.page_readers.connect(user_agent)
    }
}

impl PageReaders {
    fn connect(
        self: &Arc<Self>,
        user_agent: String,
    ) -> (PageReaderClaim, mpsc::UnboundedReceiver<Event>) {
        let (events, received) = mpsc::unbounded_channel();
        let mut state = self.lock();
        state.next_reader += 1;
        let id = state.next_reader;
        state.connected.push(Reader {
            id,
            user_agent,
            events,
        });
        let claim = PageReaderClaim {
            readers: self.clone(),
            id,
        };
        (claim, received)
    }
}

impl PageReaderClaim {
    /// Takes `call` when it answers a page read: `true` when it was one, whether or not the read still waited.
    #[must_use]
    pub fn take(&self, call: &Call) -> bool {
        let readers = &self.readers;
        match call {
            Call::PageReadPart(part) => {
                let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(&part.bytes)
                else {
                    readers.settle(self.id, part.id.0, Err(BrowserFailed::Network));
                    return true;
                };
                let mut state = readers.lock();
                let too_large = match state.waiting.get_mut(&part.id.0) {
                    Some(waiting) if waiting.reader == self.id => {
                        waiting.body.extend(bytes);
                        waiting.body.len() > MAX_PAGE_READ_LEN
                    }
                    _ => false,
                };
                drop(state);
                if too_large {
                    readers.settle(self.id, part.id.0, Err(BrowserFailed::TooLarge));
                }
            }
            Call::PageReadAnswered(answered) => {
                let mut state = readers.lock();
                let body = match state.waiting.get_mut(&answered.id.0) {
                    Some(waiting) if waiting.reader == self.id => std::mem::take(&mut waiting.body),
                    _ => return true,
                };
                drop(state);
                let answer = BrowserAnswer {
                    status: answered.status,
                    headers: answered
                        .headers
                        .iter()
                        .map(|h| (h.name.clone(), h.value.expose().to_owned()))
                        .collect(),
                    body,
                };
                readers.settle(self.id, answered.id.0, Ok(answer));
            }
            Call::PageReadFailed(failed) => {
                let reason = match failed.reason {
                    PageReadFailure::Network => BrowserFailed::Network,
                    PageReadFailure::TooLarge => BrowserFailed::TooLarge,
                };
                readers.settle(self.id, failed.id.0, Err(reason));
            }
            _ => return false,
        }
        true
    }
}

impl Drop for PageReaderClaim {
    fn drop(&mut self) {
        let mut state = self.readers.lock();
        state.connected.retain(|r| r.id != self.id);
        let owed: Vec<u64> = state
            .waiting
            .iter()
            .filter(|(_, w)| w.reader == self.id)
            .map(|(id, _)| *id)
            .collect();
        for id in owed {
            if let Some(waiting) = state.waiting.remove(&id) {
                let (Ok(()) | Err(_)) = waiting.answer.send(Err(BrowserFailed::Gone));
            }
        }
    }
}

/// A run's way to one browser.
struct ReaderLink {
    readers: Arc<PageReaders>,
    user_agent: Option<String>,
}

impl BrowserReader for ReaderLink {
    fn read(&self, request: BrowserRequest) -> BrowserReading {
        let readers = self.readers.clone();
        let user_agent = self.user_agent.clone();
        Box::pin(async move {
            let (answer, answered) = oneshot::channel();
            let asked = {
                let mut state = readers.lock();
                state.next_read += 1;
                let id = state.next_read;
                let event = Event::PageReadRequested(page_read(id, request));
                let Some(reader) = state
                    .chosen(user_agent.as_deref())
                    .filter(|r| r.events.send(event).is_ok())
                    .map(|r| r.id)
                else {
                    return Err(BrowserFailed::Gone);
                };
                state.waiting.insert(
                    id,
                    Waiting {
                        reader,
                        body: Vec::new(),
                        answer,
                    },
                );
                Asked {
                    readers: readers.clone(),
                    reader,
                    id,
                }
            };
            let limit = Duration::from_millis(PAGE_READ_TIMEOUT_MS);
            let answer = match tokio::time::timeout(limit, answered).await {
                Ok(Ok(answer)) => answer,
                Ok(Err(_)) => Err(BrowserFailed::Gone),
                Err(_) => Err(BrowserFailed::TimedOut),
            };
            drop(asked);
            answer
        })
    }
}

/// A read the run waits for: when the run stops waiting first, the browser is told to stop.
struct Asked {
    readers: Arc<PageReaders>,
    reader: u64,
    id: u64,
}

impl Drop for Asked {
    fn drop(&mut self) {
        let mut state = self.readers.lock();
        if state.waiting.remove(&self.id).is_none() {
            return;
        }
        if let Some(reader) = state.connected.iter().find(|r| r.id == self.reader) {
            let cancelled = Event::PageReadCancelled(PageReadCancelled {
                id: PageReadId(self.id),
            });
            // A browser that went away has nothing to stop.
            let (Ok(()) | Err(_)) = reader.events.send(cancelled);
        }
    }
}

/// The event for request `request`, without the headers the browser sets itself.
fn page_read(id: u64, request: BrowserRequest) -> PageRead {
    PageRead {
        id: PageReadId(id),
        method: request.method,
        url: Secret::new(request.url.as_str()),
        headers: request
            .headers
            .into_iter()
            .filter(|(name, _)| {
                let name = name.to_ascii_lowercase();
                !BROWSER_SETS.contains(&name.as_str())
                    && !name.starts_with("sec-")
                    && !name.starts_with("proxy-")
            })
            .map(|(name, value)| Header {
                name,
                value: Secret::new(value),
            })
            .collect(),
        body: (!request.body.is_empty())
            .then(|| base64::engine::general_purpose::STANDARD.encode(&request.body)),
    }
}

impl Manager {
    /// The browser a media read of `url` goes through (D282), with the state locked: none for a private download, since an extension's `fetch()` carries the regular profile's cookies.
    pub(super) fn browser_route(
        &self,
        state: &super::state::State,
        origin: &crate::headers::Origin,
        url: &Url,
    ) -> Option<BrowserRoute> {
        if super::private_sessions::is_private(origin) {
            return None;
        }
        let host = url.host_str()?;
        let site = state
            .suffixes
            .registrable_domain(host)
            .unwrap_or(host)
            .to_owned();
        self.inner
            .page_readers
            .route(origin.user_agent.as_deref(), site)
    }
}

#[cfg(test)]
mod tests {
    use wirecatch_protocol::page_read::{PageReadAnswer, PageReadPart};

    use super::*;

    fn part(id: PageReadId, bytes: &[u8]) -> Call {
        Call::PageReadPart(PageReadPart {
            id,
            bytes: base64::engine::general_purpose::STANDARD.encode(bytes),
        })
    }

    /// A run's request reaches its browser without the headers the browser sets itself, its answer comes back joined from its parts, and a browser that goes away fails what it still owed.
    #[tokio::test]
    async fn a_browser_answers_a_runs_request_in_parts() {
        let readers = Arc::new(PageReaders::default());
        let (claim, mut events) = readers.connect("Browser/1".into());
        let route = readers
            .route(Some("Browser/1"), "example.com".into())
            .unwrap();
        let request = BrowserRequest {
            method: "GET".into(),
            url: Url::parse("https://www.example.com/v").unwrap(),
            headers: vec![
                ("Accept".into(), "*/*".into()),
                ("Cookie".into(), "yt-dlp's".into()),
                ("Sec-Fetch-Mode".into(), "navigate".into()),
            ],
            body: Vec::new(),
        };
        let reading = tokio::spawn(route.reader.read(request.clone()));
        let Some(Event::PageReadRequested(asked)) = events.recv().await else {
            panic!("no page read was asked");
        };
        assert_eq!(asked.url.expose(), "https://www.example.com/v");
        let names: Vec<&str> = asked.headers.iter().map(|h| h.name.as_str()).collect();
        assert_eq!(names, ["Accept"]);
        assert!(claim.take(&part(asked.id, b"pa")));
        assert!(claim.take(&part(asked.id, b"ge")));
        assert!(claim.take(&Call::PageReadAnswered(PageReadAnswer {
            id: asked.id,
            status: 410,
            headers: Vec::new(),
        })));
        let answer = reading.await.unwrap().unwrap();
        assert_eq!(
            (answer.status, answer.body.as_slice()),
            (410, b"page".as_slice())
        );

        let reading = tokio::spawn(route.reader.read(request));
        assert!(events.recv().await.is_some());
        drop(claim);
        assert_eq!(reading.await.unwrap().unwrap_err(), BrowserFailed::Gone);
        assert!(readers.route(None, "example.com".into()).is_none());
    }
}

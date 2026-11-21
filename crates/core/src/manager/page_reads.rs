//! The browsers that make page reads for media reads (D282): each connection whose hello offers them, the reads waiting for their answers, and which browser a run goes through.

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use base64::Engine as _;
use tokio::sync::{mpsc, oneshot};
use wirecatch_engine::Url;
use wirecatch_media::ytdlp::{
    BrowserAnswer, BrowserFailed, BrowserReader, BrowserReading, BrowserRequest, BrowserRoute,
    RoutedHosts,
};
use wirecatch_protocol::Secret;
use wirecatch_protocol::extension::Header;
use wirecatch_protocol::page_read::{
    MAX_PAGE_READ_LEN, MAX_PAGE_READ_PART, PAGE_READ_TIMEOUT_MS, PageRead, PageReadAnswer,
    PageReadCancelled, PageReadFailure, PageReadId,
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

/// Identifies one connection's browser among those that make page reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ReaderId(u64);

/// The connected browsers that make page reads, and the reads they owe.
#[derive(Default)]
pub(crate) struct PageReaders {
    state: Mutex<Readers>,
}

#[derive(Default)]
struct Readers {
    last_reader: u64,
    last_read: u64,
    /// In the order they connected.
    connected: Vec<Reader>,
    waiting: HashMap<PageReadId, Waiting>,
}

impl Readers {
    /// The browser of `profile`, the last of its connections. Chosen again at each read, so a browser that reconnects keeps a run's reads.
    fn chosen(&self, profile: &str) -> Option<&Reader> {
        self.connected.iter().rfind(|r| r.profile == profile)
    }
}

struct Reader {
    id: ReaderId,
    profile: String,
    events: mpsc::UnboundedSender<Event>,
}

struct Waiting {
    reader: ReaderId,
    body: Vec<u8>,
    answer: oneshot::Sender<Result<BrowserAnswer, BrowserFailed>>,
}

/// One connection's page reads: dropping it, when the connection ends, fails the reads its browser still owed.
pub struct PageReaderClaim {
    readers: Arc<PageReaders>,
    id: ReaderId,
}

impl PageReaders {
    fn lock(&self) -> MutexGuard<'_, Readers> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The browser of `profile` a run's reads of `hosts` go through, when it is connected.
    pub(crate) fn route(
        self: &Arc<Self>,
        profile: &str,
        hosts: RoutedHosts,
    ) -> Option<BrowserRoute> {
        self.lock().chosen(profile)?;
        Some(BrowserRoute {
            reader: Arc::new(ReaderLink {
                readers: self.clone(),
                profile: profile.to_owned(),
            }),
            hosts,
        })
    }

    /// Hands `answer` to the read `id` of `reader`, when it still waits.
    fn settle(
        &self,
        reader: ReaderId,
        id: PageReadId,
        answer: Result<BrowserAnswer, BrowserFailed>,
    ) {
        let mut readers = self.lock();
        if readers.waiting.get(&id).is_some_and(|w| w.reader == reader)
            && let Some(waiting) = readers.waiting.remove(&id)
        {
            // The run may have stopped waiting meanwhile.
            let (Ok(()) | Err(_)) = waiting.answer.send(answer);
        }
    }

    fn connect(
        self: &Arc<Self>,
        profile: String,
    ) -> (PageReaderClaim, mpsc::UnboundedReceiver<Event>) {
        let (events, received) = mpsc::unbounded_channel();
        let mut state = self.lock();
        state.last_reader += 1;
        let id = ReaderId(state.last_reader);
        state.connected.push(Reader {
            id,
            profile,
            events,
        });
        let claim = PageReaderClaim {
            readers: self.clone(),
            id,
        };
        (claim, received)
    }
}

impl Manager {
    /// A connection's browser, of `profile`, makes page reads (D282): the events for it come out of the receiver, and the claim takes its answers.
    #[must_use]
    pub fn connect_page_reader(
        &self,
        profile: String,
    ) -> (PageReaderClaim, mpsc::UnboundedReceiver<Event>) {
        self.inner.page_readers.connect(profile)
    }
}

impl PageReaderClaim {
    /// Takes `call` when it answers a page read: `true` when it was one, whether or not the read still waited. A part or an answer that breaks the protocol's limits fails its read.
    #[must_use]
    pub fn take(&self, call: &Call) -> bool {
        let readers = &self.readers;
        match call {
            Call::PageReadPart(part) => {
                let bytes = base64::engine::general_purpose::STANDARD
                    .decode(&part.bytes)
                    .ok()
                    .filter(|bytes| bytes.len() <= MAX_PAGE_READ_PART);
                let Some(bytes) = bytes else {
                    readers.settle(self.id, part.id, Err(BrowserFailed::Malformed));
                    return true;
                };
                let mut state = readers.lock();
                let too_large = match state.waiting.get_mut(&part.id) {
                    Some(waiting) if waiting.reader == self.id => {
                        waiting.body.extend(bytes);
                        waiting.body.len() > MAX_PAGE_READ_LEN
                    }
                    _ => false,
                };
                drop(state);
                if too_large {
                    readers.settle(self.id, part.id, Err(BrowserFailed::TooLarge));
                }
            }
            Call::PageReadAnswered(answered) => {
                let mut state = readers.lock();
                let body = match state.waiting.get_mut(&answered.id) {
                    Some(waiting) if waiting.reader == self.id => std::mem::take(&mut waiting.body),
                    _ => return true,
                };
                drop(state);
                readers.settle(self.id, answered.id, answer_of(answered, body));
            }
            Call::PageReadFailed(failed) => {
                let reason = match failed.reason {
                    PageReadFailure::Network => BrowserFailed::Network,
                    PageReadFailure::TooLarge => BrowserFailed::TooLarge,
                };
                readers.settle(self.id, failed.id, Err(reason));
            }
            _ => return false,
        }
        true
    }
}

/// The answer `answered` with `body`, when HTTP can carry it; an address that does not read is no address.
fn answer_of(answered: &PageReadAnswer, body: Vec<u8>) -> Result<BrowserAnswer, BrowserFailed> {
    BrowserAnswer::checked(
        answered.status,
        answered
            .headers
            .iter()
            .map(|h| (h.name.clone(), h.value.expose().to_owned()))
            .collect(),
        body,
        answered
            .address
            .as_deref()
            .and_then(|address| address.parse::<IpAddr>().ok()),
    )
}

impl Drop for PageReaderClaim {
    fn drop(&mut self) {
        let mut state = self.readers.lock();
        state.connected.retain(|r| r.id != self.id);
        let owed: Vec<PageReadId> = state
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

/// A run's way to one browser profile.
struct ReaderLink {
    readers: Arc<PageReaders>,
    profile: String,
}

impl BrowserReader for ReaderLink {
    fn read(&self, request: BrowserRequest) -> BrowserReading {
        let readers = self.readers.clone();
        let profile = self.profile.clone();
        Box::pin(async move {
            let (answer, answered) = oneshot::channel();
            let asked = {
                let mut state = readers.lock();
                state.last_read += 1;
                let id = PageReadId(state.last_read);
                let event = Event::PageReadRequested(page_read(id, request));
                let Some(reader) = state
                    .chosen(&profile)
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
    reader: ReaderId,
    id: PageReadId,
}

impl Drop for Asked {
    fn drop(&mut self) {
        let mut state = self.readers.lock();
        if state.waiting.remove(&self.id).is_none() {
            return;
        }
        if let Some(reader) = state.connected.iter().find(|r| r.id == self.reader) {
            let cancelled = Event::PageReadCancelled(PageReadCancelled { id: self.id });
            // A browser that went away has nothing to stop.
            let (Ok(()) | Err(_)) = reader.events.send(cancelled);
        }
    }
}

/// The event for request `request`, without the headers the browser sets itself.
fn page_read(id: PageReadId, request: BrowserRequest) -> PageRead {
    PageRead {
        id,
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
    /// The browser a media read of `url` goes through (D282), with the state locked: see [`routed_profile`].
    pub(super) fn browser_route(
        &self,
        state: &super::state::State,
        origin: &crate::headers::Origin,
        url: &Url,
    ) -> Option<BrowserRoute> {
        let profile = routed_profile(origin)?;
        let host = url.host_str()?;
        let site = state
            .suffixes
            .registrable_domain(host)
            .unwrap_or(host)
            .to_owned();
        self.inner
            .page_readers
            .route(profile, RoutedHosts::Site(site))
    }
}

/// The browser profile a download's reads go through: the one it came from, and none for a private download (an extension's `fetch()` carries the regular profile's cookies) or one whose sign-in was forgotten (D17), which must not pick up the browser's current one.
fn routed_profile(origin: &crate::headers::Origin) -> Option<&str> {
    if super::private_sessions::is_private(origin) || origin.forgot_sign_in {
        return None;
    }
    origin.browser_source.as_ref()?.profile.as_deref()
}

#[cfg(test)]
mod tests {
    use wirecatch_protocol::page_read::PageReadPart;
    use wirecatch_test_support::PATIENCE;

    use super::*;

    fn part(id: PageReadId, bytes: &[u8]) -> Call {
        Call::PageReadPart(PageReadPart {
            id,
            bytes: base64::engine::general_purpose::STANDARD.encode(bytes),
        })
    }

    fn answered(id: PageReadId, status: u16, headers: Vec<Header>, address: &str) -> Call {
        Call::PageReadAnswered(PageReadAnswer {
            id,
            status,
            headers,
            address: Some(address.into()),
        })
    }

    fn request() -> BrowserRequest {
        BrowserRequest {
            method: "GET".into(),
            url: Url::parse("https://www.example.com/v").unwrap(),
            headers: vec![
                ("Accept".into(), "*/*".into()),
                ("Cookie".into(), "yt-dlp's".into()),
                ("Sec-Fetch-Mode".into(), "navigate".into()),
            ],
            body: Vec::new(),
        }
    }

    fn site() -> RoutedHosts {
        RoutedHosts::Site("example.com".into())
    }

    /// The read the browser is asked for next.
    async fn asked(events: &mut mpsc::UnboundedReceiver<Event>) -> PageRead {
        match tokio::time::timeout(PATIENCE, events.recv()).await {
            Ok(Some(Event::PageReadRequested(asked))) => asked,
            other => panic!("no page read was asked: {other:?}"),
        }
    }

    async fn settled(
        reading: tokio::task::JoinHandle<Result<BrowserAnswer, BrowserFailed>>,
    ) -> Result<BrowserAnswer, BrowserFailed> {
        tokio::time::timeout(PATIENCE, reading)
            .await
            .unwrap()
            .unwrap()
    }

    /// A run's request reaches its browser without the headers the browser sets itself, its answer comes back joined from its parts, and a browser that goes away fails what it still owed.
    #[tokio::test]
    async fn a_browser_answers_a_runs_request_in_parts() {
        let readers = Arc::new(PageReaders::default());
        let (claim, mut events) = readers.connect("profile-1".into());
        let route = readers.route("profile-1", site()).unwrap();
        let reading = tokio::spawn(route.reader.read(request()));
        let read = asked(&mut events).await;
        assert_eq!(read.url.expose(), "https://www.example.com/v");
        let names: Vec<&str> = read.headers.iter().map(|h| h.name.as_str()).collect();
        assert_eq!(names, ["Accept"]);
        assert!(claim.take(&part(read.id, b"pa")));
        assert!(claim.take(&part(read.id, b"ge")));
        assert!(claim.take(&answered(read.id, 410, Vec::new(), "93.184.215.14")));
        let answer = settled(reading).await.unwrap();
        assert_eq!(
            (answer.status, answer.body.as_slice(), answer.address),
            (410, b"page".as_slice(), "93.184.215.14".parse().ok())
        );

        let reading = tokio::spawn(route.reader.read(request()));
        asked(&mut events).await;
        drop(claim);
        assert_eq!(settled(reading).await.unwrap_err(), BrowserFailed::Gone);
        assert!(readers.route("profile-1", site()).is_none());
    }

    /// A download's reads go only through the browser profile it came from, never another one connected at the same time.
    #[tokio::test]
    async fn a_read_goes_only_through_the_downloads_own_profile() {
        let readers = Arc::new(PageReaders::default());
        let (_first, mut first_events) = readers.connect("profile-1".into());
        let (_second, mut second_events) = readers.connect("profile-2".into());
        assert!(readers.route("profile-3", site()).is_none());
        let route = readers.route("profile-1", site()).unwrap();
        let reading = tokio::spawn(route.reader.read(request()));
        asked(&mut first_events).await;
        assert!(second_events.try_recv().is_err());
        reading.abort();
    }

    /// A part over the protocol's limit, a status no final answer has, and a header HTTP/1.1 cannot carry each fail their read rather than reach yt-dlp.
    #[tokio::test]
    async fn a_reply_that_breaks_the_protocol_fails_its_read() {
        let readers = Arc::new(PageReaders::default());
        let (claim, mut events) = readers.connect("profile-1".into());
        let route = readers.route("profile-1", site()).unwrap();
        let header = |name: &str, value: &str| Header {
            name: name.into(),
            value: Secret::new(value),
        };
        let replies = [
            part(PageReadId(0), &vec![0; MAX_PAGE_READ_PART + 1]),
            answered(PageReadId(0), 101, Vec::new(), "93.184.215.14"),
            answered(PageReadId(0), 1000, Vec::new(), "93.184.215.14"),
            answered(
                PageReadId(0),
                200,
                vec![header("Bad Name", "x")],
                "93.184.215.14",
            ),
            answered(
                PageReadId(0),
                200,
                vec![header("X-Split", "a\r\nb: c")],
                "93.184.215.14",
            ),
        ];
        for reply in replies {
            let reading = tokio::spawn(route.reader.read(request()));
            let id = asked(&mut events).await.id;
            let reply = match reply {
                Call::PageReadPart(part) => Call::PageReadPart(PageReadPart { id, ..part }),
                Call::PageReadAnswered(answer) => {
                    Call::PageReadAnswered(PageReadAnswer { id, ..answer })
                }
                other => other,
            };
            assert!(claim.take(&reply));
            assert_eq!(
                settled(reading).await.unwrap_err(),
                BrowserFailed::Malformed
            );
        }
    }

    /// An address the browser reports that does not read is no address: the relay does not use the answer.
    #[tokio::test]
    async fn an_unreadable_address_is_no_address() {
        let readers = Arc::new(PageReaders::default());
        let (claim, mut events) = readers.connect("profile-1".into());
        let route = readers.route("profile-1", site()).unwrap();
        let reading = tokio::spawn(route.reader.read(request()));
        let id = asked(&mut events).await.id;
        assert!(claim.take(&answered(id, 200, Vec::new(), "not an address")));
        assert_eq!(settled(reading).await.unwrap().address, None);
    }

    /// Only a download that names its profile goes through the browser, and not once it is private or its sign-in was forgotten.
    #[test]
    fn only_a_signed_in_download_from_a_profile_has_one() {
        use wirecatch_records::origin::BrowserSource;
        let origin = |profile: Option<&str>, private: bool, forgot: bool| crate::headers::Origin {
            forgot_sign_in: forgot,
            browser_source: Some(Box::new(BrowserSource {
                observed_page: None,
                page_redirect_urls: Vec::new(),
                redirect_urls: Vec::new(),
                private_window: private,
                private_session: private.then(|| "session-1".into()),
                profile: profile.map(Into::into),
                file_name: None,
            })),
            ..crate::headers::Origin::default()
        };
        assert_eq!(routed_profile(&origin(Some("p"), false, false)), Some("p"));
        assert_eq!(routed_profile(&origin(None, false, false)), None);
        assert_eq!(routed_profile(&origin(Some("p"), true, false)), None);
        assert_eq!(routed_profile(&origin(Some("p"), false, true)), None);
        assert_eq!(routed_profile(&crate::headers::Origin::default()), None);
    }
}

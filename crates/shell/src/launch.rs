//! What a launch asks for, read from its command line, and handing it to the copy of the app
//! that already runs for this user (D103).

use std::ffi::OsString;
use std::future::Future;
use std::time::Duration;

use wirecatch_engine::units::KIB_USIZE;
use wirecatch_ipc::IpcError;
use wirecatch_protocol::frame::{self, FrameError};
use wirecatch_protocol::version::VersionRange;
use wirecatch_protocol::wire::{
    Call, CallMethod, ClientKind, Hello, MAX_OPEN_LINKS, Message, OpenLinks, Outcome,
    ProtocolError, Reply,
};

/// Why a launch could not be handed to the running copy.
#[derive(Debug, thiserror::Error)]
pub enum HandOverError {
    #[error(transparent)]
    Reach(#[from] IpcError),
    #[error(transparent)]
    Frame(#[from] FrameError),
    #[error("the running copy did not answer in time")]
    Timeout,
    #[error("the running copy closed the connection")]
    Closed,
    #[error("the running copy answered the hello with {0}")]
    Unexpected(String),
    #[error("{}", .0.message)]
    Refused(ProtocolError),
}

impl HandOverError {
    /// Nothing listens at the endpoint any more, so this launch may claim it after all.
    #[must_use]
    pub fn copy_has_gone(&self) -> bool {
        matches!(self, Self::Reach(IpcError::NotRunning { .. }))
    }

    /// A new connection may fare better: the running copy may be restarting its connection, or
    /// another client holds the one pipe instance waiting, and a busy pipe reports `Io`.
    fn may_pass(&self) -> bool {
        matches!(
            self,
            Self::Closed | Self::Frame(_) | Self::Reach(IpcError::Io { .. })
        )
    }
}

/// Shown by a copy of the app that finds another one already serving this user and cannot hand
/// its launch over.
pub const ALREADY_RUNNING: &str = "Wirecatch is already running for you, so this copy will close. Look for its icon in the tray, or open it from the Start menu again.";

/// Says that another copy of the app is already running, and waits until the person closes the
/// message.
pub fn say_already_running() {
    crate::message_box::show(
        crate::message_box::Tone::Information,
        crate::host::CHANNEL.name(),
        ALREADY_RUNNING,
    );
}

/// The flag the bridge (and the start-with-system entry) passes: run with no window.
pub const BACKGROUND_FLAG: &str = "--background";

/// Most links taken from one command line.
pub const MAX_LAUNCH_LINKS: usize = MAX_OPEN_LINKS;

/// Longest link taken from a command line, in bytes.
pub const MAX_LINK_LEN: usize = 8 * KIB_USIZE;

/// Schemes a command-line link may use.
const LINK_SCHEMES: [&str; 5] = ["http://", "https://", "ftp://", "ftps://", "ftpes://"];

/// How long a launch waits for a running copy to answer. Opening a long list, or repairing one,
/// takes this copy no window and the person no action, so it may wait.
const HAND_OVER_PATIENCE: Duration = Duration::from_mins(1);

/// The pause before trying again a running copy that closed the connection at once.
const TRY_AGAIN_AFTER: Duration = Duration::from_millis(200);

/// This process's command line. An argument that is not valid text, which a link never is, keeps
/// its readable part: `std::env::args` would abort the app on it.
#[must_use]
pub fn command_line() -> Vec<String> {
    readable(std::env::args_os())
}

fn readable(args: impl IntoIterator<Item = OsString>) -> Vec<String> {
    args.into_iter()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect()
}

/// True when the command line asks to start without a window. `args` includes the program name.
pub fn wants_background<I, S>(args: I) -> bool
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    args.into_iter()
        .skip(1)
        .any(|a| a.as_ref() == BACKGROUND_FLAG)
}

/// Whether `arg` (already trimmed) is a link the Add dialog may offer: a known scheme with
/// something after it, no control characters, and not absurdly long.
fn is_launch_link(arg: &str) -> bool {
    if arg.len() > MAX_LINK_LEN || arg.chars().any(char::is_control) {
        return false;
    }
    let lower = arg.to_ascii_lowercase();
    LINK_SCHEMES
        .iter()
        .any(|scheme| lower.starts_with(scheme) && lower.len() > scheme.len())
}

/// Web and FTP links on a command line (program name first), in order, for the Add dialog.
/// Anything else on the command line is ignored, and at most [`MAX_LAUNCH_LINKS`] are taken.
pub fn links_from_args<I, S>(args: I) -> Vec<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    args.into_iter()
        .skip(1)
        .map(|a| a.as_ref().trim().to_owned())
        .filter(|a| is_launch_link(a))
        .take(MAX_LAUNCH_LINKS)
        .collect()
}

/// What a launch asks for, read from its command line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Launch {
    /// Started for the browser or at sign-in: show nothing.
    pub background: bool,
    /// Links to offer in the Add dialog.
    pub links: Vec<String>,
}

impl Launch {
    /// Reads a command line (program name first).
    #[must_use]
    pub fn from_args(args: &[String]) -> Self {
        Self {
            background: wants_background(args),
            links: links_from_args(args),
        }
    }

    /// Whether this launch should bring up the window. A link always does, even with
    /// `--background`, because the person asked to download something.
    #[must_use]
    pub fn shows_window(&self) -> bool {
        !self.background || !self.links.is_empty()
    }

    /// The call that hands this launch to a running copy speaking protocol `version`, or `None`
    /// when it asks for nothing visible. A copy too old for links still shows its window.
    fn call(&self, version: u32) -> Option<Call> {
        if !self.links.is_empty() && CallMethod::OpenLinks.is_in(version) {
            Some(Call::OpenLinks(OpenLinks {
                links: self.links.clone(),
            }))
        } else {
            self.shows_window().then_some(Call::ShowWindow)
        }
    }
}

/// Hands `launch` to the copy of the app already serving this user: it shows its window, or
/// opens the Add dialog with the links.
///
/// # Errors
///
/// The running copy could not be reached, refused the hello, or did not answer within
/// [`HAND_OVER_PATIENCE`]; the message says which.
pub async fn hand_over(launch: &Launch) -> Result<(), HandOverError> {
    let endpoint = crate::host::endpoint()?;
    hand_over_patiently(launch, || wirecatch_ipc::connect(&endpoint)).await
}

/// A launch that asks for nothing visible contacts no one, so it has nothing to fail at.
async fn hand_over_patiently<S, C, F>(launch: &Launch, mut connect: C) -> Result<(), HandOverError>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
    C: FnMut() -> F,
    F: Future<Output = Result<S, IpcError>>,
{
    if !launch.shows_window() {
        return Ok(());
    }
    let deadline = tokio::time::Instant::now() + HAND_OVER_PATIENCE;
    loop {
        let result = match connect().await {
            // Kept open for the whole wait: on Windows the one pipe instance is taken until the
            // running copy accepts, so a new connection would find it busy.
            Ok(mut stream) => hand_over_on(&mut stream, launch, deadline).await,
            Err(e) => Err(e.into()),
        };
        match result {
            Err(e) if e.may_pass() && tokio::time::Instant::now() < deadline => {
                tracing::debug!("the running copy cannot take this launch yet: {e}");
                // Another process: there is no event to wait on.
                tokio::time::sleep(TRY_AGAIN_AFTER).await;
            }
            other => return other,
        }
    }
}

/// [`hand_over`] on a connection already open, waiting for answers until `deadline`.
async fn hand_over_on<S>(
    stream: &mut S,
    launch: &Launch,
    deadline: tokio::time::Instant,
) -> Result<(), HandOverError>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let hello = Call::Hello(Hello {
        client: ClientKind::App,
        client_version: wirecatch_product::PRODUCT_VERSION.to_owned(),
        protocol_version: VersionRange::THIS_BUILD.oldest(),
        max_protocol_version: Some(VersionRange::THIS_BUILD.newest()),
        browser: None,
        start_app: false,
    });
    let version = match ask(stream, 1, hello, deadline).await? {
        Reply::Hello(reply) => reply.protocol_version,
        other => return Err(HandOverError::Unexpected(format!("{other:?}"))),
    };
    match launch.call(version) {
        Some(call) => ask(stream, 2, call, deadline).await.map(|_| ()),
        None => Ok(()),
    }
}

/// Sends `call` as request `id` and waits for its answer until `deadline`.
async fn ask<S>(
    stream: &mut S,
    id: u64,
    call: Call,
    deadline: tokio::time::Instant,
) -> Result<Reply, HandOverError>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    frame::write_async(stream, &Message::Request { id, call }).await?;
    loop {
        let message = tokio::time::timeout_at(deadline, frame::read_async::<Message>(stream))
            .await
            .map_err(|_| HandOverError::Timeout)??
            .ok_or(HandOverError::Closed)?;
        match message {
            Message::Response {
                id: answered,
                outcome,
            } if answered == id => {
                return match outcome {
                    Outcome::Ok(reply) => Ok(reply),
                    Outcome::Err(error) => Err(HandOverError::Refused(error)),
                };
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wirecatch_protocol::app::UiEvent;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(ToString::to_string).collect()
    }

    /// A connection to a running copy that never answers: `held` keeps the other end open.
    fn unanswered(held: &mut Vec<tokio::io::DuplexStream>) -> tokio::io::DuplexStream {
        let (stream, server_side) = tokio::io::duplex(64 * KIB_USIZE);
        held.push(server_side);
        stream
    }

    #[tokio::test]
    async fn a_launch_asking_for_nothing_visible_contacts_no_one() {
        let launch = Launch::from_args(&args(&["app", "--background"]));
        let mut tries = 0;
        let handed = hand_over_patiently(&launch, || {
            tries += 1;
            std::future::ready(Err::<tokio::io::DuplexStream, _>(IpcError::NotRunning {
                endpoint: "test".into(),
            }))
        })
        .await;
        assert!(handed.is_ok(), "{handed:?}");
        assert_eq!(tries, 0);
    }

    #[tokio::test]
    async fn a_copy_that_has_gone_is_reported_at_once_so_the_launch_can_claim_its_place() {
        let launch = Launch::from_args(&args(&["app"]));
        let mut tries = 0;
        let handed = hand_over_patiently(&launch, || {
            tries += 1;
            std::future::ready(Err::<tokio::io::DuplexStream, _>(IpcError::NotRunning {
                endpoint: "test".into(),
            }))
        })
        .await;
        assert!(handed.is_err_and(|e| e.copy_has_gone()));
        assert_eq!(tries, 1);
    }

    #[tokio::test(start_paused = true)]
    async fn a_copy_that_never_answers_is_waited_for_on_one_connection() {
        let launch = Launch::from_args(&args(&["app", "https://x.example/f"]));
        let mut held = Vec::new();
        let handed = hand_over_patiently(&launch, || {
            std::future::ready(Ok::<_, IpcError>(unanswered(&mut held)))
        })
        .await;
        assert!(matches!(handed, Err(HandOverError::Timeout)), "{handed:?}");
        assert_eq!(held.len(), 1, "a new connection would find the pipe busy");
    }

    #[test]
    fn an_argument_that_is_not_text_is_read_without_harm() {
        #[cfg(windows)]
        let odd = {
            use std::os::windows::ffi::OsStringExt as _;
            OsString::from_wide(&[0xD800])
        };
        #[cfg(unix)]
        let odd = {
            use std::os::unix::ffi::OsStringExt as _;
            OsString::from_vec(vec![0xFF])
        };
        let line = readable(["app".into(), odd, "https://x.example/f".into()]);
        assert_eq!(line.len(), 3);
        assert_eq!(links_from_args(&line), ["https://x.example/f"]);
    }

    #[test]
    fn the_already_running_message_is_plain() {
        assert!(ALREADY_RUNNING.contains("already running"));
        assert!(!ALREADY_RUNNING.contains('\u{2014}'));
    }

    #[test]
    fn background_flag_is_read_after_the_program_name() {
        assert!(wants_background(["wirecatch-app", "--background"]));
        assert!(!wants_background(["wirecatch-app"]));
        assert!(!wants_background(["--background"]));
        assert!(!wants_background(["wirecatch-app", "--backgrounds"]));
    }

    #[test]
    fn links_are_picked_from_the_command_line() {
        let found = links_from_args([
            "C:\\Apps\\wirecatch-app.exe",
            "https://example.com/a.zip",
            "--background",
            "notes.txt",
            "HTTP://EXAMPLE.COM/B",
            "ftp://files.example.com/c.iso",
            "https://",
        ]);
        assert_eq!(
            found,
            [
                "https://example.com/a.zip",
                "HTTP://EXAMPLE.COM/B",
                "ftp://files.example.com/c.iso"
            ]
        );
        assert_eq!(
            links_from_args(["https://not-the-program-name.example"]),
            [] as [String; 0]
        );
    }

    #[test]
    fn odd_or_hostile_arguments_are_not_taken_as_links() {
        let long = format!("https://example.com/{}", "a".repeat(MAX_LINK_LEN));
        let found = links_from_args([
            "app",
            "  https://example.com/ok  ",
            "ftpes://files.example.com/x",
            "https://example.com/a\nb",
            "https://example.com/\u{7}bell",
            "file:///C:/Windows/system32/calc.exe",
            "javascript:alert(1)",
            "data:text/html,hi",
            long.as_str(),
        ]);
        assert_eq!(
            found,
            ["https://example.com/ok", "ftpes://files.example.com/x"]
        );
    }

    #[test]
    fn at_most_twenty_links_are_taken() {
        let many: Vec<String> = std::iter::once("app".to_string())
            .chain((0..50).map(|i| format!("https://example.com/{i}")))
            .collect();
        let found = links_from_args(&many);
        assert_eq!(found.len(), MAX_LAUNCH_LINKS);
        assert_eq!(found[0], "https://example.com/0");
    }

    #[test]
    fn a_background_launch_shows_nothing_unless_it_brings_a_link() {
        assert!(!Launch::from_args(&args(&["app", "--background"])).shows_window());
        assert!(Launch::from_args(&args(&["app"])).shows_window());
        let with_link = Launch::from_args(&args(&["app", "--background", "https://x.example/f"]));
        assert!(with_link.shows_window());
        assert_eq!(with_link.links, ["https://x.example/f"]);
    }

    #[test]
    fn a_launch_is_handed_over_as_the_running_copy_can_take_it() {
        let quiet = Launch::from_args(&args(&["app", "--background"]));
        assert_eq!(quiet.call(2), None);
        assert_eq!(
            Launch::from_args(&args(&["app"])).call(2),
            Some(Call::ShowWindow)
        );
        let linked = Launch::from_args(&args(&["app", "https://x.example/f"]));
        assert_eq!(
            linked.call(2),
            Some(Call::OpenLinks(OpenLinks {
                links: vec!["https://x.example/f".into()]
            }))
        );
        assert_eq!(linked.call(1), Some(Call::ShowWindow));
    }

    async fn manager_in(dir: &std::path::Path) -> wirecatch_core::Manager {
        use wirecatch_core::{KnownFolders, ManagerConfig};

        let mut config = ManagerConfig::new(dir.join("data"), KnownFolders::under_home(dir));
        config.addon_sources.ytdlp_releases = None;
        let core = crate::host::Core::new(config);
        core.opened().await.unwrap().clone()
    }

    async fn links_requested(ui: &mut tokio::sync::broadcast::Receiver<UiEvent>) -> Vec<String> {
        loop {
            if let UiEvent::LinksRequested(links) = ui.recv().await.unwrap() {
                return links;
            }
        }
    }

    /// Another client takes the pipe's one instance, as the first try of a launch does, so the
    /// launch finds the pipe busy until the running copy opens its list and accepts.
    #[cfg(windows)]
    #[tokio::test]
    async fn a_busy_pipe_is_tried_again_until_the_running_copy_accepts() {
        use wirecatch_ipc::{Endpoint, Listener};
        use wirecatch_test_support::PATIENCE;

        let dir = wirecatch_test_support::private_dir("launch").unwrap();
        let manager = manager_in(dir.path()).await;
        let mut ui = manager.subscribe();
        let endpoint = Endpoint::from_raw(dir.endpoint_name());
        let mut listener = Listener::bind(&endpoint).unwrap();
        let holder = wirecatch_ipc::connect(&endpoint).await.unwrap();

        let (bump, mut total) = tokio::sync::watch::channel(0_u32);
        let launch = Launch::from_args(&args(&["app", "https://x.example/f"]));
        let client = endpoint.clone();
        let handing = tokio::spawn(async move {
            hand_over_patiently(&launch, || {
                bump.send_modify(|count| *count += 1);
                wirecatch_ipc::connect(&client)
            })
            .await
        });
        tokio::time::timeout(PATIENCE, total.wait_for(|count| *count >= 2))
            .await
            .unwrap()
            .unwrap();

        let first = listener.accept().await.unwrap();
        drop((holder, first));
        let second = listener.accept().await.unwrap();
        tokio::spawn(wirecatch_server::serve_connection(manager, second));
        let handed = tokio::time::timeout(PATIENCE, handing)
            .await
            .unwrap()
            .unwrap();
        assert!(handed.is_ok(), "{handed:?}");
        assert_eq!(links_requested(&mut ui).await, ["https://x.example/f"]);
    }

    /// The hello is sent while the running copy has not accepted: its list is still opening.
    #[tokio::test]
    async fn a_launch_is_answered_on_the_connection_it_opened_before_the_list_was_open() {
        use wirecatch_ipc::{Endpoint, Listener};
        use wirecatch_test_support::PATIENCE;

        const HELLO_SENT_BY_THEN: Duration = Duration::from_millis(100);

        let dir = wirecatch_test_support::private_dir("launch").unwrap();
        let manager = manager_in(dir.path()).await;
        let mut ui = manager.subscribe();
        let endpoint = Endpoint::from_raw(dir.endpoint_name());
        let mut listener = Listener::bind(&endpoint).unwrap();
        let mut stream = wirecatch_ipc::connect(&endpoint).await.unwrap();

        let handing = tokio::spawn(async move {
            let launch = Launch::from_args(&args(&["app", "https://x.example/f"]));
            let deadline = tokio::time::Instant::now() + PATIENCE;
            hand_over_on(&mut stream, &launch, deadline).await
        });
        tokio::time::sleep(HELLO_SENT_BY_THEN).await;
        let accepted = listener.accept().await.unwrap();
        tokio::spawn(wirecatch_server::serve_connection(manager, accepted));
        handing.await.unwrap().unwrap();
        assert_eq!(links_requested(&mut ui).await, ["https://x.example/f"]);
    }

    #[tokio::test]
    async fn a_second_launch_hands_its_links_to_the_running_copy() {
        let dir = wirecatch_test_support::private_dir("launch").unwrap();
        let manager = manager_in(dir.path()).await;
        let mut ui = manager.subscribe();
        let (mut stream, server_side) = tokio::io::duplex(64 * KIB_USIZE);
        tokio::spawn(wirecatch_server::serve_connection(
            manager.clone(),
            server_side,
        ));

        let launch = Launch::from_args(&args(&["app", "https://x.example/f"]));
        let deadline = tokio::time::Instant::now() + wirecatch_test_support::PATIENCE;
        hand_over_on(&mut stream, &launch, deadline).await.unwrap();
        assert_eq!(links_requested(&mut ui).await, ["https://x.example/f"]);

        // A copy not ready for the first try, which ends at once, takes the second.
        let launch = Launch::from_args(&args(&["app", "https://y.example/f"]));
        let mut tries = 0;
        let handed = hand_over_patiently(&launch, || {
            tries += 1;
            let (stream, server_side) = tokio::io::duplex(64 * KIB_USIZE);
            if tries > 1 {
                tokio::spawn(wirecatch_server::serve_connection(
                    manager.clone(),
                    server_side,
                ));
            }
            std::future::ready(Ok::<_, IpcError>(stream))
        })
        .await;
        assert!(handed.is_ok(), "{handed:?}");
        assert_eq!(tries, 2);
        assert_eq!(links_requested(&mut ui).await, ["https://y.example/f"]);
    }
}

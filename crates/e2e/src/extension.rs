//! A scripted browser extension: starts the real bridge exactly as a browser does
//! (the caller on the command line, frames on piped stdin and stdout) and speaks
//! the protocol to it.

use std::collections::{HashMap, VecDeque};
use std::ffi::OsString;
use std::io;
use std::process::{ExitStatus, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::{Child, ChildStdin, Command};
use tokio::sync::mpsc;
use tokio::time::{Instant, timeout, timeout_at};
use wirecatch_ipc::Endpoint;
use wirecatch_protocol::PROTOCOL_VERSION;
use wirecatch_protocol::extension::{
    AddedDownload, ExtensionSettings, ExtensionSettingsPatch, NewDownload, WithdrawOutcome,
    WithdrawRequest,
};
use wirecatch_protocol::frame;
use wirecatch_protocol::wire::{
    BrowserInfo, Call, ClientKind, Event, Hello, HelloReply, Message, Outcome, ProtocolError,
    QueueSummary, Reply,
};

use crate::PATIENCE;
use crate::bridge::{self, ENDPOINT_VAR, LAUNCH_VAR};

/// Who the browser says started the bridge.
#[derive(Debug, Clone)]
pub enum Caller {
    /// Chrome with the extension id every bridge trusts.
    Chrome,
    /// Firefox with the extension's add-on id.
    Firefox,
    /// Chrome with this origin, for example an extension that is not ours.
    ChromeOrigin(String),
}

impl Caller {
    fn args(&self, scratch: &std::path::Path) -> Vec<OsString> {
        match self {
            Self::Chrome => vec![bridge::chrome_origin().into()],
            Self::ChromeOrigin(origin) => vec![origin.into()],
            // Firefox passes the host manifest's path, then the add-on id.
            Self::Firefox => vec![
                scratch.join("app.wirecatch.bridge.json").into(),
                bridge::firefox_id().into(),
            ],
        }
    }
}

/// What the bridge starts when the app is not running.
#[derive(Debug, Clone, Default)]
pub enum Launch {
    /// A program that does not exist, so nothing is ever started.
    #[default]
    Nothing,
    /// This program with these arguments.
    Program(Vec<OsString>),
}

/// The extension end of one native messaging connection.
pub struct FakeExtension {
    child: Child,
    stdin: Option<ChildStdin>,
    inbox: mpsc::UnboundedReceiver<Message>,
    stderr: Arc<Mutex<String>>,
    next_id: u64,
    responses: HashMap<u64, Outcome>,
    events: VecDeque<Event>,
    stdout_closed: bool,
}

impl FakeExtension {
    /// Starts the bridge as Chrome does for our extension, pointed at `endpoint`.
    pub fn chrome(endpoint: &Endpoint) -> Self {
        Self::start(endpoint, Caller::Chrome, Launch::Nothing)
    }

    /// Starts the bridge as Firefox does for our add-on, pointed at `endpoint`.
    pub fn firefox(endpoint: &Endpoint) -> Self {
        Self::start(endpoint, Caller::Firefox, Launch::Nothing)
    }

    /// Starts the bridge for `caller`, connecting to `endpoint` and running
    /// `launch` if nothing listens there.
    pub fn start(endpoint: &Endpoint, caller: Caller, launch: Launch) -> Self {
        let launch = match launch {
            Launch::Nothing => vec![
                std::env::temp_dir()
                    .join(format!(
                        "wirecatch-e2e-no-such-app{}",
                        std::env::consts::EXE_SUFFIX
                    ))
                    .into_os_string(),
            ],
            Launch::Program(parts) => parts,
        };
        let launch: Vec<String> = launch
            .iter()
            .map(|part| {
                part.to_str()
                    .expect("launch arguments are text")
                    .to_string()
            })
            .collect();
        let mut command = Command::new(bridge::bridge_exe());
        command
            .args(caller.args(&std::env::temp_dir()))
            .env(ENDPOINT_VAR, endpoint.as_os_str())
            .env(
                LAUNCH_VAR,
                serde_json::to_string(&launch).expect("text is JSON"),
            )
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        let mut child = command.spawn().expect("could not start wirecatch-bridge");
        let stdin = child.stdin.take();
        let mut stdout = child.stdout.take().expect("stdout is piped");
        let mut stderr_pipe = child.stderr.take().expect("stderr is piped");

        // Reading on a task of its own keeps every read whole: a test may stop
        // waiting at any moment without cutting a frame in half.
        let (to_inbox, inbox) = mpsc::unbounded_channel();
        tokio::spawn(async move {
            loop {
                match frame::read_async::<Message>(&mut stdout).await {
                    Ok(Some(message)) => {
                        if to_inbox.send(message).is_err() {
                            return;
                        }
                    }
                    Ok(None) => return,
                    Err(e) => panic!("the bridge wrote something that is not a frame: {e}"),
                }
            }
        });
        let stderr = Arc::new(Mutex::new(String::new()));
        let log = stderr.clone();
        tokio::spawn(async move {
            let mut buf = [0u8; 4096];
            while let Ok(n) = stderr_pipe.read(&mut buf).await {
                if n == 0 {
                    return;
                }
                log.lock()
                    .expect("stderr log lock")
                    .push_str(&String::from_utf8_lossy(&buf[..n]));
            }
        });

        Self {
            child,
            stdin,
            inbox,
            stderr,
            next_id: 1,
            responses: HashMap::new(),
            events: VecDeque::new(),
            stdout_closed: false,
        }
    }

    /// What the bridge has logged so far.
    pub fn stderr(&self) -> String {
        self.stderr.lock().expect("stderr log lock").clone()
    }

    /// Sends a request and returns its id, or the error if the bridge's stdin is
    /// closed.
    pub async fn try_send(&mut self, call: Call) -> io::Result<u64> {
        let id = self.next_id;
        self.next_id += 1;
        let bytes = frame::encode(&Message::Request { id, call }).map_err(io::Error::other)?;
        let stdin = self
            .stdin
            .as_mut()
            .ok_or_else(|| io::Error::from(io::ErrorKind::BrokenPipe))?;
        stdin.write_all(&bytes).await?;
        stdin.flush().await?;
        Ok(id)
    }

    /// Sends a request and returns its id.
    pub async fn send(&mut self, call: Call) -> u64 {
        match self.try_send(call).await {
            Ok(id) => id,
            Err(e) => panic!("could not write to the bridge: {e}\n{}", self.stderr()),
        }
    }

    /// Takes the next message from the bridge, filing it as a response or an
    /// event. Returns `false` at `deadline` or when stdout has closed.
    async fn pump(&mut self, deadline: Instant) -> bool {
        if self.stdout_closed {
            return false;
        }
        match timeout_at(deadline, self.inbox.recv()).await {
            Err(_) => false,
            Ok(None) => {
                self.stdout_closed = true;
                false
            }
            Ok(Some(Message::Response { id, outcome })) => {
                assert!(
                    self.responses.insert(id, outcome).is_none(),
                    "request {id} was answered twice"
                );
                true
            }
            Ok(Some(Message::Event { event })) => {
                self.events.push_back(event);
                true
            }
            Ok(Some(other)) => panic!("the bridge sent a request: {other:?}"),
        }
    }

    /// The answer to request `id`, if it arrives within `wait`.
    pub async fn outcome_within(&mut self, id: u64, wait: Duration) -> Option<Outcome> {
        let deadline = Instant::now() + wait;
        loop {
            if let Some(outcome) = self.responses.remove(&id) {
                return Some(outcome);
            }
            if !self.pump(deadline).await {
                return self.responses.remove(&id);
            }
        }
    }

    /// The answer to request `id`.
    pub async fn outcome(&mut self, id: u64) -> Outcome {
        match self.outcome_within(id, PATIENCE).await {
            Some(outcome) => outcome,
            None if self.stdout_closed => panic!(
                "the bridge closed stdout without answering request {id}\n{}",
                self.stderr()
            ),
            None => panic!("no answer to request {id}\n{}", self.stderr()),
        }
    }

    /// Sends a request and waits for its answer.
    pub async fn call(&mut self, call: Call) -> Outcome {
        let id = self.send(call).await;
        self.outcome(id).await
    }

    /// Sends a request that must succeed and returns the reply.
    pub async fn ok(&mut self, call: Call) -> Reply {
        let described = format!("{call:?}");
        match self.call(call).await {
            Outcome::Ok(reply) => reply,
            Outcome::Err(e) => panic!("{described} failed: {e:?}"),
        }
    }

    /// `hello`, as the extension sends it first.
    pub async fn hello(&mut self) -> HelloReply {
        match self.ok(hello_call()).await {
            Reply::Hello(reply) => reply,
            other => panic!("hello got {other:?}"),
        }
    }

    /// `add_download`.
    pub async fn add_download(
        &mut self,
        capture: NewDownload,
    ) -> Result<AddedDownload, ProtocolError> {
        let id = self.send(Call::AddDownload(capture)).await;
        added(self.outcome(id).await)
    }

    /// `withdraw_download`.
    pub async fn withdraw_download(
        &mut self,
        handover_id: &str,
    ) -> Result<WithdrawOutcome, ProtocolError> {
        let id = self.send(withdraw_call(handover_id)).await;
        withdrawn(self.outcome(id).await)
    }

    /// `get_extension_settings`.
    pub async fn get_extension_settings(&mut self) -> ExtensionSettings {
        match self.ok(Call::GetExtensionSettings).await {
            Reply::ExtensionSettings(settings) => settings,
            other => panic!("get_extension_settings got {other:?}"),
        }
    }

    /// `update_extension_settings`.
    pub async fn update_extension_settings(
        &mut self,
        patch: ExtensionSettingsPatch,
    ) -> ExtensionSettings {
        match self.ok(Call::UpdateExtensionSettings(patch)).await {
            Reply::ExtensionSettings(settings) => settings,
            other => panic!("update_extension_settings got {other:?}"),
        }
    }

    /// `list_queues`.
    pub async fn list_queues(&mut self) -> Vec<QueueSummary> {
        match self.ok(Call::ListQueues).await {
            Reply::Queues(queues) => queues,
            other => panic!("list_queues got {other:?}"),
        }
    }

    /// `pause_all`.
    pub async fn pause_all(&mut self) {
        assert_eq!(self.ok(Call::PauseAll).await, Reply::Done);
    }

    /// `resume_all`.
    pub async fn resume_all(&mut self) {
        assert_eq!(self.ok(Call::ResumeAll).await, Reply::Done);
    }

    /// Waits for the first event (received earlier or later) that `pick`
    /// accepts, dropping the events before it.
    pub async fn event<T>(&mut self, what: &str, mut pick: impl FnMut(&Event) -> Option<T>) -> T {
        let deadline = Instant::now() + PATIENCE;
        loop {
            while let Some(event) = self.events.pop_front() {
                if let Some(found) = pick(&event) {
                    return found;
                }
            }
            if !self.pump(deadline).await {
                panic!("no {what} event arrived\n{}", self.stderr());
            }
        }
    }

    /// Waits until the bridge closes stdout, and returns the responses that
    /// arrived and were not collected yet, by request id.
    pub async fn wait_stdout_closed(&mut self) -> HashMap<u64, Outcome> {
        let deadline = Instant::now() + PATIENCE;
        while self.pump(deadline).await {}
        assert!(
            self.stdout_closed,
            "the bridge kept stdout open\n{}",
            self.stderr()
        );
        std::mem::take(&mut self.responses)
    }

    /// Waits for the bridge to exit (without closing its stdin).
    pub async fn wait_exit(&mut self) -> ExitStatus {
        timeout(PATIENCE, self.child.wait())
            .await
            .unwrap_or_else(|_| panic!("the bridge did not exit\n{}", self.stderr()))
            .expect("could not wait for the bridge")
    }

    /// Closes the connection the way the browser does (closing stdin) and waits
    /// for the bridge to exit.
    pub async fn close(mut self) -> ExitStatus {
        drop(self.stdin.take());
        self.wait_exit().await
    }
}

/// The `hello` call the extension sends.
pub fn hello_call() -> Call {
    Call::Hello(Hello {
        client: ClientKind::Extension,
        client_version: "0.1.0-e2e".into(),
        protocol_version: PROTOCOL_VERSION,
        browser: Some(BrowserInfo {
            name: "chrome".into(),
            version: Some("140".into()),
        }),
        start_app: true,
    })
}

/// A `withdraw_download` call.
pub fn withdraw_call(handover_id: &str) -> Call {
    Call::WithdrawDownload(WithdrawRequest {
        handover_id: handover_id.to_string(),
    })
}

/// The answer to `add_download`, typed.
pub fn added(outcome: Outcome) -> Result<AddedDownload, ProtocolError> {
    match outcome {
        Outcome::Ok(Reply::DownloadAdded(added)) => Ok(added),
        Outcome::Ok(other) => panic!("add_download got {other:?}"),
        Outcome::Err(e) => Err(e),
    }
}

/// The answer to `withdraw_download`, typed.
pub fn withdrawn(outcome: Outcome) -> Result<WithdrawOutcome, ProtocolError> {
    match outcome {
        Outcome::Ok(Reply::Withdrawn(outcome)) => Ok(outcome),
        Outcome::Ok(other) => panic!("withdraw_download got {other:?}"),
        Outcome::Err(e) => Err(e),
    }
}

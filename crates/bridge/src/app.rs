//! Reaching the app: find its endpoint, connect, and start the app when it is not
//! running.

use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use tokio::time::{Instant, sleep_until, timeout_at};
use wirecatch_ipc::{Connection, Endpoint, IpcError};
use wirecatch_protocol::wire::ErrorCode;

/// How long the bridge waits for the app, from the first connection attempt to
/// giving up, including the time the app needs to start.
pub(crate) const START_BUDGET: Duration = Duration::from_secs(5);

/// First and largest pause between connection attempts while the app starts.
const FIRST_RETRY: Duration = Duration::from_millis(20);
const MAX_RETRY: Duration = Duration::from_millis(250);

/// File name of the desktop app executable, which is installed in the same
/// folder as the bridge (D19).
pub(crate) use wirecatch_product::APP_FILE_NAME;

/// Argument that starts the app without opening its window.
pub(crate) const BACKGROUND_ARG: &str = "--background";

/// Overrides used by the end-to-end tests, present only when the non-default
/// `test-overrides` cargo feature is enabled (which `tests/bridge.rs` requires).
///
/// - `WIRECATCH_BRIDGE_TEST_ENDPOINT`: connect to this pipe name or socket path
///   instead of the per-user endpoint.
/// - `WIRECATCH_BRIDGE_TEST_LAUNCH`: a JSON array `[program, args...]` started
///   instead of the app.
///
/// Shipped builds must not enable the feature, so they always talk to the real
/// per-user endpoint and start the real app. (The connector still checks that
/// whatever it reaches belongs to the current user either way.)
#[cfg(feature = "test-overrides")]
pub(crate) mod test_overrides {
    /// See the module documentation.
    pub(crate) const ENDPOINT_VAR: &str = "WIRECATCH_BRIDGE_TEST_ENDPOINT";
    /// See the module documentation.
    pub(crate) const LAUNCH_VAR: &str = "WIRECATCH_BRIDGE_TEST_LAUNCH";
}

/// The endpoint to connect to.
pub(crate) fn endpoint() -> Result<Endpoint, IpcError> {
    #[cfg(feature = "test-overrides")]
    if let Some(raw) = std::env::var_os(test_overrides::ENDPOINT_VAR) {
        return Ok(Endpoint::from_raw(raw));
    }
    wirecatch_ipc::endpoint()
}

/// The launcher for the app.
pub(crate) fn launcher() -> anyhow::Result<ProcessLauncher> {
    #[cfg(feature = "test-overrides")]
    if let Ok(raw) = std::env::var(test_overrides::LAUNCH_VAR) {
        let mut parts: Vec<String> = serde_json::from_str(&raw)?;
        if parts.is_empty() {
            anyhow::bail!("{} is empty", test_overrides::LAUNCH_VAR);
        }
        let program = PathBuf::from(parts.remove(0));
        return Ok(ProcessLauncher {
            program,
            args: parts.into_iter().map(OsString::from).collect(),
        });
    }
    let bridge = std::env::current_exe()?;
    let app = app_path(&bridge).ok_or_else(|| {
        anyhow::anyhow!(
            "The bridge at {} has no containing folder",
            bridge.display()
        )
    })?;
    Ok(ProcessLauncher::for_app(app))
}

/// Where the app is: [`APP_FILE_NAME`] in the folder that holds the bridge
/// executable. Installers put both in the same folder (on Windows the install
/// folder, on Linux the package's private binary folder), so this needs no
/// search path and cannot be redirected by `PATH` or the working folder.
pub(crate) fn app_path(bridge_exe: &Path) -> Option<PathBuf> {
    bridge_exe
        .parent()
        .filter(|dir| !dir.as_os_str().is_empty())
        .map(|dir| dir.join(APP_FILE_NAME))
}

/// Starts the app.
pub(crate) trait Launcher {
    /// Starts the app without waiting for it. An error means it could not be
    /// started at all.
    fn launch(&self) -> io::Result<()>;

    /// Describes what is started, for the log.
    fn describe(&self) -> String;
}

/// Starts a program as a detached background process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProcessLauncher {
    pub(crate) program: PathBuf,
    pub(crate) args: Vec<OsString>,
}

impl ProcessLauncher {
    /// Starts `app --background`.
    pub(crate) fn for_app(app: PathBuf) -> Self {
        Self {
            program: app,
            args: vec![OsString::from(BACKGROUND_ARG)],
        }
    }
}

impl Launcher for ProcessLauncher {
    fn launch(&self) -> io::Result<()> {
        spawn_detached(&self.program, &self.args)
    }

    fn describe(&self) -> String {
        self.program.display().to_string()
    }
}

/// Starts `program` so that it outlives the bridge and shares nothing with the
/// browser: standard streams go to the null device, it gets its own session (Linux)
/// or process group (Windows), and on Windows it leaves the browser's job object
/// when allowed (Firefox kills the whole job when it closes the host).
///
/// On Windows it is started as a child of the desktop shell when possible (see
/// [`crate::shell_parent`]), so Task Manager lists it as Wirecatch rather than
/// inside the browser.
fn spawn_detached(program: &Path, args: &[OsString]) -> io::Result<()> {
    #[cfg(windows)]
    {
        let dir = program.parent().filter(|d| !d.as_os_str().is_empty());
        match crate::shell_parent::spawn(program, args, dir) {
            Ok(_) => return Ok(()),
            Err(e) => log!(
                "could not start Wirecatch from the desktop shell ({e}); starting it from here instead, so Task Manager may list it inside the browser"
            ),
        }
    }
    let mut command = Command::new(program);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if let Some(dir) = program.parent().filter(|d| !d.as_os_str().is_empty()) {
        command.current_dir(dir);
    }

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        use windows_sys::Win32::Foundation::ERROR_ACCESS_DENIED;
        use windows_sys::Win32::System::Threading::{
            CREATE_BREAKAWAY_FROM_JOB, CREATE_NEW_PROCESS_GROUP, DETACHED_PROCESS,
        };
        let base = DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP;
        command.creation_flags(base | CREATE_BREAKAWAY_FROM_JOB);
        match command.spawn() {
            Ok(_child) => return Ok(()),
            Err(e) if e.raw_os_error() == Some(ERROR_ACCESS_DENIED.cast_signed()) => {
                log!(
                    "the browser does not let the programs it starts leave its job, so Wirecatch is being started inside it. The browser may close Wirecatch when it closes the extension's connection; if that happens, open Wirecatch yourself (or let it start with the computer) so it keeps running"
                );
                command.creation_flags(base);
            }
            Err(e) => return Err(e),
        }
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // SAFETY: the closure runs in the child between fork and exec, and only
        // calls setsid, which is async-signal-safe. A new session detaches the app
        // from the browser's session and process group, so signals aimed at the
        // browser's group (or a hang-up of its terminal) do not reach it.
        unsafe {
            command.pre_exec(|| {
                if libc::setsid() == -1 {
                    return Err(io::Error::last_os_error());
                }
                Ok(())
            });
        }
    }
    // Dropping the handle does not stop the process.
    command.spawn().map(drop)
}

/// Why the app could not be reached, as told to the user through the extension.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Unavailable {
    /// The code the extension acts on.
    pub(crate) code: ErrorCode,
    /// Plain-language text for the extension to show.
    pub(crate) message: String,
}

impl Unavailable {
    fn new(message: impl Into<String>) -> Self {
        Self {
            code: ErrorCode::AppUnavailable,
            message: message.into(),
        }
    }

    /// The app is not running, and the extension asked not to start it: the person closed it and has not asked for it since.
    pub(crate) fn not_running() -> Self {
        Self {
            code: ErrorCode::AppNotRunning,
            message: "Wirecatch is closed. It opens again when the browser has a download for it, or when you open it.".to_owned(),
        }
    }

    /// The connection to the app ended during the session.
    pub(crate) fn lost() -> Self {
        Self::new(
            "The connection to Wirecatch was closed. Try again; Wirecatch will be started if it is not running.",
        )
    }

    /// Starting the app failed outright.
    pub(crate) fn launch_failed(error: &io::Error) -> Self {
        if error.kind() == io::ErrorKind::NotFound {
            Self::new(
                "Wirecatch is not running, and the browser bridge could not find it to start it. Reinstall Wirecatch, or open it yourself, then try again.",
            )
        } else {
            Self::new(format!(
                "Wirecatch is not running and could not be started ({error}). Open Wirecatch yourself, then try again."
            ))
        }
    }

    /// The bridge could not work out how to start the app.
    pub(crate) fn cannot_start(reason: &str) -> Self {
        Self::new(format!(
            "Wirecatch is not running and the browser bridge cannot start it ({reason}). Open Wirecatch yourself, then try again."
        ))
    }

    /// The app was started but did not start listening in time.
    pub(crate) fn timed_out(budget: Duration) -> Self {
        Self::new(format!(
            "Wirecatch was started but did not respond within {} seconds. Wait a moment, then try again. If this keeps happening, open Wirecatch yourself.",
            budget.as_secs()
        ))
    }

    /// Connecting failed for a reason other than "not running".
    pub(crate) fn from_ipc(error: &IpcError) -> Self {
        match error {
            IpcError::WrongOwner { .. } | IpcError::EndpointTaken { .. } => Self::new(
                "Wirecatch's connection point belongs to another user account on this computer, so nothing was sent to it. Another signed-in user may be trying to intercept your downloads. Ask the other users to sign out (or restart the computer), then open Wirecatch again.",
            ),
            IpcError::UnsafeDirectory { .. } => Self::new(format!(
                "Wirecatch could not be reached safely, so nothing was sent to it. {error}"
            )),
            other => Self::new(format!(
                "Wirecatch could not be reached ({other}). Open Wirecatch, then try again."
            )),
        }
    }
}

/// Whether a connection may start the app.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Start {
    /// Start it when nothing is listening: the person asked for something that needs it.
    IfNeeded,
    /// Only connect to a running app: a check the extension makes on its own.
    Never,
}

/// Connects to the app, starting it if nothing is listening and `start` allows it, and gives up after `budget`.
///
/// A server owned by another user (or any error other than "not running") is
/// never retried and never causes a launch.
pub(crate) async fn connect_or_launch(
    endpoint: &Endpoint,
    launcher: &dyn Launcher,
    budget: Duration,
    start: Start,
) -> Result<Connection, Unavailable> {
    let deadline = Instant::now() + budget;
    match attempt(endpoint, deadline, budget).await {
        Attempt::Connected(conn) => return Ok(conn),
        Attempt::NotRunning if start == Start::Never => return Err(Unavailable::not_running()),
        Attempt::NotRunning => {}
        Attempt::Failed(unavailable) => return Err(unavailable),
    }

    if let Err(e) = launcher.launch() {
        log!("could not start Wirecatch ({}): {e}", launcher.describe());
        return Err(Unavailable::launch_failed(&e));
    }

    // Nothing signals "the app is now listening", so poll with backoff.
    let mut pause = FIRST_RETRY;
    loop {
        if Instant::now() >= deadline {
            log!("Wirecatch did not start listening within {budget:?}");
            return Err(Unavailable::timed_out(budget));
        }
        sleep_until((Instant::now() + pause).min(deadline)).await;
        pause = (pause * 2).min(MAX_RETRY);
        match attempt(endpoint, deadline, budget).await {
            Attempt::Connected(conn) => return Ok(conn),
            Attempt::NotRunning => {}
            Attempt::Failed(unavailable) => return Err(unavailable),
        }
    }
}

enum Attempt {
    Connected(Connection),
    NotRunning,
    Failed(Unavailable),
}

async fn attempt(endpoint: &Endpoint, deadline: Instant, budget: Duration) -> Attempt {
    match timeout_at(deadline, wirecatch_ipc::connect(endpoint)).await {
        Ok(Ok(conn)) => Attempt::Connected(conn),
        Ok(Err(IpcError::NotRunning { .. })) => Attempt::NotRunning,
        Ok(Err(e)) => {
            log!("could not connect to Wirecatch: {e}");
            Attempt::Failed(Unavailable::from_ipc(&e))
        }
        Err(_) => Attempt::Failed(Unavailable::timed_out(budget)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use wirecatch_ipc::Listener;
    use wirecatch_test_support::{PrivateDir, private_dir};

    #[test]
    fn the_app_is_found_next_to_the_bridge() {
        let bridge = Path::new("install").join("dir").join("wirecatch-bridge");
        assert_eq!(
            app_path(&bridge),
            Some(Path::new("install").join("dir").join(APP_FILE_NAME))
        );
        assert_eq!(app_path(Path::new("wirecatch-bridge")), None);
    }

    #[test]
    fn the_app_is_started_in_the_background() {
        let launcher = ProcessLauncher::for_app(PathBuf::from("app"));
        assert_eq!(launcher.args, [OsString::from("--background")]);
    }

    /// A fresh endpoint nobody else uses, in a private folder of its own that
    /// the returned guard removes.
    fn test_endpoint() -> (Endpoint, PrivateDir) {
        let dir = private_dir("bridge-unit").unwrap();
        (Endpoint::from_raw(dir.endpoint_name()), dir)
    }

    /// Starts an in-process "app" after a delay, the way a real app takes a moment
    /// to come up.
    struct DelayedApp {
        endpoint: Endpoint,
        delay: Duration,
        launches: Arc<Mutex<u32>>,
    }

    impl Launcher for DelayedApp {
        fn launch(&self) -> io::Result<()> {
            *self.launches.lock().unwrap() += 1;
            let endpoint = self.endpoint.clone();
            let delay = self.delay;
            tokio::spawn(async move {
                tokio::time::sleep(delay).await;
                let mut listener = Listener::bind(&endpoint).unwrap();
                let mut conn = listener.accept().await.unwrap();
                let mut byte = [0u8; 1];
                conn.read_exact(&mut byte).await.unwrap();
                conn.write_all(&byte).await.unwrap();
            });
            Ok(())
        }

        fn describe(&self) -> String {
            "in-process test app".into()
        }
    }

    struct FailingLauncher;
    impl Launcher for FailingLauncher {
        fn launch(&self) -> io::Result<()> {
            Err(io::Error::from(io::ErrorKind::NotFound))
        }
        fn describe(&self) -> String {
            "missing app".into()
        }
    }

    #[tokio::test]
    async fn starts_the_app_and_connects_once_it_listens() {
        let (endpoint, _dir) = test_endpoint();
        let launches = Arc::new(Mutex::new(0));
        let app = DelayedApp {
            endpoint: endpoint.clone(),
            delay: Duration::from_millis(400),
            launches: launches.clone(),
        };
        let started = Instant::now();
        let mut conn = connect_or_launch(&endpoint, &app, START_BUDGET, Start::IfNeeded)
            .await
            .unwrap();
        assert!(started.elapsed() < START_BUDGET);
        assert_eq!(*launches.lock().unwrap(), 1);
        conn.write_all(b"x").await.unwrap();
        let mut byte = [0u8; 1];
        conn.read_exact(&mut byte).await.unwrap();
        assert_eq!(&byte, b"x");
    }

    // A Wirecatch the person closed used to come back within a second: the extension reconnected and
    // every new bridge started the app. A check that may not start it finds it closed and leaves it so.
    #[tokio::test]
    async fn a_closed_app_is_not_started_when_starting_is_not_allowed() {
        let (endpoint, _dir) = test_endpoint();
        let launches = Arc::new(Mutex::new(0));
        let app = DelayedApp {
            endpoint: endpoint.clone(),
            delay: Duration::ZERO,
            launches: launches.clone(),
        };

        let refused = connect_or_launch(&endpoint, &app, START_BUDGET, Start::Never).await;

        assert_eq!(refused.err(), Some(Unavailable::not_running()));
        assert_eq!(*launches.lock().unwrap(), 0, "nothing was started");
    }

    #[tokio::test]
    async fn a_running_app_is_reached_even_when_starting_is_not_allowed() {
        let (endpoint, _dir) = test_endpoint();
        let _listener = Listener::bind(&endpoint).unwrap();
        let launches = Arc::new(Mutex::new(0));
        let app = DelayedApp {
            endpoint: endpoint.clone(),
            delay: Duration::ZERO,
            launches: launches.clone(),
        };

        assert!(
            connect_or_launch(&endpoint, &app, START_BUDGET, Start::Never)
                .await
                .is_ok()
        );
        assert_eq!(*launches.lock().unwrap(), 0);
    }

    #[tokio::test]
    async fn a_running_app_is_not_started_again() {
        let (endpoint, _dir) = test_endpoint();
        let _listener = Listener::bind(&endpoint).unwrap();
        let launches = Arc::new(Mutex::new(0));
        let app = DelayedApp {
            endpoint: endpoint.clone(),
            delay: Duration::ZERO,
            launches: launches.clone(),
        };
        connect_or_launch(&endpoint, &app, START_BUDGET, Start::IfNeeded)
            .await
            .unwrap();
        assert_eq!(*launches.lock().unwrap(), 0);
    }

    #[tokio::test]
    async fn gives_up_when_the_app_cannot_be_started() {
        let (endpoint, _dir) = test_endpoint();
        let started = Instant::now();
        let err = connect_or_launch(&endpoint, &FailingLauncher, START_BUDGET, Start::IfNeeded)
            .await
            .unwrap_err();
        // FailingLauncher reports NotFound: the advice is to reinstall.
        assert!(err.message.contains("could not find it"), "{err:?}");
        assert!(err.message.contains("Reinstall Wirecatch"), "{err:?}");
        assert!(started.elapsed() < Duration::from_secs(1), "fails fast");
    }

    #[test]
    fn messages_name_the_cause_and_the_fix() {
        let denied = io::Error::from(io::ErrorKind::PermissionDenied);
        assert!(
            Unavailable::launch_failed(&denied)
                .message
                .contains("Open Wirecatch yourself")
        );
        let squatted = IpcError::WrongOwner {
            endpoint: "pipe".into(),
            expected: "me".into(),
            actual: "them".into(),
        };
        let message = Unavailable::from_ipc(&squatted).message;
        assert!(message.contains("another user account"), "{message}");
        assert!(!message.contains("Restart the computer, open"), "{message}");
        assert!(
            Unavailable::timed_out(START_BUDGET)
                .message
                .contains("within 5 seconds")
        );
    }

    #[tokio::test]
    async fn gives_up_when_the_app_never_listens() {
        let (endpoint, _dir) = test_endpoint();
        let app = DelayedApp {
            endpoint: endpoint.clone(),
            delay: Duration::from_secs(30),
            launches: Arc::new(Mutex::new(0)),
        };
        let budget = Duration::from_millis(600);
        let started = Instant::now();
        let err = connect_or_launch(&endpoint, &app, budget, Start::IfNeeded)
            .await
            .unwrap_err();
        assert_eq!(err, Unavailable::timed_out(budget));
        let waited = started.elapsed();
        assert!(
            waited >= budget && waited < budget + Duration::from_millis(500),
            "{waited:?}"
        );
    }
}

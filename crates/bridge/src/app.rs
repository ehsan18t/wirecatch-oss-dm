//! Reaching the app: find its endpoint, connect, and start the app when it is not
//! running.

use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use tokio::time::{Instant, sleep_until, timeout_at};
use wirecatch_ipc::{Connection, Endpoint, IpcError};

/// How long the bridge waits for the app, from the first connection attempt to
/// giving up, including the time the app needs to start.
pub(crate) const START_BUDGET: Duration = Duration::from_secs(5);

/// First and largest pause between connection attempts while the app starts.
const FIRST_RETRY: Duration = Duration::from_millis(20);
const MAX_RETRY: Duration = Duration::from_millis(250);

/// File name of the desktop app executable, which is installed in the same
/// folder as the bridge.
pub(crate) const APP_FILE_NAME: &str = if cfg!(windows) {
    "wirecatch-app.exe"
} else {
    "wirecatch-app"
};

/// Argument that starts the app without opening its window.
pub(crate) const BACKGROUND_ARG: &str = "--background";

/// Debug builds only: overrides used by the integration tests, never present in
/// release builds.
///
/// - `WIRECATCH_BRIDGE_TEST_ENDPOINT`: connect to this pipe name or socket path
///   instead of the per-user endpoint.
/// - `WIRECATCH_BRIDGE_TEST_LAUNCH`: a JSON array `[program, args...]` started
///   instead of the app.
///
/// They are compiled out of release builds so a shipped bridge always talks to
/// the real per-user endpoint and starts the real app. (The connector still checks
/// that whatever it reaches belongs to the current user either way.)
#[cfg(debug_assertions)]
pub(crate) mod test_overrides {
    /// See the module documentation.
    pub(crate) const ENDPOINT_VAR: &str = "WIRECATCH_BRIDGE_TEST_ENDPOINT";
    /// See the module documentation.
    pub(crate) const LAUNCH_VAR: &str = "WIRECATCH_BRIDGE_TEST_LAUNCH";
}

/// The endpoint to connect to.
pub(crate) fn endpoint() -> Result<Endpoint, IpcError> {
    #[cfg(debug_assertions)]
    if let Some(raw) = std::env::var_os(test_overrides::ENDPOINT_VAR) {
        return Ok(Endpoint::from_raw(raw));
    }
    wirecatch_ipc::endpoint()
}

/// The launcher for the app.
pub(crate) fn launcher() -> anyhow::Result<ProcessLauncher> {
    #[cfg(debug_assertions)]
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
/// browser: standard streams go to the null device, it runs in its own process
/// group, and on Windows it leaves the browser's job object when allowed (Firefox
/// kills the whole job when it closes the host).
fn spawn_detached(program: &Path, args: &[OsString]) -> io::Result<()> {
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
            // The job does not allow breaking away; start it inside the job.
            Err(e) if e.raw_os_error() == Some(ERROR_ACCESS_DENIED as i32) => {
                command.creation_flags(base);
            }
            Err(e) => return Err(e),
        }
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    // Dropping the handle does not stop the process.
    command.spawn().map(drop)
}

/// Why the app could not be reached, as told to the user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Unavailable {
    /// Plain-language text for the extension to show.
    pub(crate) message: &'static str,
}

impl Unavailable {
    /// The app is not running and could not be started.
    pub(crate) const NOT_STARTED: Self = Self {
        message: "Wirecatch is not running and could not be started. Open Wirecatch, then try again.",
    };
    /// The connection to the app ended.
    pub(crate) const LOST: Self = Self {
        message: "The connection to Wirecatch was lost. Open Wirecatch, then try again.",
    };
    /// The endpoint belongs to someone else, or could not be checked.
    pub(crate) const REFUSED: Self = Self {
        message: "Wirecatch could not be reached safely, so nothing was sent to it. Restart the computer, open Wirecatch, then try again.",
    };
}

/// Connects to the app, starting it if nothing is listening, and gives up after
/// `budget`.
///
/// A server owned by another user (or any error other than "not running") is
/// never retried and never causes a launch.
pub(crate) async fn connect_or_launch(
    endpoint: &Endpoint,
    launcher: &dyn Launcher,
    budget: Duration,
) -> Result<Connection, Unavailable> {
    let deadline = Instant::now() + budget;
    match attempt(endpoint, deadline).await {
        Attempt::Connected(conn) => return Ok(conn),
        Attempt::NotRunning => {}
        Attempt::Failed(unavailable) => return Err(unavailable),
    }

    if let Err(e) = launcher.launch() {
        log!("could not start Wirecatch ({}): {e}", launcher.describe());
        return Err(Unavailable::NOT_STARTED);
    }

    // Nothing signals "the app is now listening", so poll with backoff.
    let mut pause = FIRST_RETRY;
    loop {
        if Instant::now() >= deadline {
            log!("Wirecatch did not start listening within {budget:?}");
            return Err(Unavailable::NOT_STARTED);
        }
        sleep_until((Instant::now() + pause).min(deadline)).await;
        pause = (pause * 2).min(MAX_RETRY);
        match attempt(endpoint, deadline).await {
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

async fn attempt(endpoint: &Endpoint, deadline: Instant) -> Attempt {
    match timeout_at(deadline, wirecatch_ipc::connect(endpoint)).await {
        Ok(Ok(conn)) => Attempt::Connected(conn),
        Ok(Err(IpcError::NotRunning { .. })) => Attempt::NotRunning,
        Ok(Err(e @ IpcError::WrongOwner { .. })) => {
            log!("{e}");
            Attempt::Failed(Unavailable::REFUSED)
        }
        Ok(Err(e)) => {
            log!("could not connect to Wirecatch: {e}");
            Attempt::Failed(Unavailable::REFUSED)
        }
        Err(_) => Attempt::Failed(Unavailable::NOT_STARTED),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::{Arc, Mutex};

    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use wirecatch_ipc::Listener;

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

    fn test_endpoint() -> (Endpoint, TestDir) {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let tag = format!(
            "{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        );
        #[cfg(windows)]
        {
            (
                Endpoint::from_raw(format!(r"\\.\pipe\wirecatch-bridge-unit-{tag}")),
                TestDir(None),
            )
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
            let dir = std::env::temp_dir().join(format!("wirecatch-bridge-unit-{tag}"));
            std::fs::DirBuilder::new().mode(0o700).create(&dir).unwrap();
            std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
            (Endpoint::from_raw(dir.join("app.sock")), TestDir(Some(dir)))
        }
    }

    struct TestDir(Option<PathBuf>);
    impl Drop for TestDir {
        fn drop(&mut self) {
            if let Some(dir) = &self.0 {
                let _ = std::fs::remove_dir_all(dir);
            }
        }
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
        let mut conn = connect_or_launch(&endpoint, &app, START_BUDGET)
            .await
            .unwrap();
        assert!(started.elapsed() < START_BUDGET);
        assert_eq!(*launches.lock().unwrap(), 1);
        conn.write_all(b"x").await.unwrap();
        let mut byte = [0u8; 1];
        conn.read_exact(&mut byte).await.unwrap();
        assert_eq!(&byte, b"x");
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
        connect_or_launch(&endpoint, &app, START_BUDGET)
            .await
            .unwrap();
        assert_eq!(*launches.lock().unwrap(), 0);
    }

    #[tokio::test]
    async fn gives_up_when_the_app_cannot_be_started() {
        let (endpoint, _dir) = test_endpoint();
        let started = Instant::now();
        let err = connect_or_launch(&endpoint, &FailingLauncher, START_BUDGET)
            .await
            .unwrap_err();
        assert_eq!(err, Unavailable::NOT_STARTED);
        assert!(started.elapsed() < Duration::from_secs(1), "fails fast");
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
        let err = connect_or_launch(&endpoint, &app, budget)
            .await
            .unwrap_err();
        assert_eq!(err, Unavailable::NOT_STARTED);
        let waited = started.elapsed();
        assert!(
            waited >= budget && waited < budget + Duration::from_millis(500),
            "{waited:?}"
        );
    }
}

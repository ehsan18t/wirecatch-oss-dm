//! The download manager behind every window: opened on a Tokio runtime of its own, since GPUI's
//! executor is not Tokio's, and reached from the interface through [`Backend::run`].

use std::future::Future;
use std::sync::Arc;

use gpui_kit::component::WindowExt as _;
use gpui_kit::{AnyWindowHandle, App, Global};
use wirecatch_core::{Manager, ManagerConfig};
use wirecatch_protocol::wire::ProtocolError;
use wirecatch_shell::host::{self, Core, Unopened};

/// The manager this process hosts and the runtime its work runs on.
pub struct Backend {
    runtime: tokio::runtime::Runtime,
    core: Arc<Core>,
}

impl Global for Backend {}

impl Backend {
    /// Starts the runtime and begins opening the manager in the background, so the first window
    /// never waits on the database.
    ///
    /// # Errors
    ///
    /// The runtime could not start: the system refused threads.
    pub fn start() -> std::io::Result<Self> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .thread_name("wirecatch-core")
            .build()?;
        let core = Arc::new(Core::new(config()));
        let opening = Arc::clone(&core);
        runtime.spawn(async move { opening.opened().await.map(|_| ()).map_err(Unopened::clone) });
        Ok(Self { runtime, core })
    }

    /// Runs `work` with the manager on the runtime, once it has opened; its result comes back
    /// to whichever executor awaits the handle.
    pub fn run<T, F, Fut>(cx: &App, work: F) -> tokio::task::JoinHandle<Result<T, Unopened>>
    where
        T: Send + 'static,
        F: FnOnce(Manager) -> Fut + Send + 'static,
        Fut: Future<Output = T> + Send + 'static,
    {
        let backend = cx.global::<Self>();
        let core = Arc::clone(&backend.core);
        backend.runtime.spawn(async move {
            let manager = core.opened().await.map_err(Unopened::clone)?.clone();
            Ok(work(manager).await)
        })
    }
}

/// Runs a command on the manager; a refusal shows as a notification in `window`.
pub fn command<F, Fut>(window: AnyWindowHandle, cx: &mut App, work: F)
where
    F: FnOnce(Manager) -> Fut + Send + 'static,
    Fut: Future<Output = Result<(), ProtocolError>> + Send + 'static,
{
    let done = Backend::run(cx, work);
    cx.spawn(async move |cx| {
        let refused = match done.await {
            Ok(Ok(Ok(()))) => return,
            Ok(Ok(Err(error))) => error.message,
            Ok(Err(unopened)) => unopened.reason.message,
            Err(e) => {
                tracing::error!("a command stopped unexpectedly: {e}");
                return;
            }
        };
        _ = window.update(cx, |_, window, cx| window.push_notification(refused, cx));
    })
    .detach();
}

/// [`command`] for work that does not wait.
pub fn act(
    window: AnyWindowHandle,
    cx: &mut App,
    work: impl FnOnce(&Manager) -> Result<(), ProtocolError> + Send + 'static,
) {
    command(window, cx, move |manager| async move { work(&manager) });
}

/// Where the manager keeps its data and which folders it saves to.
fn config() -> ManagerConfig {
    let local = dirs::data_local_dir().unwrap_or_default();
    let roaming = dirs::data_dir().unwrap_or_else(|| local.clone());
    let data_dir = host::data_dir(&local, &roaming);
    let home = dirs::home_dir().unwrap_or_else(|| data_dir.clone());
    ManagerConfig::new(data_dir, host::known_folders(&home)).with_official_suffix_list()
}

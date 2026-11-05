//! The download manager behind every window: opened on a Tokio runtime of its own, since GPUI's
//! executor is not Tokio's, and reached from the interface through [`Backend::run`]. Once it
//! opens, it also serves the browser bridge.

use std::future::Future;
use std::sync::Arc;

use gpui_kit::component::WindowExt as _;
use gpui_kit::{AnyWindowHandle, App, Global};
use wirecatch_core::{Manager, ManagerConfig};
use wirecatch_ipc::Listener;
use wirecatch_protocol::wire::ProtocolError;
use wirecatch_shell::host::{self, Core, Unopened};
use wirecatch_shell::launch::Launch;
use wirecatch_shell::{bridge, quit};

/// The manager this process hosts and the runtime its work runs on.
pub struct Backend {
    runtime: tokio::runtime::Runtime,
    core: Arc<Core>,
    /// Serves the browser, and holds the endpoint a second launch looks for.
    serving: Option<tokio::task::JoinHandle<()>>,
}

impl Global for Backend {}

impl Backend {
    /// The runtime the manager's work runs on.
    ///
    /// # Errors
    ///
    /// The system refused threads.
    pub fn runtime() -> std::io::Result<tokio::runtime::Runtime> {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .thread_name("wirecatch-core")
            .build()
    }

    /// Begins opening the manager in the background, so the first window never waits on the
    /// data folder or the database, then serves the browser on `listener`; registers the bridge
    /// with the browsers.
    pub fn start(
        runtime: tokio::runtime::Runtime,
        listener: Option<Listener>,
        launch: Launch,
    ) -> Self {
        let core = Arc::new(Core::preparing(|| {
            let config = config();
            bridge::register_on_start(&config.data_dir);
            config
        }));
        let serving = runtime.spawn(serve(Arc::clone(&core), listener, launch));
        Self {
            runtime,
            core,
            serving: Some(serving),
        }
    }

    /// A backend whose manager never opens, for forms that only read `now`.
    #[cfg(test)]
    pub fn unopened() -> Self {
        Self {
            runtime: Self::runtime().expect("a test runtime"),
            core: Arc::new(Core::preparing(config)),
            serving: None,
        }
    }

    /// Stops serving the browser; the returned future ends once the endpoint is free for a new
    /// copy of the app to claim.
    pub fn free_endpoint(cx: &mut App) -> impl Future<Output = ()> + use<> {
        let serving = cx.global_mut::<Self>().serving.take();
        async move {
            if let Some(serving) = serving {
                serving.abort();
                // Ends once the task, and the listener it holds, is dropped.
                _ = serving.await;
            }
        }
    }

    /// The manager if it has opened, and the runtime its work runs on, for work that cannot
    /// wait for it.
    pub fn now(cx: &App) -> (Option<Manager>, tokio::runtime::Handle) {
        let backend = cx.global::<Self>();
        (
            backend.core.get().cloned(),
            backend.runtime.handle().clone(),
        )
    }

    /// Stops the manager so every download continues next time, waiting for one still opening.
    pub fn stop(cx: &App) -> tokio::task::JoinHandle<()> {
        let backend = cx.global::<Self>();
        let core = Arc::clone(&backend.core);
        backend
            .runtime
            .spawn(async move { quit::stop(&core).await })
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
            let manager = core.opened().await?.clone();
            Ok(work(manager).await)
        })
    }
}

/// Runs `work` once the manager has opened, however late; this alone does not open it.
pub fn once_opened<F, Fut>(cx: &App, work: F)
where
    F: FnOnce(Manager) -> Fut + Send + 'static,
    Fut: Future<Output = ()> + Send + 'static,
{
    let backend = cx.global::<Backend>();
    let core = Arc::clone(&backend.core);
    // Ends with the runtime, as the app quits.
    drop(backend.runtime.spawn(async move {
        let manager = core.when_opened().await.clone();
        work(manager).await;
    }));
}

/// Lets the next use of the manager open it again, after an open failed.
pub fn try_opening_again(cx: &App) {
    cx.global::<Backend>().core.try_again();
}

/// What a [`Backend::run`] of a command comes to.
pub type Done<T> = Result<Result<Result<T, ProtocolError>, Unopened>, tokio::task::JoinError>;

/// What the manager answered, or why it could not; `None` when the work stopped before it could.
pub fn answer<T>(done: Done<T>) -> Option<Result<T, ProtocolError>> {
    match done {
        Ok(Ok(answered)) => Some(answered),
        Ok(Err(unopened)) => Some(Err(unopened.reason)),
        Err(e) => {
            tracing::error!("work on the manager stopped unexpectedly: {e}");
            None
        }
    }
}

/// [`answer`], with a refusal in the words to show.
pub fn said<T>(done: Done<T>) -> Option<Result<T, String>> {
    answer(done).map(|answered| answered.map_err(|refused| refused.message))
}

/// Runs a command on the manager; a refusal shows as a notification in `window`.
pub fn command<F, Fut>(window: AnyWindowHandle, cx: &mut App, work: F)
where
    F: FnOnce(Manager) -> Fut + Send + 'static,
    Fut: Future<Output = Result<(), ProtocolError>> + Send + 'static,
{
    command_saying(window, cx, work, |()| None);
}

/// [`command`], with what `say` makes of the answer shown as a notification too.
pub fn command_saying<T, F, Fut>(
    window: AnyWindowHandle,
    cx: &mut App,
    work: F,
    say: impl FnOnce(T) -> Option<String> + 'static,
) where
    T: Send + 'static,
    F: FnOnce(Manager) -> Fut + Send + 'static,
    Fut: Future<Output = Result<T, ProtocolError>> + Send + 'static,
{
    let done = Backend::run(cx, work);
    cx.spawn(async move |cx| {
        let told = match said(done.await) {
            Some(Ok(answered)) => say(answered),
            Some(Err(refused)) => Some(refused),
            None => None,
        };
        let Some(refused) = told else {
            return;
        };
        _ = window.update(cx, |_, window, cx| window.push_notification(refused, cx));
    })
    .detach();
}

/// The window to tell something in. None is active while the app is in the background, as when a
/// system dialog has just closed, so any open window does.
#[must_use]
pub fn window_in_front(cx: &App) -> Option<AnyWindowHandle> {
    cx.active_window()
        .or_else(|| cx.windows().into_iter().next())
}

/// [`command`] for work that does not wait.
pub fn act(
    window: AnyWindowHandle,
    cx: &mut App,
    work: impl FnOnce(&Manager) -> Result<(), ProtocolError> + Send + 'static,
) {
    command(window, cx, move |manager| async move { work(&manager) });
}

/// Serves the browser once the manager opens, or tells it why the manager could not open.
async fn serve(core: Arc<Core>, listener: Option<Listener>, launch: Launch) {
    match core.opened().await {
        Ok(manager) => {
            if let Some(listener) = listener {
                bridge::serve(listener, manager.clone()).await;
            }
        }
        // Past the endpoint beside an elevated copy, whose pipe keeps the probe out (D41.8).
        Err(unopened) if unopened.another_copy => {
            tracing::error!("{}; this copy exits", unopened.reason);
            drop(listener);
            _ = tokio::task::spawn_blocking(move || launch.say_already_running()).await;
            std::process::exit(0);
        }
        Err(unopened) => {
            if let Some(listener) = listener {
                bridge::serve_once_opened(&core, listener, unopened.reason.message.clone()).await;
            }
        }
    }
}

/// Where the manager keeps its data and which folders it saves to. Blocks on the disk: it may
/// move the data folder (`host::data_dir`).
fn config() -> ManagerConfig {
    let local = dirs::data_local_dir().unwrap_or_default();
    let roaming = dirs::data_dir().unwrap_or_else(|| local.clone());
    let data_dir = host::data_dir(&local, &roaming);
    let home = dirs::home_dir().unwrap_or_else(|| data_dir.clone());
    ManagerConfig::new(data_dir, host::known_folders(&home))
        .with_official_suffix_list()
        .with_official_app_releases()
        .with_official_proxy_test()
}

#[cfg(test)]
mod tests {
    use super::config;

    #[test]
    fn the_app_names_every_official_online_source() {
        let config = config();
        assert!(config.suffix_list_source.is_some());
        assert!(config.app_releases.is_some());
        assert!(config.proxy_test_url.is_some());
    }
}

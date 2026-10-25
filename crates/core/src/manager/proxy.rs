//! Changing and testing the app's proxy setting (D71).

use wirecatch_protocol::app::DownloadStatus;
use wirecatch_protocol::proxy::{ProxyChange, ProxyTest, ProxyTestResult};

use super::Manager;
use super::state::Stop;
use crate::error::Result;
use crate::proxy::{self, UpdaterProxy};

impl Manager {
    /// Saves a new proxy setting with its passwords, and, when `change` asks, restarts the
    /// downloads that are connecting or downloading so every request follows it at once; they continue their
    /// part files. Downloads that start later follow it in any case. The settings arrive as
    /// `SettingsChanged`, without the passwords.
    ///
    /// # Errors
    ///
    /// [`CoreError::Invalid`](crate::CoreError::Invalid) for a proxy that cannot work, and
    /// [`CoreError::Closed`](crate::CoreError::Closed) once the manager has shut down.
    /// Nothing changes on an error.
    pub fn set_proxy(&self, change: ProxyChange) -> Result<()> {
        let ProxyChange {
            settings,
            passwords,
            restart_running,
        } = change;
        let mut settings = proxy::validate(settings)?;
        let mut state = self.lock_open()?;
        let logins = proxy::merge_passwords(
            &state.proxy_logins,
            &passwords,
            &state.settings.proxy,
            &settings,
        )?;
        proxy::mark_passwords(&mut settings, &logins);
        if logins != state.proxy_logins {
            self.inner.store.save_proxy_logins(logins.clone());
            state.proxy_logins = logins;
        }
        let mut new = state.settings.clone();
        new.proxy = settings;
        self.apply_settings(&mut state, new);
        if restart_running {
            let running: Vec<_> = state.running.iter().copied().collect();
            for id in running {
                // One joining video and audio sends nothing; restarting it would only redo the work.
                if let Some(entry) = state.downloads.get_mut(&id)
                    && matches!(
                        entry.row.status,
                        DownloadStatus::Connecting | DownloadStatus::Downloading
                    )
                {
                    self.stop_run(entry, Stop::Requeue);
                }
            }
            self.reconcile(&mut state);
        }
        Ok(())
    }

    /// Sends one request through the proxy setting `test` describes, before it is saved
    /// (D71.5). A password `test` leaves as it is is the saved one.
    ///
    /// # Errors
    ///
    /// [`CoreError::Invalid`](crate::CoreError::Invalid) for a proxy that cannot work.
    pub async fn test_proxy(&self, test: ProxyTest) -> Result<ProxyTestResult> {
        let settings = proxy::validate(test.settings)?;
        let (saved, was) = {
            let state = self.lock();
            (state.proxy_logins.clone(), state.settings.proxy.clone())
        };
        let logins = proxy::merge_passwords(&saved, &test.passwords, &was, &settings)?;
        let choice = proxy::engine_choice(&settings, &logins);
        let Some(url) = self.inner.proxy_test_url.clone() else {
            return Ok(ProxyTestResult::Unreachable {
                reason: "this build has no address to test the proxy with".into(),
            });
        };
        Ok(proxy::test(choice, &url, self.inner.stopped.child_token()).await)
    }

    /// The proxy the app updater uses: the setting's proxy for `https`.
    #[must_use]
    pub fn updater_proxy(&self) -> UpdaterProxy {
        proxy::updater_proxy(&self.lock().app_proxy())
    }
}

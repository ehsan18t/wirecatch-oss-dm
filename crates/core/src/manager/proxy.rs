//! Proxy servers (D73): saving, testing and deleting them, and Copy from system.

use wirecatch_engine::CancellationToken;
use wirecatch_protocol::app::{Setting, UiEvent};
use wirecatch_protocol::proxy::{
    ProxyServer, ProxyServerDraft, ProxyServerSettings, ProxyTestResult, ProxyTestRun, ProxyUse,
};
use wirecatch_protocol::{ProxyServerId, Secret};
use wirecatch_settings::proxy::{ComputerProxy, SavedProxyServer, UpdaterProxy};

use super::Manager;
use super::state::State;
use crate::error::{CoreError, Result};

impl Manager {
    #[must_use]
    pub fn list_proxy_servers(&self) -> Vec<ProxyServer> {
        self.lock().proxy_server_list()
    }

    /// Saves a new profile with its password and tests it in the background (D73.1); the
    /// profiles arrive as `ProxyServersChanged`, again once the test has run.
    ///
    /// # Errors
    ///
    /// [`CoreError::Invalid`] for a profile that cannot work, [`CoreError::LimitReached`] past
    /// the most profiles there can be, and [`CoreError::Closed`] once the manager has shut down.
    pub fn create_proxy_server(&self, draft: ProxyServerDraft) -> Result<ProxyServerId> {
        let mut state = self.lock_open()?;
        let id = self.add_profile(&mut state, draft)?;
        self.inner.store.save_last_ids(state.ids.last());
        self.proxy_servers_changed(&state);
        drop(state);
        self.test_in_background(id);
        Ok(id)
    }

    fn add_profile(&self, state: &mut State, draft: ProxyServerDraft) -> Result<ProxyServerId> {
        wirecatch_settings::proxy::check_count(state.proxy_servers.len() + 1)?;
        let others: Vec<_> = state.proxy_servers.iter().map(|p| &p.settings).collect();
        let settings = wirecatch_settings::proxy::validate_settings_file(draft.settings, &others)?;
        let password =
            wirecatch_settings::proxy::merge_password(None, &draft.password, None, &settings)?;
        let id = state.ids.proxy_server();
        state.proxy_servers.push(SavedProxyServer {
            id,
            settings,
            last_test: None,
        });
        if let Some(password) = password {
            state.proxy_logins.insert(id, password);
            self.inner
                .store
                .save_proxy_logins(state.proxy_logins.clone());
        }
        Ok(id)
    }

    /// Saves a profile's new settings and password, testing it again in the background when its
    /// server or login changed. Downloads running through it keep the proxy they started with
    /// (D73.3).
    ///
    /// # Errors
    ///
    /// [`CoreError::ProxyServerNotFound`], the errors of
    /// [`create_proxy_server`](Self::create_proxy_server), and nothing changes on an error.
    pub fn update_proxy_server(&self, id: ProxyServerId, draft: ProxyServerDraft) -> Result<()> {
        let mut state = self.lock_open()?;
        let index = profile_index(&state, id)?;
        let others: Vec<_> = state
            .proxy_servers
            .iter()
            .filter(|p| p.id != id)
            .map(|p| &p.settings)
            .collect();
        let settings = wirecatch_settings::proxy::validate_settings_file(draft.settings, &others)?;
        let saved = &state.proxy_servers[index];
        let password = wirecatch_settings::proxy::merge_password(
            state.proxy_logins.get(&id),
            &draft.password,
            Some(&saved.settings),
            &settings,
        )?;
        let retest = !wirecatch_settings::proxy::same_proxy(&saved.settings, &settings)
            || !draft.password.is_keep();
        let profile = &mut state.proxy_servers[index];
        profile.settings = settings;
        if retest {
            profile.last_test = None;
        }
        if password.as_ref() != state.proxy_logins.get(&id) {
            match password {
                Some(password) => state.proxy_logins.insert(id, password),
                None => state.proxy_logins.remove(&id),
            };
            self.inner
                .store
                .save_proxy_logins(state.proxy_logins.clone());
        }
        self.proxy_servers_changed(&state);
        drop(state);
        if retest {
            self.test_in_background(id);
        }
        Ok(())
    }

    /// Deletes a profile; the app and the queues that use it use `replacement` instead (D73.4).
    /// Downloads running through it finish on it.
    ///
    /// # Errors
    ///
    /// [`CoreError::ProxyServerNotFound`], [`CoreError::Invalid`] when `replacement` is the
    /// profile itself or one that is not saved, and [`CoreError::Closed`] once the manager has
    /// shut down.
    pub fn delete_proxy_server(&self, id: ProxyServerId, replacement: ProxyUse) -> Result<()> {
        let mut state = self.lock_open()?;
        let index = profile_index(&state, id)?;
        if replacement == ProxyUse::Server(id) {
            return Err(CoreError::Invalid(
                "Choose another proxy for what uses the one being deleted.".into(),
            ));
        }
        wirecatch_settings::proxy::check_use(replacement, &state.proxy_servers)?;
        let gone = ProxyUse::Server(id);
        let mut queues_changed = false;
        for slot in &mut state.queues {
            if slot.queue.settings.proxy == Setting::Own(gone) {
                slot.queue.settings.proxy = Setting::Own(replacement);
                queues_changed = true;
            }
        }
        state.proxy_servers.remove(index);
        if state.proxy_logins.remove(&id).is_some() {
            self.inner
                .store
                .save_proxy_logins(state.proxy_logins.clone());
        }
        self.proxy_servers_changed(&state);
        if queues_changed {
            self.queues_changed(&mut state);
        }
        if state.settings.proxy.proxy == gone {
            let mut settings = state.settings.clone();
            settings.proxy.proxy = replacement;
            self.apply_settings(&mut state, settings);
        }
        Ok(())
    }

    /// Sends one request through a profile as `draft` has it, before it is saved (D71.5). For a
    /// saved profile `id`, a password the draft keeps is the saved one, and the result becomes
    /// its last test when the draft is the profile as saved.
    ///
    /// # Errors
    ///
    /// [`CoreError::Invalid`] for a profile that cannot work, and
    /// [`CoreError::ProxyServerNotFound`] for an `id` that is not saved.
    pub async fn test_proxy_server(
        &self,
        id: Option<ProxyServerId>,
        draft: ProxyServerDraft,
    ) -> Result<ProxyTestResult> {
        let (saved, password) = {
            let state = self.lock();
            match id {
                Some(id) => {
                    let saved = state.proxy_servers[profile_index(&state, id)?]
                        .settings
                        .clone();
                    (Some(saved), state.proxy_logins.get(&id).cloned())
                }
                None => (None, None),
            }
        };
        let settings = wirecatch_settings::proxy::validate_settings_file(draft.settings, &[])?;
        let password = wirecatch_settings::proxy::merge_password(
            password.as_ref(),
            &draft.password,
            saved.as_ref(),
            &settings,
        )?;
        let result = self.run_test(&settings, password.as_ref()).await;
        if let Some(id) = id
            && draft.password.is_keep()
            && saved.as_ref() == Some(&settings)
        {
            self.keep_test(id, &settings, password.as_ref(), result.clone());
        }
        Ok(result)
    }

    /// Saves a profile for each server the computer's proxy names, without passwords; a server
    /// already saved is not saved twice. Answers with their ids.
    ///
    /// # Errors
    ///
    /// The errors of [`create_proxy_server`](Self::create_proxy_server).
    pub async fn copy_system_proxy(&self) -> Result<Vec<ProxyServerId>> {
        let computer = self.lock_open()?.computer_proxy.clone();
        let found = self
            .blocking(move || Ok(wirecatch_settings::proxy::from_system(&computer)))
            .await?;
        let mut state = self.lock_open()?;
        let copies = wirecatch_settings::proxy::copies(found.clone(), &state.proxy_servers)?;
        let mut added = Vec::with_capacity(copies.len());
        for settings in copies {
            let id = state.ids.proxy_server();
            state.proxy_servers.push(SavedProxyServer {
                id,
                settings,
                last_test: None,
            });
            added.push(id);
        }
        let ids = found
            .iter()
            .filter_map(|server| {
                state
                    .proxy_servers
                    .iter()
                    .find(|p| wirecatch_settings::proxy::same_proxy(&p.settings, server))
                    .map(|p| p.id)
            })
            .collect();
        if !added.is_empty() {
            self.inner.store.save_last_ids(state.ids.last());
            self.proxy_servers_changed(&state);
        }
        drop(state);
        for id in added {
            self.test_in_background(id);
        }
        Ok(ids)
    }

    /// The proxy the app updater uses: Main's proxy for `https`.
    #[must_use]
    pub fn updater_proxy(&self) -> UpdaterProxy {
        wirecatch_settings::proxy::updater_proxy(&self.lock().app_proxy())
    }

    /// Saves the profiles and tells the windows.
    pub(super) fn proxy_servers_changed(&self, state: &State) {
        self.inner
            .store
            .save_proxy_servers(state.proxy_servers.clone());
        self.emit(UiEvent::ProxyServersChanged(state.proxy_server_list()));
    }

    /// Tests saved profile `id` once, keeping the result as its last test.
    pub(super) fn test_in_background(&self, id: ProxyServerId) {
        let manager = self.clone();
        self.spawn(async move {
            let (settings, password) = {
                let state = manager.lock();
                let Ok(index) = profile_index(&state, id) else {
                    return;
                };
                (
                    state.proxy_servers[index].settings.clone(),
                    state.proxy_logins.get(&id).cloned(),
                )
            };
            let result = manager.run_test(&settings, password.as_ref()).await;
            manager.keep_test(id, &settings, password.as_ref(), result);
        });
    }

    async fn run_test(
        &self,
        settings: &ProxyServerSettings,
        password: Option<&Secret>,
    ) -> ProxyTestResult {
        let Some(url) = self.inner.proxy_test_url.clone() else {
            return ProxyTestResult::Unreachable {
                reason: "this build has no address to test the proxy with".into(),
            };
        };
        let mut manual = wirecatch_engine::ManualProxies::default();
        manual.all = wirecatch_settings::proxy::profile_url(settings, password);
        crate::proxy::test(
            wirecatch_engine::ProxyChoice::Manual(manual),
            &url,
            self.inner.stopped.child_token(),
        )
        .await
    }

    /// Keeps `result` as profile `id`'s last test while it is still the proxy and password tested,
    /// so a test that ends after a newer one never replaces its result.
    fn keep_test(
        &self,
        id: ProxyServerId,
        tested: &ProxyServerSettings,
        password: Option<&Secret>,
        result: ProxyTestResult,
    ) {
        let at = self.inner.clock.unix_millis();
        let mut state = self.lock();
        let Ok(index) = profile_index(&state, id) else {
            return;
        };
        let current = state.proxy_logins.get(&id) == password;
        let profile = &mut state.proxy_servers[index];
        if !current || !wirecatch_settings::proxy::same_proxy(&profile.settings, tested) {
            return;
        }
        profile.last_test = Some(ProxyTestRun { at, result });
        self.proxy_servers_changed(&state);
    }
}

fn profile_index(state: &State, id: ProxyServerId) -> Result<usize> {
    state
        .proxy_servers
        .iter()
        .position(|p| p.id == id)
        .ok_or(CoreError::ProxyServerNotFound(id))
}

/// How often the system's proxy setting is read again for [`State::system_proxy`].
const READ_SYSTEM_PROXY_EVERY: std::time::Duration = std::time::Duration::from_secs(30);

/// Keeps [`State::system_proxy`] up to date, reading the setting off the manager's lock.
pub(super) async fn run_system_proxy_reads(
    weak: std::sync::Weak<super::Inner>,
    stopped: CancellationToken,
    computer: ComputerProxy,
) {
    loop {
        tokio::select! {
            () = tokio::time::sleep(READ_SYSTEM_PROXY_EVERY) => {}
            () = stopped.cancelled() => return,
        }
        let computer = computer.clone();
        let Ok(read) = tokio::task::spawn_blocking(move || computer.proxies()).await else {
            return;
        };
        let Some(manager) = Manager::from_weak(&weak) else {
            return;
        };
        manager.lock().system_proxy = read;
    }
}

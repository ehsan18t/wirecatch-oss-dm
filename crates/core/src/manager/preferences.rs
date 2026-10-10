//! Reading and changing settings, from the UI or from the extension, and settings
//! export and import.

use std::path::Path;

use wirecatch_protocol::app::{AppSettings, UiEvent};
use wirecatch_protocol::extension::{ExtensionSettings, ExtensionSettingsPatch};
use wirecatch_protocol::wire::Event;

use super::Manager;
use super::queues::apply_to_slot;
use super::state::{QueueSlot, State};
use crate::error::{CoreError, Result};
use crate::{profile, settings};

impl Manager {
    /// The current settings.
    pub fn get_settings(&self) -> AppSettings {
        self.lock().settings.clone()
    }

    /// Replaces the settings after checking them. Returns them cleaned up (lists in
    /// lower case without duplicates). New limits apply to running downloads at once.
    pub fn update_settings(&self, new: AppSettings) -> Result<AppSettings> {
        let mut state = self.lock_open()?;
        let new = settings::validate(new, &state.queue_list(), &self.inner.folders)?;
        self.apply_settings(&mut state, new.clone());
        Ok(new)
    }

    /// The settings the extension shows.
    pub fn extension_settings(&self) -> ExtensionSettings {
        self.lock().settings.extension.clone()
    }

    /// Changes some of the extension's settings, as asked by a socket client. Sites
    /// to add or remove are matched the way the list stores them (lower case).
    pub fn update_extension_settings(
        &self,
        mut patch: ExtensionSettingsPatch,
    ) -> Result<ExtensionSettings> {
        for list in [
            &mut patch.add_ignored_sites,
            &mut patch.remove_ignored_sites,
        ]
        .into_iter()
        .flatten()
        {
            list.iter_mut()
                .for_each(|s| *s = settings::normalize_site(s));
        }
        let mut state = self.lock_open()?;
        let mut new = state.settings.clone();
        new.extension.apply(patch);
        let new = settings::validate(new, &state.queue_list(), &self.inner.folders)?;
        let extension = new.extension.clone();
        self.apply_settings(&mut state, new);
        Ok(extension)
    }

    /// Writes every setting and queue to `path` as a settings profile (D6). Folders
    /// are written relative to the user's standard folders, so the file works on
    /// another machine.
    pub async fn export_settings(&self, path: &Path) -> Result<()> {
        let profile = {
            let state = self.lock();
            profile::export(
                &state.settings,
                &state.queue_list(),
                &self.inner.folders,
                self.inner.clock.unix_millis(),
            )
        };
        let json =
            serde_json::to_vec_pretty(&profile).map_err(|e| CoreError::Internal(e.to_string()))?;
        let path = path.to_path_buf();
        self.blocking(move || {
            std::fs::write(&path, json).map_err(|source| CoreError::File {
                path: path.display().to_string(),
                source,
            })
        })
        .await
    }

    /// Applies a settings profile from `path`. Queues are matched by name, folders
    /// are resolved on this machine, and downloads are left alone. What changed,
    /// including any folder that had to be replaced, is sent to the UI as
    /// [`UiEvent::SettingsImported`].
    pub async fn import_settings(&self, path: &Path) -> Result<()> {
        let path = path.to_path_buf();
        let bytes = self.blocking(move || profile::read_file(&path)).await?;
        let incoming = profile::parse(&bytes)?;

        let mut state = self.lock_open()?;
        let current = state.queue_list();
        let imported = profile::merge(incoming, &current, &self.inner.folders, || {
            state.ids.queue()
        })?;
        self.inner.store.save_last_ids(state.ids.last());
        for queue in imported.queues {
            match state.slot_mut(queue.id) {
                Ok(slot) => apply_to_slot(slot, queue),
                Err(_) => state.queues.push(QueueSlot::new(queue)),
            }
        }
        // Queues first: the new settings may name a queue that was just added.
        self.queues_changed(&mut state);
        self.apply_settings(&mut state, imported.settings);
        self.emit(UiEvent::SettingsImported(imported.summary));
        Ok(())
    }

    /// Stores checked settings and tells the UI and, if their part changed, every
    /// socket client.
    pub(super) fn apply_settings(&self, state: &mut State, new: AppSettings) {
        if state.settings == new {
            return;
        }
        let old = std::mem::replace(&mut state.settings, new);
        let new = &state.settings;
        if old.global_speed_limit != new.global_speed_limit {
            self.inner.global_limiter.set_limit(new.global_speed_limit);
        }
        self.inner.store.save_settings(new.clone());
        if old.extension != new.extension {
            self.emit_wire(Event::ExtensionSettingsChanged(new.extension.clone()));
        }
        self.emit(UiEvent::SettingsChanged(new.clone()));
        self.reconcile(state);
    }
}

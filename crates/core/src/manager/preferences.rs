//! Reading and changing settings, from the UI or from the extension, and settings
//! export and import.

use std::path::Path;

use wirecatch_protocol::QueueId;
use wirecatch_protocol::app::{AddonId, AppSettings, AppSettingsPatch, ImportPreview, UiEvent};
use wirecatch_protocol::extension::{ExtensionSettings, ExtensionSettingsPatch};
use wirecatch_protocol::wire::Event;

use super::Manager;
use super::queues::apply_to_slot;
use super::state::{QueueSlot, State};
use crate::error::{CoreError, Result};
use crate::{profile, settings};

impl Manager {
    /// The current settings.
    #[must_use]
    pub fn get_settings(&self) -> AppSettings {
        self.lock().settings.clone()
    }

    /// Replaces every setting after checking them. Lists are cleaned up (lower case,
    /// no duplicates), and the stored result arrives as `SettingsChanged`. New
    /// limits apply to running downloads at once.
    ///
    /// Settings someone else changed in the meantime are overwritten, so the windows and
    /// the shell change settings with [`change_settings`](Self::change_settings); this is
    /// for replacing them all at once.
    ///
    /// # Errors
    ///
    /// [`CoreError::Invalid`] for a value out of range or a queue that does not exist,
    /// [`CoreError::Path`] for a folder that does not resolve, and
    /// [`CoreError::Closed`] once the manager has shut down. Nothing changes on an
    /// error.
    pub fn update_settings(&self, new: AppSettings) -> Result<()> {
        let mut state = self.lock_open()?;
        let new = settings::validate(new, &state.queue_list(), &self.inner.folders)?;
        self.apply_settings(&mut state, new);
        Ok(())
    }

    /// Applies `patch` to the current settings, checks the result and keeps it, like [`update_settings`](Self::update_settings). A patch holds only the settings that change, so settings nobody changed keep their current values whoever set them: two windows changing different settings never undo each other.
    ///
    /// # Errors
    ///
    /// The errors of [`update_settings`](Self::update_settings).
    pub fn change_settings(&self, patch: AppSettingsPatch) -> Result<()> {
        if let Some(extension) = &patch.extension {
            settings::check_site_patch(extension)?;
        }
        let mut state = self.lock_open()?;
        let mut new = state.settings.clone();
        new.apply(patch);
        let new = settings::validate(new, &state.queue_list(), &self.inner.folders)?;
        self.apply_settings(&mut state, new);
        Ok(())
    }

    /// The settings the extension shows.
    #[must_use]
    pub fn extension_settings(&self) -> ExtensionSettings {
        self.lock().settings.extension.clone()
    }

    /// Changes some of the extension's settings, as asked by a socket client. Sites to add or remove are matched the way the list stores them ([`normalize_site`](wirecatch_protocol::extension::normalize_site)); the lists are checked before the manager's lock is taken, so a client sending a huge one holds nobody up.
    ///
    /// # Errors
    ///
    /// The errors of [`update_settings`](Self::update_settings), and [`CoreError::Invalid`] for a site to keep that names no host.
    pub fn update_extension_settings(
        &self,
        patch: ExtensionSettingsPatch,
    ) -> Result<ExtensionSettings> {
        settings::check_site_patch(&patch)?;
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
    ///
    /// # Errors
    ///
    /// [`CoreError::File`] when `path` cannot be written.
    pub async fn export_settings(&self, path: &Path) -> Result<()> {
        let profile = {
            let state = self.lock();
            let mut profile = profile::export(
                &state.settings,
                &state.queue_list(),
                &self.inner.folders,
                self.inner.clock.unix_millis(),
            );
            if state.addon.tools.is_some() {
                profile.addons.push(AddonId::Media);
            }
            profile
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
    /// [`UiEvent::SettingsImported`]. Add-ons the profile lists that are not
    /// installed here are listed there for the UI to offer; nothing is
    /// downloaded without the person asking.
    ///
    /// # Errors
    ///
    /// [`CoreError::File`] when `path` cannot be read, [`CoreError::InvalidProfile`]
    /// when it is not a settings profile, [`CoreError::ProfileTooNew`] when a newer
    /// Wirecatch made it, [`CoreError::Invalid`] when its settings or queues are out of
    /// range, and [`CoreError::Closed`] once the manager has shut down. Nothing changes
    /// on an error.
    pub async fn import_settings(&self, path: &Path) -> Result<()> {
        let localized = self.read_profile(path).await?;
        let wants_media = localized.profile.addons.contains(&AddonId::Media);

        {
            let mut state = self.lock_open()?;
            let current = state.queue_list();
            // Ids for added queues come from a copy of the counters, kept only
            // once the profile is accepted, so a refused one uses up none.
            let mut ids = state.ids.clone();
            let mut imported =
                profile::merge_localized(localized, &current, &self.inner.folders, || ids.queue())?;
            state.ids = ids;
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
            if wants_media && state.addon.tools.is_none() {
                imported.summary.addons_to_offer.push(AddonId::Media);
            }
            self.emit(UiEvent::SettingsImported(imported.summary));
        }
        Ok(())
    }

    /// What [`import_settings`](Self::import_settings) would change with the
    /// profile at `path`, without changing anything: every folder as it would
    /// resolve here (startup and system folders refused and replaced, folders
    /// outside the person's own folders flagged), the queues it would add and
    /// update, and the add-ons it would offer. The UI shows this and asks before
    /// importing.
    ///
    /// # Errors
    ///
    /// The errors of [`import_settings`](Self::import_settings).
    pub async fn preview_import(&self, path: &Path) -> Result<ImportPreview> {
        let localized = self.read_profile(path).await?;
        let wants_media = localized.profile.addons.contains(&AddonId::Media);
        let state = self.lock_open()?;
        let current = state.queue_list();
        // Ids for queues that would be added; they are never used.
        let mut next = current.iter().map(|q| q.id.0).max().unwrap_or(0);
        let imported = profile::merge_localized(localized, &current, &self.inner.folders, || {
            next += 1;
            QueueId(next)
        })?;
        let mut addons_to_offer = Vec::new();
        if wants_media && state.addon.tools.is_none() {
            addons_to_offer.push(AddonId::Media);
        }
        Ok(ImportPreview {
            folders: imported.summary.folders,
            queues_added: imported.added,
            queues_updated: imported.updated,
            addons_to_offer,
        })
    }

    /// Reads and checks the profile at `path` and resolves its folders here,
    /// off the async threads.
    async fn read_profile(&self, path: &Path) -> Result<profile::Localized> {
        let path = path.to_path_buf();
        let folders = self.inner.folders.clone();
        self.blocking(move || {
            let bytes = profile::read_file(&path)?;
            Ok(profile::localize(profile::parse(&bytes)?, &folders))
        })
        .await
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

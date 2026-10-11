//! Reading and changing settings, from the UI or from the extension, and settings
//! export and import.

use std::path::Path;

use wirecatch_protocol::QueueId;
use wirecatch_protocol::app::{AppSettings, ImportPreview, SettingsChange, UiEvent};
use wirecatch_protocol::extension::{ExtensionSettings, ExtensionSettingsPatch};
use wirecatch_protocol::wire::Event;

use super::Manager;
use super::queues::apply_to_slot;
use super::state::{QueueSlot, State};
use crate::addons::manifest::MEDIA_ADDON_ID;
use crate::error::{CoreError, Result};
use crate::{profile, settings};

impl Manager {
    /// The current settings.
    pub fn get_settings(&self) -> AppSettings {
        self.lock().settings.clone()
    }

    /// Replaces the settings after checking them. Lists are cleaned up (lower case,
    /// no duplicates), and the stored result arrives as `SettingsChanged`. New
    /// limits apply to running downloads at once.
    pub fn update_settings(&self, new: AppSettings) -> Result<()> {
        let mut state = self.lock_open()?;
        let new = settings::validate(new, &state.queue_list(), &self.inner.folders)?;
        self.apply_settings(&mut state, new);
        Ok(())
    }

    /// Applies `changes` to the current settings, checks the result and keeps it, like [`update_settings`](Self::update_settings). Each change names one setting, so settings nobody changed keep their current values whoever set them: two windows changing different settings never undo each other.
    pub fn change_settings(&self, changes: &[SettingsChange]) -> Result<()> {
        let mut state = self.lock_open()?;
        let new = with_changes(&state.settings, changes)?;
        let new = settings::validate(new, &state.queue_list(), &self.inner.folders)?;
        self.apply_settings(&mut state, new);
        Ok(())
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
            let mut profile = profile::export(
                &state.settings,
                &state.queue_list(),
                &self.inner.folders,
                self.inner.clock.unix_millis(),
            );
            if state.addon.tools.is_some() {
                profile.addons.push(MEDIA_ADDON_ID.to_string());
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
    pub async fn import_settings(&self, path: &Path) -> Result<()> {
        let localized = self.read_profile(path).await?;
        let wants_media = localized.profile.addons.iter().any(|a| a == MEDIA_ADDON_ID);

        {
            let mut state = self.lock_open()?;
            let current = state.queue_list();
            let mut imported =
                profile::merge_localized(localized, &current, &self.inner.folders, || {
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
            if wants_media && state.addon.tools.is_none() {
                imported
                    .summary
                    .addons_to_offer
                    .push(MEDIA_ADDON_ID.to_string());
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
    pub async fn preview_import(&self, path: &Path) -> Result<ImportPreview> {
        let localized = self.read_profile(path).await?;
        let wants_media = localized.profile.addons.iter().any(|a| a == MEDIA_ADDON_ID);
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
            addons_to_offer.push(MEDIA_ADDON_ID.to_string());
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

/// `current` with `changes` applied, each replacing the value at its path. The result is read back as [`AppSettings`], so a value of the wrong kind is refused rather than stored.
fn with_changes(current: &AppSettings, changes: &[SettingsChange]) -> Result<AppSettings> {
    let mut value = serde_json::to_value(current)
        .map_err(|e| CoreError::Internal(format!("settings could not be read as JSON: {e}")))?;
    for change in changes {
        let path = change.path.join(".");
        let refused = |problem: &str| CoreError::SettingsChange {
            path: path.clone(),
            problem: problem.to_owned(),
        };
        let Some((last, parents)) = change.path.split_last() else {
            return Err(refused("no setting was named"));
        };
        let mut object = &mut value;
        for name in parents {
            object = object
                .get_mut(name.as_str())
                .filter(|inner| inner.is_object())
                .ok_or_else(|| refused("there is no such group of settings"))?;
        }
        let slot = object
            .get_mut(last.as_str())
            .ok_or_else(|| refused("there is no such setting"))?;
        *slot = change.value.clone();
    }
    serde_json::from_value(value).map_err(|e| CoreError::SettingsChange {
        path: changes
            .iter()
            .map(|c| c.path.join("."))
            .collect::<Vec<_>>()
            .join(", "),
        problem: format!("the new value is not one it can take ({e})"),
    })
}

//! Looking for newer yt-dlp releases, checked against their signed checksums (D21).

use tracing::{info, warn};
use wirecatch_addons::manifest::{self, AddonComponent, ComponentId, ReleaseFile};
use wirecatch_addons::{self as addons, signature};
use wirecatch_engine::units::{KIB, MIB};
use wirecatch_engine::{
    CancellationToken, Fetcher, Url,
    header::{ACCEPT, HeaderValue},
};
use wirecatch_protocol::UnixMillis;

use super::{
    ADDON_NETWORK, CHECK_EVERY, LOOK_EVERY, Lookup, MATURE_AGE, NewRelease, OpKind, Unsigned,
};
use crate::error::{CoreError, Result};
use crate::manager::Manager;

/// Largest yt-dlp release file accepted from an update (it is about 40 MB).
pub(super) const MAX_UPDATE_SIZE: u64 = 200 * MIB;

/// How many of the newest yt-dlp releases are looked at for one old enough.
pub(super) const RELEASES_LOOKED_AT: usize = 10;

/// Largest GitHub release description or checksum list read.
pub(super) const MAX_METADATA: u64 = 4 * MIB;

/// Largest signature file read (yt-dlp's is under 1 KB).
pub(super) const MAX_SIGNATURE: u64 = 64 * KIB;

/// The releases of `releases` that may be installed, with when each came out, newest version first.
fn candidates(releases: Vec<addons::Release>, pinned: &str) -> Vec<(addons::Release, UnixMillis)> {
    let mut candidates: Vec<(addons::Release, UnixMillis)> = releases
        .into_iter()
        .take(RELEASES_LOOKED_AT)
        .filter(|r| !r.draft && !r.prerelease)
        // `newer` only reads plain dotted numbers, so the tag is safe in a path.
        .filter(|r| addons::newer(r.tag_name.trim(), pinned))
        .filter_map(|r| {
            let published = r
                .published_at
                .as_deref()
                .and_then(|at| chrono::DateTime::parse_from_rfc3339(at).ok())?
                .timestamp_millis();
            Some((r, UnixMillis(published)))
        })
        .collect();
    // Newest version first, whatever order the list came in.
    candidates.sort_by(|(a, _), (b, _)| {
        if addons::newer(a.tag_name.trim(), b.tag_name.trim()) {
            std::cmp::Ordering::Less
        } else if addons::newer(b.tag_name.trim(), a.tag_name.trim()) {
            std::cmp::Ordering::Greater
        } else {
            std::cmp::Ordering::Equal
        }
    });
    candidates
}

/// What a look at yt-dlp's list of releases found.
pub(super) struct Looked {
    /// `None` when the look was cut short, which says nothing about the releases it did not reach.
    pub found: Option<Vec<NewRelease>>,
    /// Why a release was passed over, the newest first.
    pub problem: Option<Lookup>,
}

/// What the person who asked for the check is told about `lookup`, if anything. A release that does not meet the rules is not offered; there is nothing to report.
fn problem_for(lookup: Lookup) -> Option<String> {
    match lookup {
        Lookup::Refused(reason) => {
            warn!(%reason, "a newer yt-dlp was refused");
            None
        }
        Lookup::Unsigned { version, why } => {
            warn!(%version, reason = %why, "a newer yt-dlp was refused");
            Some(format!(
                "yt-dlp {version} was not installed because its checksum list does not carry a valid signature from yt-dlp's developers ({why}). The installed version keeps working; if this keeps happening, please report it."
            ))
        }
        Lookup::Unreachable(reason) => {
            warn!(%reason, "could not check for a newer yt-dlp");
            Some(format!(
                "Wirecatch could not check for a newer media add-on ({reason}). Check your internet connection, then try again."
            ))
        }
    }
}

impl Manager {
    /// Looks for a newer yt-dlp now (sites change often, and an old yt-dlp stops
    /// working with them). With `auto_update_addons` on, anything out of date is
    /// then updated quietly. Does nothing when the add-on is not installed.
    ///
    /// When the release could not be looked up, or its checksum list is not
    /// signed by yt-dlp's developers, the check still counts (so the daily
    /// check does not retry at once) and this fails with the reason, for a
    /// person who asked to check.
    ///
    /// # Errors
    ///
    /// [`CoreError::UpdateCheckFailed`], with a message for the person who asked, when the
    /// releases could not be looked up or a newer release's checksum list is not signed
    /// by yt-dlp's developers; and [`CoreError::Closed`] once the manager has shut
    /// down.
    pub async fn check_addon_updates(&self) -> Result<()> {
        if self.lock_open()?.addon.tools.is_none() {
            return Ok(());
        }
        let (found, problem) = match self.new_ytdlp_releases().await {
            Ok(looked) => (looked.found, looked.problem.and_then(problem_for)),
            Err(lookup) => (None, problem_for(lookup)),
        };
        // Read only now: the add-on may have been removed or updated during the wait.
        let now = self.inner.clock.unix_millis();
        let to_save = {
            let mut state = self.lock_open()?;
            state.addon.record.last_update_check = Some(now.0);
            if state.addon.tools.is_none() {
                return Ok(());
            }
            // An install under way saves its own record.
            state.addon.op.is_none().then(|| state.addon.record.clone())
        };
        if let Some(to_save) = to_save {
            let dir = self.inner.addon_dir.clone();
            // Saving creates the folder, and a removal may have taken it since the lock.
            let saved = self
                .blocking(move || Ok(dir.is_dir().then(|| to_save.save(&dir))))
                .await?;
            if let Some(Err(e)) = saved {
                warn!(error = %e, "could not record the add-on update check");
            }
        }
        let mut state = self.lock_open()?;
        // Removed during the save: every part would read as missing and be installed again.
        if state.addon.tools.is_none() {
            return Ok(());
        }
        let installed = state
            .addon
            .record
            .version(ComponentId::YtDlp)
            .map(str::to_string);
        let refused: Vec<String> = state
            .addon
            .refused
            .iter()
            .map(|r| r.version.clone())
            .collect();
        // A check that failed says nothing about what the last one found.
        if let Some(found) = found {
            state.addon.ytdlp_releases = found
                .into_iter()
                .filter(|r| {
                    installed
                        .as_deref()
                        .is_none_or(|have| addons::newer(r.component.version(), have))
                        // Refused for not being what it claims: never downloaded or run again while the app runs.
                        && !refused.iter().any(|v| v == r.component.version())
                })
                .collect();
        }
        // A release younger than two days is not installed yet, by any path.
        let plan = self.addon_plan(&state.addon);
        if state.settings.auto_update_addons && !plan.is_empty() && state.addon.op.is_none() {
            info!(parts = plan.len(), "updating the media add-on quietly");
            self.start_addon_op(&mut state.addon, plan, OpKind::Quiet);
        } else {
            self.emit_addons(&state.addon);
        }
        match problem {
            None => Ok(()),
            Some(message) => Err(CoreError::UpdateCheckFailed(message)),
        }
    }

    /// The yt-dlp releases worth installing, newest first, each newer than the
    /// pinned one and checked: the newest release and, when that one is younger
    /// than two days, the newest older than that, so the add-on can move to it
    /// meanwhile (D21.1). Read from yt-dlp's list of releases; drafts,
    /// pre-releases, tags that are not plain versions and releases that do not
    /// say when they came out are skipped. A release that fails its checks is passed over for an older one and reported in [`Looked::problem`].
    pub(super) async fn new_ytdlp_releases(&self) -> std::result::Result<Looked, Lookup> {
        let sources = self.inner.addon_sources.clone();
        let Some(api) = &sources.ytdlp_releases else {
            return Ok(Looked {
                found: Some(Vec::new()),
                problem: None,
            });
        };
        let cancel = self.inner.stopped.child_token();
        let proxy = self.lock().app_proxy();
        let mut options = crate::headers::download_options(ADDON_NETWORK, proxy);
        options.headers.insert(
            ACCEPT,
            HeaderValue::from_static("application/vnd.github+json"),
        );
        let fetcher = Fetcher::with_private_network(api, &options, cancel, ADDON_NETWORK)
            .await
            .map_err(|e| Lookup::Unreachable(e.to_string()))?;
        let body = fetcher
            .get(api, None, MAX_METADATA)
            .await
            .map_err(|e| Lookup::Unreachable(e.to_string()))?
            .body;
        let releases: Vec<addons::Release> = serde_json::from_slice(&body)
            .map_err(|e| Lookup::Refused(format!("unreadable list of releases ({e})")))?;
        let now = self.inner.clock.unix_millis();
        let candidates = candidates(releases, sources.ytdlp.version());
        let mut found = Vec::new();
        let mut problem = None;
        for (release, published) in candidates {
            let mature = now.since(published) >= MATURE_AGE;
            // Past the newest, only a release old enough to install is of use.
            if !found.is_empty() && !mature {
                continue;
            }
            let component = match self.checked_release(&fetcher, &sources, &release).await {
                Ok(component) => component,
                Err(lookup) => {
                    if matches!(lookup, Lookup::Unreachable(_)) {
                        return Ok(Looked {
                            found: None,
                            problem: Some(lookup),
                        });
                    }
                    problem.get_or_insert(lookup);
                    continue;
                }
            };
            found.push(NewRelease {
                component,
                published,
            });
            if mature {
                break;
            }
        }
        Ok(Looked {
            found: Some(found),
            problem,
        })
    }

    /// `release` as a component checked against its own checksum list.
    ///
    /// Only yt-dlp's own release folder is trusted for the files, the file's
    /// size must be known and reasonable, and its checksum comes from the
    /// release's checksum list, which must carry a valid OpenPGP signature by
    /// yt-dlp's pinned release key (D21.1).
    pub(super) async fn checked_release(
        &self,
        fetcher: &Fetcher,
        sources: &manifest::MediaAddonSources,
        release: &addons::Release,
    ) -> std::result::Result<AddonComponent, Lookup> {
        let version = release.tag_name.trim().to_string();
        // `newer` let only a plain dotted version through, so the tag is one folder name.
        let mut downloads = sources.ytdlp_release_downloads.clone();
        if !downloads.path().ends_with('/') {
            let path = format!("{}/", downloads.path());
            downloads.set_path(&path);
        }
        let folder = downloads
            .join(&format!("{version}/"))
            .map_err(|e| Lookup::Refused(e.to_string()))?;
        // A file of the release is taken only from exactly `<folder><name>`.
        let in_folder = |asset: &addons::Asset| -> Option<Url> {
            folder
                .join(&asset.name)
                .ok()
                .filter(|url| url.as_str() == asset.browser_download_url)
        };
        let asset = |name: &str| release.assets.iter().find(|a| a.name == name);
        let (Some(program), Some(sums)) = (
            asset(&sources.ytdlp_asset),
            asset(&sources.ytdlp_checksums_asset),
        ) else {
            return Err(Lookup::Refused(format!(
                "release {version} lacks {} or its checksums",
                sources.ytdlp_asset
            )));
        };
        let (Some(program_url), Some(sums_url)) = (in_folder(program), in_folder(sums)) else {
            return Err(Lookup::Refused(format!(
                "release {version} points {} or its checksums somewhere other than yt-dlp's releases",
                program.name
            )));
        };
        if program.size == 0 || program.size > MAX_UPDATE_SIZE {
            return Err(Lookup::Refused(format!(
                "release {version} gives {} an unusable size ({} bytes)",
                program.name, program.size
            )));
        }
        let list = fetcher
            .get(&sums_url, None, MAX_METADATA)
            .await
            .map_err(|e| Lookup::Unreachable(e.to_string()))?
            .body;
        // The checksum list counts only with yt-dlp's signature on it.
        let unsigned = |why: Unsigned| Lookup::Unsigned {
            version: version.clone(),
            why,
        };
        let sig_name = format!("{}.sig", sources.ytdlp_checksums_asset);
        let sig = asset(&sig_name).ok_or_else(|| unsigned(Unsigned::NoSignature))?;
        let sig_url = in_folder(sig).ok_or_else(|| unsigned(Unsigned::Elsewhere))?;
        let signature = fetcher
            .get(&sig_url, None, MAX_SIGNATURE)
            .await
            .map_err(|e| Lookup::Unreachable(e.to_string()))?
            .body;
        let key = sources.ytdlp_signing_key.clone();
        let signed = list.clone();
        // The check itself not running (the app closing) says nothing about the release: it is tried again later.
        self.blocking(move || Ok(signature::verify_detached(&key, &signed, &signature)))
            .await
            .map_err(|e| Lookup::Unreachable(e.to_string()))?
            .map_err(|why| unsigned(Unsigned::Invalid(why)))?;
        let list = String::from_utf8_lossy(&list);
        let sha256 = addons::checksum_for(&list, &sources.ytdlp_asset).ok_or_else(|| {
            Lookup::Refused(format!(
                "release {version} has no checksum for {}",
                sources.ytdlp_asset
            ))
        })?;
        Ok(sources.ytdlp.for_release(
            version,
            ReleaseFile::new(program_url, sha256, program.size)
                .map_err(|e| Lookup::Refused(e.to_string()))?,
        ))
    }

    /// Runs the update check whenever it is due, while the app runs.
    pub(in crate::manager) async fn run_addon_updates(
        weak: std::sync::Weak<crate::manager::Inner>,
        stopped: CancellationToken,
    ) {
        loop {
            let Some(manager) = Manager::from_weak(&weak) else {
                return;
            };
            let due = {
                let state = manager.lock();
                let last = UnixMillis(state.addon.record.last_update_check.unwrap_or(i64::MIN));
                state.settings.auto_update_addons
                    && state.addon.tools.is_some()
                    && manager.inner.addon_sources.ytdlp_releases.is_some()
                    && manager.inner.clock.unix_millis().since(last) >= CHECK_EVERY
            };
            if due && let Err(e) = manager.check_addon_updates().await {
                warn!(error = %e, "the add-on update check failed");
            }
            manager.expire_addon_waits();
            drop(manager);
            tokio::select! {
                () = tokio::time::sleep(LOOK_EVERY) => {}
                () = stopped.cancelled() => return,
            }
        }
    }
}

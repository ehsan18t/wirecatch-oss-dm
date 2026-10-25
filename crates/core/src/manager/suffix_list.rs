//! Keeping the Public Suffix List current while the app runs (D44). A newer
//! list is fetched from publicsuffix.org once a week while no download runs,
//! or when the person asks; it is checked whole, kept on disk in one step, and
//! only then takes the place of the list in use. Any failure keeps the list in
//! use.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tracing::{info, warn};
use wirecatch_engine::{CancellationToken, Fetcher, PrivateNetwork, Url};
use wirecatch_protocol::app::{SuffixListInfo, UiEvent};

use super::Manager;
use super::state::State;
use crate::error::{CoreError, Result};
use crate::public_suffix::{MAX_LIST_BYTES, Refused, SuffixList};
use crate::settings;

/// Where publicsuffix.org publishes the list (D44).
pub const PUBLIC_SUFFIX_LIST_URL: &str = "https://publicsuffix.org/list/public_suffix_list.dat";

/// How long a list stays current before a newer one is looked for.
const CHECK_EVERY_MILLIS: i64 = 7 * 24 * 60 * 60 * 1000;
/// How soon a check that failed is tried again.
const RETRY_AFTER_MILLIS: i64 = 24 * 60 * 60 * 1000;
/// How often the background task looks whether a check is due. It waits this
/// long after the app starts too, so a start is never slowed by a check.
const LOOK_EVERY: Duration = Duration::from_hours(1);
/// The longest a check may take, from the first connection to the last byte.
const FETCH_TIMEOUT: Duration = Duration::from_mins(2);
/// The list's host is never on the local network.
const SUFFIX_NETWORK: PrivateNetwork = PrivateNetwork::Block;
/// The file the last check is recorded in, in the data folder.
const CHECK_FILE: &str = "public-suffix-check.json";

/// The last check, kept across starts so the check stays weekly.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct SuffixCheck {
    /// When it ran, in milliseconds since the Unix epoch.
    at: Option<i64>,
    /// Whether it ended with the newest list in use: a newer one taken, or none found.
    current: bool,
}

impl SuffixCheck {
    /// The check recorded in `dir`; a missing or damaged record counts as none. Blocks on
    /// the disk.
    pub(super) fn load(dir: &Path) -> Self {
        match std::fs::read(dir.join(CHECK_FILE)) {
            Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_else(|e| {
                warn!(error = %e, "the record of the last public suffix list check is damaged");
                Self::default()
            }),
            Err(e) => {
                if e.kind() != std::io::ErrorKind::NotFound {
                    warn!(error = %e, "the record of the last public suffix list check cannot be read");
                }
                Self::default()
            }
        }
    }

    fn save(self, dir: &Path) -> std::io::Result<()> {
        let json = serde_json::to_vec(&self).map_err(std::io::Error::other)?;
        crate::whole_file::replace(dir, CHECK_FILE, &json)
    }

    /// Whether a check is due at `now` with a list in use published at `published`: a week
    /// after the last check that ended current, a day after one that did not, and before
    /// any check, once the list in use is a week old. A check recorded after `now` ran while
    /// the clock was ahead, so how long ago it was is unknown, and it is due.
    fn due(self, published: i64, now: i64) -> bool {
        match self.at {
            None => now.saturating_sub(published) >= CHECK_EVERY_MILLIS,
            Some(at) if at > now => true,
            Some(at) if self.current => now.saturating_sub(at) >= CHECK_EVERY_MILLIS,
            Some(at) => now.saturating_sub(at) >= RETRY_AFTER_MILLIS,
        }
    }
}

/// Why a check left the list in use as it was.
#[derive(Debug, thiserror::Error)]
enum NotTaken {
    /// The list could not be fetched, or not kept on disk.
    #[error("{0}")]
    Failed(String),
    /// The list fetched failed a check of a downloaded list (D44.3).
    #[error("the list it got was refused: {0}")]
    Refused(Refused),
}

impl NotTaken {
    /// What the person can do before trying again.
    fn advice(&self) -> &'static str {
        match self {
            // The clock is behind, not the list ahead: upstream dates a list when it is built.
            Self::Refused(Refused::FromTheFuture) => {
                "check that your computer's date and time are right"
            }
            Self::Failed(_) | Self::Refused(_) => "check your internet connection",
        }
    }
}

fn list_info(state: &State) -> SuffixListInfo {
    SuffixListInfo {
        published: state.suffixes.published(),
        checked: state.suffix_check.at,
        check_failed: state.suffix_check.at.is_some() && !state.suffix_check.current,
    }
}

impl Manager {
    /// The Public Suffix List in use and when a newer one was last looked for (D44).
    #[must_use]
    pub fn suffix_list(&self) -> SuffixListInfo {
        list_info(&self.lock())
    }

    /// Looks for a newer Public Suffix List now, whatever
    /// [`update_suffix_list`](wirecatch_protocol::app::AppSettings::update_suffix_list)
    /// says, and takes it when it passes every check of a downloaded list (D44.3), is newer
    /// than the list in use and has been kept on disk. Answers with the list as it stands
    /// afterwards, which every window also receives as [`UiEvent::SuffixListChecked`],
    /// failure or not.
    ///
    /// # Errors
    ///
    /// [`CoreError::UpdateCheckFailed`], with a message for the person who asked, when the
    /// list could not be fetched, was refused or could not be kept, which leaves the list
    /// in use as it was; and [`CoreError::Closed`] once the manager has shut down.
    pub async fn check_suffix_list(&self) -> Result<SuffixListInfo> {
        let Some(source) = self.inner.suffix_list_source.clone() else {
            return Err(CoreError::UpdateCheckFailed(
                "This copy of Wirecatch does not download the Public Suffix List.".into(),
            ));
        };
        let _one_at_a_time = self.inner.suffix_checking.lock().await;
        self.check_suffix_list_from(&source).await
    }

    /// One check of the list at `source`, for a caller that holds `suffix_checking`; it
    /// ends by telling the windows the list as it stands, however it went.
    async fn check_suffix_list_from(&self, source: &Url) -> Result<SuffixListInfo> {
        let found = self.newer_suffix_list(source).await;
        let check = SuffixCheck {
            at: Some(self.inner.clock.unix_millis()),
            current: found.is_ok(),
        };
        let dir = self.inner.data_dir.clone();
        if let Err(e) = self.blocking(move || Ok(check.save(&dir))).await? {
            warn!(error = %e, "could not record the public suffix list check");
        }
        let mut state = self.lock_open()?;
        state.suffix_check = check;
        let failed = match found {
            Ok(Some(list)) => {
                info!(
                    published = list.published(),
                    "took a newer public suffix list"
                );
                state.suffixes = list;
                self.drop_sites_made_public(&mut state);
                None
            }
            Ok(None) => None,
            Err(reason) => {
                warn!(%reason, "kept the public suffix list in use");
                Some(reason)
            }
        };
        let info = list_info(&state);
        self.emit(UiEvent::SuffixListChecked(info));
        match failed {
            None => Ok(info),
            Some(reason) => Err(CoreError::UpdateCheckFailed(format!(
                "Wirecatch could not update its list of public domains ({reason}). It keeps using the list it has; {}, then try again.",
                reason.advice()
            ))),
        }
    }

    /// Drops the stored sites on the person's network that the list in use makes public,
    /// as a start does, so the settings pass their own checks again and any other
    /// setting can still change.
    fn drop_sites_made_public(&self, state: &mut State) {
        let kept = settings::stored_local_sites(&state.settings.local_sites, &state.suffixes);
        if kept != state.settings.local_sites {
            let mut new = state.settings.clone();
            new.local_sites = kept;
            self.apply_settings(state, new);
        }
    }

    /// The list at `source` when it may take the place of the list in use and is newer,
    /// once it is kept on disk; `None` when the list in use is as new. The error says why
    /// the list in use stays.
    async fn newer_suffix_list(
        &self,
        source: &Url,
    ) -> std::result::Result<Option<Arc<SuffixList>>, NotTaken> {
        let proxy = self.lock().app_proxy();
        let options = crate::headers::download_options(SUFFIX_NETWORK, proxy);
        let cancel = self.inner.stopped.child_token();
        let fetched = tokio::time::timeout(FETCH_TIMEOUT, async {
            let fetcher =
                Fetcher::with_private_network(source, &options, cancel, SUFFIX_NETWORK).await?;
            fetcher.get(source, None, MAX_LIST_BYTES).await
        })
        .await
        .map_err(|_| NotTaken::Failed("publicsuffix.org took too long to answer".into()))?
        .map_err(|e| NotTaken::Failed(e.to_string()))?;
        let current = self.lock().suffixes.clone();
        let now = self.inner.clock.unix_millis();
        let dir = self.inner.data_dir.clone();
        self.blocking(move || {
            let list = match SuffixList::accept(&fetched.body, &current, now) {
                Ok(list) => list,
                Err(refused) => return Ok(Err(NotTaken::Refused(refused))),
            };
            if list.published() <= current.published() {
                return Ok(Ok(None));
            }
            Ok(match SuffixList::keep(&dir, &fetched.body) {
                Ok(()) => Ok(Some(Arc::new(list))),
                Err(e) => Err(NotTaken::Failed(format!(
                    "the new list could not be saved: {e}"
                ))),
            })
        })
        .await
        .map_err(|e| NotTaken::Failed(e.to_string()))?
    }

    /// Runs a check when the weekly one is due, the setting is on and no download runs.
    /// Returns whether it ran one.
    pub(super) async fn update_suffix_list_if_due(&self) -> bool {
        let Some(source) = self.inner.suffix_list_source.clone() else {
            return false;
        };
        // Decided once no other check runs, so one that just ended counts.
        let _one_at_a_time = self.inner.suffix_checking.lock().await;
        let due = {
            let state = self.lock();
            state.settings.update_suffix_list
                && state.running.is_empty()
                && state
                    .suffix_check
                    .due(state.suffixes.published(), self.inner.clock.unix_millis())
        };
        if !due {
            return false;
        }
        if let Err(e) = self.check_suffix_list_from(&source).await {
            warn!(error = %e, "the public suffix list check failed");
        }
        true
    }

    /// Looks whether a check is due every [`LOOK_EVERY`], while the app runs.
    pub(super) async fn run_suffix_list_updates(
        weak: std::sync::Weak<super::Inner>,
        stopped: CancellationToken,
    ) {
        loop {
            tokio::select! {
                () = tokio::time::sleep(LOOK_EVERY) => {}
                () = stopped.cancelled() => return,
            }
            let Some(manager) = Manager::from_weak(&weak) else {
                return;
            };
            manager.update_suffix_list_if_due().await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ManagerConfig;
    use crate::clock::{Clock, ManualClock};
    use crate::folders::KnownFolders;
    use crate::public_suffix::testing;
    use wirecatch_test_support::files::FileServer;

    const DAY: i64 = 24 * 60 * 60 * 1000;
    const LIST: &str = "/list.dat";

    #[test]
    fn the_app_fetches_the_list_from_publicsuffix_org() {
        let config = ManagerConfig::new("data", KnownFolders::under_home("home"));
        assert_eq!(config.suffix_list_source, None);
        let official = config.with_official_suffix_list().suffix_list_source;
        assert_eq!(official.unwrap().as_str(), PUBLIC_SUFFIX_LIST_URL);
    }

    #[test]
    fn a_check_is_due_weekly_and_a_day_after_one_that_failed() {
        let published = 1_000 * DAY;
        let never = SuffixCheck::default();
        assert!(!never.due(published, published + 6 * DAY));
        assert!(never.due(published, published + 7 * DAY));
        let at = published + 10 * DAY;
        let current = SuffixCheck {
            at: Some(at),
            current: true,
        };
        assert!(!current.due(published, at + 6 * DAY));
        assert!(current.due(published, at + 7 * DAY));
        let failed = SuffixCheck {
            at: Some(at),
            current: false,
        };
        assert!(!failed.due(published, at + DAY - 1));
        assert!(failed.due(published, at + DAY));
    }

    #[test]
    fn a_check_recorded_after_now_is_due_once_the_clock_is_set_back() {
        let published = 1_000 * DAY;
        let now = published + 10 * DAY;
        for current in [true, false] {
            let ahead = SuffixCheck {
                at: Some(now + 365 * DAY),
                current,
            };
            assert!(ahead.due(published, now), "current: {current}");
        }
    }

    struct Harness {
        manager: Manager,
        server: FileServer,
        clock: Arc<ManualClock>,
        dir: tempfile::TempDir,
    }

    impl Harness {
        /// A manager whose list is fetched from a local server, on the day the newer test
        /// list is published, long after the built-in one, so a check is due.
        async fn new() -> Self {
            let server = FileServer::start().await;
            let dir = tempfile::tempdir().unwrap();
            let now = chrono::DateTime::from_timestamp_millis(testing::NEWER_MILLIS)
                .unwrap()
                .naive_utc();
            let clock = Arc::new(ManualClock::new(now));
            let manager = Self::open(dir.path(), &server, &clock).await;
            Self {
                manager,
                server,
                clock,
                dir,
            }
        }

        async fn open(dir: &Path, server: &FileServer, clock: &Arc<ManualClock>) -> Manager {
            let folders = KnownFolders::under_home(dir.join("home"));
            let config = ManagerConfig::new(dir.join("data"), folders)
                .with_clock(clock.clone())
                .with_suffix_list_source(server.url(LIST));
            Manager::open(config).await.unwrap()
        }

        fn serve(&self, version: &str, rules: &[&str]) {
            self.server.put(LIST, testing::list(version, rules));
        }

        fn set_updates(&self, on: bool) {
            let mut settings = self.manager.get_settings();
            settings.update_suffix_list = on;
            self.manager.update_settings(settings).unwrap();
        }

        fn add_local_site(&self, site: &str) -> Result<()> {
            let mut settings = self.manager.get_settings();
            settings.local_sites = vec![site.to_string()];
            self.manager.update_settings(settings)
        }
    }

    #[tokio::test]
    async fn with_updates_off_the_list_is_fetched_only_when_asked() {
        let h = Harness::new().await;
        h.serve(testing::NEWER, &[]);
        h.set_updates(false);
        assert!(!h.manager.update_suffix_list_if_due().await);
        assert_eq!(h.server.requests(LIST), 0);

        let checked = h.manager.check_suffix_list().await.unwrap();
        assert_eq!(h.server.requests(LIST), 1);
        assert_eq!(checked.published, testing::NEWER_MILLIS);

        h.set_updates(true);
        h.clock.advance(chrono::TimeDelta::days(7));
        assert!(h.manager.update_suffix_list_if_due().await);
        assert_eq!(h.server.requests(LIST), 2);
        // Weekly: not again the same week.
        assert!(!h.manager.update_suffix_list_if_due().await);
        assert_eq!(h.server.requests(LIST), 2);
    }

    #[tokio::test]
    async fn decisions_about_sites_use_the_newer_list_and_keep_it_across_a_start() {
        let mut h = Harness::new().await;
        h.add_local_site("customers.example.net").unwrap();
        h.serve(testing::NEWER, &["customers.example.net"]);
        let checked = h.manager.check_suffix_list().await.unwrap();
        assert_eq!(checked.published, testing::NEWER_MILLIS);
        let refused = h.add_local_site("customers.example.net").unwrap_err();
        assert!(matches!(refused, CoreError::Invalid(_)), "{refused:?}");

        h.manager.shutdown().await;
        h.manager = Harness::open(h.dir.path(), &h.server, &h.clock).await;
        assert_eq!(h.manager.suffix_list(), checked);
        assert!(h.add_local_site("customers.example.net").is_err());
        // The stored site that the newer list makes public is dropped at start.
        assert_eq!(h.manager.get_settings().local_sites, Vec::<String>::new());
    }

    // The newer list drops the stored site at once, as a start does, so the settings
    // still pass their checks and every other setting can change.
    #[tokio::test]
    async fn other_settings_still_change_after_a_newer_list_makes_a_stored_site_public() {
        let h = Harness::new().await;
        h.add_local_site("customers.example.net").unwrap();
        h.serve(testing::NEWER, &["customers.example.net"]);
        h.manager.check_suffix_list().await.unwrap();
        assert_eq!(h.manager.get_settings().local_sites, Vec::<String>::new());
        let mut settings = h.manager.get_settings();
        settings.connections_per_download = 3;
        h.manager.update_settings(settings).unwrap();
    }

    // Turning the refresh off is a privacy choice: a profile from before the setting
    // existed says nothing about it, so importing one leaves it as it is.
    #[tokio::test]
    async fn a_profile_without_the_setting_leaves_the_refresh_as_it_is() {
        let h = Harness::new().await;
        h.set_updates(false);
        let path = h.dir.path().join("profile.json");
        h.manager.export_settings(&path).await.unwrap();
        let mut profile: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        let settings = profile["settings"].as_object_mut().unwrap();
        assert_eq!(settings.remove("update_suffix_list"), Some(false.into()));
        std::fs::write(&path, serde_json::to_vec(&profile).unwrap()).unwrap();

        h.manager.import_settings(&path).await.unwrap();
        assert!(!h.manager.get_settings().update_suffix_list);
    }

    // The weekly check runs out of sight, and a failed Check now changes when the list was
    // last looked at, so the settings page hears of each one.
    #[tokio::test]
    async fn every_check_tells_the_windows_how_it_ended() {
        let h = Harness::new().await;
        let mut events = h.manager.subscribe();
        let checked = |events: &mut tokio::sync::broadcast::Receiver<UiEvent>| loop {
            match events.try_recv() {
                Ok(UiEvent::SuffixListChecked(info)) => break info,
                Ok(_) => {}
                Err(e) => panic!("no SuffixListChecked: {e}"),
            }
        };
        h.serve(testing::NEWER, &[]);
        assert!(h.manager.update_suffix_list_if_due().await);
        let info = checked(&mut events);
        assert_eq!(info, h.manager.suffix_list());
        assert_eq!(
            (info.published, info.check_failed),
            (testing::NEWER_MILLIS, false)
        );

        h.server.put(LIST, "not a list");
        h.manager.check_suffix_list().await.unwrap_err();
        let info = checked(&mut events);
        assert_eq!(info, h.manager.suffix_list());
        assert_eq!(
            (info.published, info.check_failed),
            (testing::NEWER_MILLIS, true)
        );
    }

    // Whether the weekly check is due is decided once no other check runs, so one that
    // waited for a Check now does not fetch the list again right after it.
    #[tokio::test]
    async fn a_weekly_check_that_waited_for_another_does_not_fetch_again() {
        let h = Harness::new().await;
        h.serve(testing::NEWER, &[]);
        let other = h.manager.inner.suffix_checking.lock().await;
        let weekly = tokio::spawn({
            let manager = h.manager.clone();
            async move { manager.update_suffix_list_if_due().await }
        });
        // The test runs on one thread: this lets the weekly check run until it waits.
        tokio::task::yield_now().await;
        h.manager.lock().suffix_check = SuffixCheck {
            at: Some(h.clock.unix_millis()),
            current: true,
        };
        drop(other);
        assert!(!weekly.await.unwrap());
        assert_eq!(h.server.requests(LIST), 0);
    }

    #[tokio::test]
    async fn a_list_refused_or_not_kept_leaves_the_list_in_use() {
        let h = Harness::new().await;
        let before = h.manager.suffix_list();
        let whole = testing::list(testing::NEWER, &["customers.example.net"]);
        h.server.put(LIST, whole[..whole.len() / 2].to_string());
        let refused = h.manager.check_suffix_list().await.unwrap_err();
        assert!(
            matches!(&refused, CoreError::UpdateCheckFailed(m) if m.contains("cut short")),
            "{refused:?}"
        );
        assert_eq!(h.manager.suffix_list().published, before.published);
        h.add_local_site("customers.example.net").unwrap();

        // A whole list that cannot be kept on disk is not used either.
        let data = h.dir.path().join("data");
        std::fs::create_dir(data.join("public-suffix-list.dat.tmp")).unwrap();
        h.server.put(LIST, whole);
        let unkept = h.manager.check_suffix_list().await.unwrap_err();
        assert!(
            matches!(unkept, CoreError::UpdateCheckFailed(_)),
            "{unkept:?}"
        );
        assert_eq!(h.manager.suffix_list().published, before.published);
        h.add_local_site("customers.example.net").unwrap();
        assert!(!data.join("public-suffix-list.dat").exists());
    }

    #[tokio::test]
    async fn a_list_refused_as_published_after_today_asks_about_the_clock() {
        let h = Harness::new().await;
        h.serve("2199-01-01_00-00-00_UTC", &[]);
        let refused = h.manager.check_suffix_list().await.unwrap_err();
        let CoreError::UpdateCheckFailed(message) = refused else {
            panic!("{refused:?}");
        };
        assert!(message.contains("published after today"), "{message}");
        assert!(message.contains("date and time"), "{message}");
        assert!(!message.contains("internet connection"), "{message}");
    }

    #[tokio::test]
    async fn the_weekly_check_waits_while_a_download_runs() {
        let h = Harness::new().await;
        h.serve(testing::NEWER, &[]);
        h.server.put("/slow.bin", vec![0u8; 1024]);
        h.server.hang("/slow.bin", true);
        let id = h
            .manager
            .add_download(wirecatch_protocol::app::AddRequest {
                url: h.server.url("/slow.bin").to_string().into(),
                queue: None,
                folder: None,
                file_name: None,
                start: true,
            })
            .await
            .unwrap();
        assert!(!h.manager.update_suffix_list_if_due().await);
        assert_eq!(h.server.requests(LIST), 0);

        h.manager.remove(&[id], true).await.unwrap();
        assert!(h.manager.update_suffix_list_if_due().await);
        assert_eq!(h.server.requests(LIST), 1);
    }

    #[tokio::test]
    async fn a_list_no_newer_than_the_one_in_use_is_not_taken() {
        let h = Harness::new().await;
        h.serve("2020-01-01_00-00-00_UTC", &["customers.example.net"]);
        let checked = h.manager.check_suffix_list().await.unwrap();
        assert_eq!(checked.published, SuffixList::built_in().published());
        assert!(checked.checked.is_some());
        h.add_local_site("customers.example.net").unwrap();
    }
}

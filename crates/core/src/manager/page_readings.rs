//! Recent readings of video pages, kept in memory only: they hold signed format addresses and sometimes cookies, which stay out of every folder.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use wirecatch_engine::CancellationToken;
use wirecatch_media::ytdlp::Info;
use wirecatch_protocol::{DownloadId, UnixMillis};

/// How long a reading serves another start of its page: a pause and resume soon after reads nothing again, while a later one meets addresses that may have expired.
pub(crate) const READING_LIFETIME: Duration = Duration::from_mins(30);

/// The most readings kept; a page with many formats takes hundreds of kilobytes.
const MOST_KEPT: usize = 32;

/// Readings by their owning download, each with when it was made.
#[derive(Default)]
pub(crate) struct PageReadings {
    recent: Mutex<HashMap<DownloadId, (UnixMillis, Arc<Info>)>>,
}

impl PageReadings {
    fn recent(&self) -> MutexGuard<'_, HashMap<DownloadId, (UnixMillis, Arc<Info>)>> {
        // A panic while holding it leaves a map that is still whole.
        self.recent.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Keeps a reading unless the run that made it has been stopped.
    pub(crate) fn put(
        &self,
        owner: DownloadId,
        reading: (UnixMillis, Arc<Info>),
        now: UnixMillis,
        cancel: &CancellationToken,
    ) {
        let mut recent = self.recent();
        if cancel.is_cancelled() || !fresh(reading.0, now) {
            return;
        }
        recent.retain(|_, (at, _)| fresh(*at, now));
        if recent.len() >= MOST_KEPT
            && let Some(oldest) = recent
                .iter()
                .min_by_key(|(_, (at, _))| *at)
                .map(|(owner, _)| *owner)
        {
            recent.remove(&oldest);
        }
        recent.insert(owner, reading);
    }

    /// The reading of `page` made less than [`READING_LIFETIME`] before `now`.
    pub(crate) fn fresh(&self, owner: DownloadId, now: UnixMillis) -> Option<Arc<Info>> {
        let recent = self.recent();
        let (at, info) = recent.get(&owner)?;
        fresh(*at, now).then(|| info.clone())
    }

    /// Drops the reading of `page`, whose addresses stopped working or are no longer needed.
    pub(crate) fn forget(&self, owner: DownloadId) {
        self.recent().remove(&owner);
    }
}

/// A clock set back says nothing about a reading's age, so it counts as old.
fn fresh(at: UnixMillis, now: UnixMillis) -> bool {
    now >= at && now.since(at) < READING_LIFETIME
}

#[cfg(test)]
mod tests {
    use super::*;

    const AT: UnixMillis = UnixMillis(1_000);

    #[test]
    fn a_reading_serves_only_its_owner_until_it_is_too_old_or_forgotten() {
        let readings = PageReadings::default();
        let page = DownloadId(1);
        let other = DownloadId(2);
        readings.put(
            page,
            (AT, Arc::new(Info::default())),
            AT,
            &CancellationToken::new(),
        );
        let almost = AT.saturating_add(READING_LIFETIME.saturating_sub(Duration::from_millis(1)));

        assert!(readings.fresh(page, almost).is_some());
        assert!(
            readings
                .fresh(page, AT.saturating_add(READING_LIFETIME))
                .is_none()
        );
        assert!(readings.fresh(other, AT).is_none());
        assert!(readings.fresh(page, UnixMillis(999)).is_none());
        readings.forget(page);
        assert!(readings.fresh(page, AT).is_none());
    }

    #[test]
    fn confirming_an_older_selection_keeps_a_newer_reading() {
        let readings = PageReadings::default();
        let running = CancellationToken::new();
        let older = DownloadId(1);
        let newer = DownloadId(2);
        let later = AT.saturating_add(Duration::from_secs(1));
        readings.put(newer, (later, Arc::new(Info::default())), later, &running);
        readings.put(older, (AT, Arc::new(Info::default())), later, &running);

        assert!(readings.fresh(newer, later).is_some());
        let older_expired = AT.saturating_add(READING_LIFETIME);
        assert!(readings.fresh(older, older_expired).is_none());
        assert!(readings.fresh(newer, older_expired).is_some());
    }

    #[test]
    fn the_oldest_reading_makes_room() {
        let readings = PageReadings::default();
        let running = CancellationToken::new();
        let page = |n: usize| DownloadId(i64::try_from(n).unwrap());
        let at = |n: usize| AT.saturating_add(Duration::from_millis(n as u64));
        for n in 0..=MOST_KEPT {
            let time = at(n);
            readings.put(page(n), (time, Arc::new(Info::default())), time, &running);
        }
        let now = at(MOST_KEPT);
        assert!(readings.fresh(page(0), now).is_none());
        assert!(readings.fresh(page(1), now).is_some());
        assert!(readings.fresh(page(MOST_KEPT), now).is_some());
    }
}

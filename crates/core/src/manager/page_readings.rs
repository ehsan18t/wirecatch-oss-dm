//! Recent readings of video pages, kept in memory only: they hold signed format addresses and sometimes cookies, which stay out of every folder.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use wirecatch_engine::Url;
use wirecatch_media::ytdlp::Info;
use wirecatch_protocol::UnixMillis;

/// How long a reading serves another start of its page: a pause and resume soon after reads nothing again, while a later one meets addresses that may have expired.
pub(crate) const READING_LIFETIME: Duration = Duration::from_mins(30);

/// The most readings kept; a page with many formats takes hundreds of kilobytes.
const MOST_KEPT: usize = 32;

/// Readings by page link, each with when it was made.
#[derive(Default)]
pub(crate) struct PageReadings {
    recent: Mutex<HashMap<String, (UnixMillis, Arc<Info>)>>,
}

impl PageReadings {
    fn recent(&self) -> MutexGuard<'_, HashMap<String, (UnixMillis, Arc<Info>)>> {
        // A panic while holding it leaves a map that is still whole.
        self.recent.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Keeps `info`, read from `page` at `now`.
    pub(crate) fn put(&self, page: &Url, now: UnixMillis, info: Arc<Info>) {
        let mut recent = self.recent();
        recent.retain(|_, (at, _)| fresh(*at, now));
        if recent.len() >= MOST_KEPT
            && let Some(oldest) = recent
                .iter()
                .min_by_key(|(_, (at, _))| *at)
                .map(|(page, _)| page.clone())
        {
            recent.remove(&oldest);
        }
        recent.insert(page.as_str().to_owned(), (now, info));
    }

    /// The reading of `page` made less than [`READING_LIFETIME`] before `now`.
    pub(crate) fn fresh(&self, page: &Url, now: UnixMillis) -> Option<Arc<Info>> {
        let recent = self.recent();
        let (at, info) = recent.get(page.as_str())?;
        fresh(*at, now).then(|| info.clone())
    }

    /// Drops the reading of `page`, whose addresses stopped working or are no longer needed.
    pub(crate) fn forget(&self, page: &Url) {
        self.recent().remove(page.as_str());
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
    fn a_reading_serves_its_page_until_it_is_too_old_or_forgotten() {
        let readings = PageReadings::default();
        let page = Url::parse("https://videos.example/watch?v=1").unwrap();
        let other = Url::parse("https://videos.example/watch?v=2").unwrap();
        readings.put(&page, AT, Arc::new(Info::default()));
        let almost = AT.saturating_add(READING_LIFETIME.saturating_sub(Duration::from_millis(1)));

        assert!(readings.fresh(&page, almost).is_some());
        assert!(
            readings
                .fresh(&page, AT.saturating_add(READING_LIFETIME))
                .is_none()
        );
        assert!(readings.fresh(&other, AT).is_none());
        assert!(readings.fresh(&page, UnixMillis(999)).is_none());
        readings.forget(&page);
        assert!(readings.fresh(&page, AT).is_none());
    }

    #[test]
    fn the_oldest_reading_makes_room() {
        let readings = PageReadings::default();
        let page = |n: usize| Url::parse(&format!("https://videos.example/{n}")).unwrap();
        let at = |n: usize| AT.saturating_add(Duration::from_millis(n as u64));
        for n in 0..=MOST_KEPT {
            readings.put(&page(n), at(n), Arc::new(Info::default()));
        }
        let now = at(MOST_KEPT);
        assert!(readings.fresh(&page(0), now).is_none());
        assert!(readings.fresh(&page(1), now).is_some());
        assert!(readings.fresh(&page(MOST_KEPT), now).is_some());
    }
}

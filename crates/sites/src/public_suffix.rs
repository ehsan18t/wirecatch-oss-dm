//! The Public Suffix List: the names under which anyone may register a site of
//! their own (`com`, `co.uk`, `github.io`), so a name at or above one never
//! stands for one owner's sites (D39). The app owns the list (D44): a snapshot
//! of publicsuffix.org's file is built into each release, and every question
//! core asks goes to the [`SuffixList`] the manager holds, so a newer list can
//! take its place while the app runs.

use std::io::{self, Read};
use std::path::Path;
use std::str::FromStr;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use publicsuffix::{List, Psl};
use wirecatch_engine::units::MIB;
use wirecatch_protocol::UnixMillis;

/// The snapshot built into this release, publicsuffix.org's file as published.
const BUILT_IN: &str = include_str!("../data/public_suffix_list.dat");

/// The lines that open and close the file's two sections, in order. The last
/// closes the file, so a list without it was cut short.
const SECTIONS: [&str; 4] = [
    "// ===BEGIN ICANN DOMAINS===",
    "// ===END ICANN DOMAINS===",
    "// ===BEGIN PRIVATE DOMAINS===",
    "// ===END PRIVATE DOMAINS===",
];

/// The file a downloaded list is kept in, in the data folder.
pub const KEPT_FILE: &str = "public-suffix-list.dat";

/// The line of the file that says when it was published, before the time.
const VERSION_LINE: &str = "// VERSION: ";

/// The largest list accepted. The file was about 330 KB in 2026 and grows by a few KB a month.
pub const MAX_LIST_BYTES: u64 = 2 * MIB;

/// How far past the clock a list may say it was published, for a clock a little behind.
const CLOCK_SLACK: Duration = Duration::from_hours(24);

/// A Public Suffix List, both its ICANN and its private section.
pub struct SuffixList {
    rules: List,
    /// How many rules it holds.
    count: usize,
    /// How many of them are in its ICANN section.
    icann: usize,
    /// When publicsuffix.org published it.
    published: UnixMillis,
}

impl std::fmt::Debug for SuffixList {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SuffixList")
            .field("count", &self.count)
            .field("icann", &self.icann)
            .field("published", &self.published)
            .finish_non_exhaustive()
    }
}

/// Why a list was not taken.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Refused {
    /// Larger than [`MAX_LIST_BYTES`].
    #[error("it is larger than {MAX_LIST_BYTES} bytes")]
    TooLarge,
    /// It stops before the end of its private section, as a transfer cut short does.
    #[error("it is cut short: its last section does not end")]
    Incomplete,
    /// It has no `VERSION` line, so whether it is newer cannot be told.
    #[error("it does not say when it was published")]
    Undated,
    /// Not UTF-8, or a rule the reader refuses.
    #[error("it cannot be read ({0})")]
    Unreadable(String),
    /// Under nine tenths of the rules of the list in use, in all or in its ICANN section,
    /// which a list could otherwise trade for private rules.
    #[error("it holds {rules} {section}rules, far fewer than the {current} of the list in use")]
    TooSmall {
        /// `"ICANN "` when only that section shrank, otherwise empty.
        section: &'static str,
        /// The rules it holds.
        rules: usize,
        /// The rules of the list in use.
        current: usize,
    },
    /// Over twice the rules of the list in use. The list grows by a few hundred rules a
    /// year; a list padded with rules would refuse every real one after it as much smaller.
    #[error("it holds {rules} rules, far more than the {current} of the list in use")]
    TooMany {
        /// The rules it holds.
        rules: usize,
        /// The rules of the list in use.
        current: usize,
    },
    /// Published more than a day after the clock's time. Taken, it would hold off every
    /// real list after it as older.
    #[error("it says it was published after today")]
    FromTheFuture,
}

impl SuffixList {
    /// The list built into this release, read once per process.
    pub fn built_in() -> Arc<Self> {
        static LIST: OnceLock<Arc<SuffixList>> = OnceLock::new();
        LIST.get_or_init(|| {
            Arc::new(Self::read(BUILT_IN.as_bytes()).unwrap_or_else(|e| {
                // A test reads the snapshot, so this is never reached. The list's
                // implicit `*` rule alone still treats every last label as a suffix,
                // and any list downloaded later counts as newer.
                tracing::error!(error = %e, "the built-in public suffix list is unreadable");
                Self {
                    rules: List::new(),
                    count: 0,
                    icann: 0,
                    published: UnixMillis(0),
                }
            }))
        })
        .clone()
    }

    /// `bytes` as a list that may take the place of `current` at `now`: one `read` takes that holds at least nine tenths of
    /// `current`'s rules and of its ICANN rules, at most twice its rules, and says it was
    /// published no later than a day after `now`. A list loses a few rules from one week to
    /// the next; one that lost more is not the list, or was cut short where no section
    /// marker shows it. One far larger or dated ahead would stay in use for good, since
    /// every real list after it would look much smaller or older.
    pub fn accept(bytes: &[u8], current: &Self, now: UnixMillis) -> Result<Self, Refused> {
        let list = Self::read(bytes)?;
        let too_small = |rules: usize, current: usize| rules * 10 < current * 9;
        if too_small(list.count, current.count) {
            return Err(Refused::TooSmall {
                section: "",
                rules: list.count,
                current: current.count,
            });
        }
        if too_small(list.icann, current.icann) {
            return Err(Refused::TooSmall {
                section: "ICANN ",
                rules: list.icann,
                current: current.icann,
            });
        }
        if list.count > current.count.saturating_mul(2) {
            return Err(Refused::TooMany {
                rules: list.count,
                current: current.count,
            });
        }
        if list.published > now.saturating_add(CLOCK_SLACK) {
            return Err(Refused::FromTheFuture);
        }
        Ok(list)
    }

    /// `bytes` as a list: at most [`MAX_LIST_BYTES`], UTF-8, dated by its `VERSION` line,
    /// with its four section markers in order and every rule one the reader takes.
    fn read(bytes: &[u8]) -> Result<Self, Refused> {
        if u64::try_from(bytes.len()).map_or(true, |len| len > MAX_LIST_BYTES) {
            return Err(Refused::TooLarge);
        }
        let text = std::str::from_utf8(bytes).map_err(|e| Refused::Unreadable(e.to_string()))?;
        let Counted { count, icann } = whole(text)?;
        let published = published(text).ok_or(Refused::Undated)?;
        let rules = List::from_str(text).map_err(|e| Refused::Unreadable(e.to_string()))?;
        Ok(Self {
            rules,
            count,
            icann,
            published,
        })
    }

    /// The list to use at start, at `now`: the one kept in `dir`, unless it is damaged, not
    /// newer than the built-in one (which an update of the app may have overtaken it with),
    /// or one [`accept`](Self::accept) would refuse today. Blocks on the disk.
    pub fn load(dir: &Path, now: UnixMillis) -> Arc<Self> {
        let built_in = Self::built_in();
        let bytes = match read_kept(&dir.join(KEPT_FILE)) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return built_in,
            Err(e) => {
                tracing::warn!(error = %e, "the kept public suffix list cannot be read; using the built-in one");
                return built_in;
            }
        };
        match Self::accept(&bytes, &built_in, now) {
            Ok(kept) if kept.published > built_in.published => Arc::new(kept),
            Ok(_) => built_in,
            Err(reason) => {
                tracing::warn!(%reason, "the kept public suffix list is refused; using the built-in one");
                built_in
            }
        }
    }

    /// The registrable domain `name` is or is under (`example.com` for
    /// `wiki.corp.example.com`), or `None` when `name` is a public suffix or
    /// above one, which no single owner holds. `name` is a host name as the URL
    /// parser writes it: lower case, in ASCII form, without a trailing dot. A
    /// last label the list does not know counts as a suffix of its own, as the
    /// list's implicit `*` rule says, so `corp.mycompany` is registrable and
    /// `mycompany` is not.
    #[must_use]
    pub fn registrable_domain<'a>(&self, name: &'a str) -> Option<&'a str> {
        let domain = self.rules.domain(name.as_bytes())?;
        std::str::from_utf8(domain.as_bytes()).ok()
    }

    /// Whether `name` (written as for [`registrable_domain`](Self::registrable_domain))
    /// is itself on the list (`com`, `ai`, `xn--p1ai`, `co.uk`, `github.io`), so
    /// public DNS may serve it. A single label the list does not know (`nas`) is not.
    #[must_use]
    pub fn is_public_suffix(&self, name: &str) -> bool {
        self.rules
            .suffix(name.as_bytes())
            .is_some_and(|suffix| suffix.is_known() && suffix.as_bytes() == name.as_bytes())
    }

    /// When publicsuffix.org published this list.
    #[must_use]
    pub fn published(&self) -> UnixMillis {
        self.published
    }
}

/// The kept list's bytes, reading at most one byte more than [`accept`](SuffixList::accept)
/// takes, so a huge file costs nothing.
fn read_kept(path: &Path) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take(MAX_LIST_BYTES + 1)
        .read_to_end(&mut bytes)?;
    Ok(bytes)
}

/// How many rules a list holds.
struct Counted {
    /// In both sections.
    count: usize,
    /// In its ICANN section.
    icann: usize,
}

/// How many rules `text` holds in its sections, when it has all four section
/// markers in order.
fn whole(text: &str) -> Result<Counted, Refused> {
    let mut next = 0;
    let mut counted = Counted { count: 0, icann: 0 };
    for line in text.lines().map(str::trim_end) {
        if next < SECTIONS.len() && line == SECTIONS[next] {
            next += 1;
        } else if next % 2 == 1 && !line.is_empty() && !line.starts_with("//") {
            counted.count += 1;
            if next == 1 {
                counted.icann += 1;
            }
        }
    }
    if next == SECTIONS.len() {
        Ok(counted)
    } else {
        Err(Refused::Incomplete)
    }
}

/// When the list in `text` was published, from its `VERSION` line
/// (`// VERSION: 2026-09-15_10-18-26_UTC`).
fn published(text: &str) -> Option<UnixMillis> {
    let version = text
        .lines()
        .take_while(|line| line.is_empty() || line.starts_with("//"))
        .find_map(|line| line.strip_prefix(VERSION_LINE))?;
    let at = chrono::NaiveDateTime::parse_from_str(version.trim(), "%Y-%m-%d_%H-%M-%S_UTC").ok()?;
    Some(UnixMillis(at.and_utc().timestamp_millis()))
}

/// Lists for tests, all made from the built-in snapshot.
#[cfg(any(test, feature = "testing"))]
pub mod testing {
    use super::{BUILT_IN, SECTIONS, UnixMillis, VERSION_LINE};

    /// A `VERSION` later than any snapshot's.
    pub const NEWER: &str = "2099-01-01_00-00-00_UTC";
    /// [`NEWER`] as a moment.
    pub const NEWER_AT: UnixMillis = UnixMillis(4_070_908_800_000);

    /// The built-in snapshot as published at `version` (`2099-01-01_00-00-00_UTC`), with
    /// `rules` added at the start of its private section.
    #[must_use]
    pub fn list(version: &str, rules: &[&str]) -> String {
        let at = BUILT_IN.find(VERSION_LINE).unwrap_or_default() + VERSION_LINE.len();
        let end = at + BUILT_IN[at..].find('\n').unwrap_or_default();
        let text = format!("{}{version}{}", &BUILT_IN[..at], &BUILT_IN[end..]);
        let private = format!("{}\n", SECTIONS[2]);
        let added: String = rules.iter().flat_map(|rule| [*rule, "\n"]).collect();
        text.replacen(&private, &format!("{private}{added}"), 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wirecatch_test_support::private_dir;

    #[test]
    fn the_list_knows_both_its_sections_and_its_wildcards() {
        let list = SuffixList::built_in();
        for suffix in ["com", "ai", "xn--p1ai", "co.uk", "github.io", "anything.ck"] {
            assert!(list.is_public_suffix(suffix), "{suffix}");
            assert_eq!(list.registrable_domain(suffix), None, "{suffix}");
        }
        for name in ["nas", "example.com", "team.github.io", "corp.mycompany"] {
            assert!(!list.is_public_suffix(name), "{name}");
        }
        assert_eq!(
            list.registrable_domain("wiki.corp.example.com"),
            Some("example.com")
        );
        assert_eq!(
            list.registrable_domain("team.github.io"),
            Some("team.github.io")
        );
        assert_eq!(
            list.registrable_domain("corp.mycompany"),
            Some("corp.mycompany")
        );
        assert_eq!(list.registrable_domain("mycompany"), None);
    }

    #[test]
    fn the_built_in_list_is_read_whole() {
        let list = SuffixList::built_in();
        assert!(list.count > 9_000, "{}", list.count);
        // 2026-09-15_10-18-26_UTC, or a later snapshot.
        assert!(
            list.published() >= UnixMillis(1_789_467_506_000),
            "{:?}",
            list.published()
        );
    }

    fn newer() -> String {
        testing::list(testing::NEWER, &[])
    }

    /// `text` without every `nth` rule.
    fn without_every(text: &str, nth: usize) -> String {
        let mut rule = 0;
        text.lines()
            .filter(|line| {
                if line.is_empty() || line.starts_with("//") {
                    return true;
                }
                rule += 1;
                rule % nth != 0
            })
            .flat_map(|line| [line, "\n"])
            .collect()
    }

    fn accept(text: impl AsRef<[u8]>) -> Result<SuffixList, Refused> {
        SuffixList::accept(text.as_ref(), &SuffixList::built_in(), testing::NEWER_AT)
    }

    #[test]
    fn a_whole_newer_list_is_accepted() {
        let list = accept(newer()).unwrap();
        assert!(list.is_public_suffix("github.io"));
        assert_eq!(list.published(), testing::NEWER_AT);
    }

    #[test]
    fn a_list_cut_short_is_refused() {
        let text = newer();
        assert_eq!(
            accept(&text[..text.len() / 2]).unwrap_err(),
            Refused::Incomplete
        );
        let last = text.trim_end().rfind('\n').unwrap();
        assert_eq!(accept(&text[..last]).unwrap_err(), Refused::Incomplete);
    }

    #[test]
    fn a_list_over_the_size_cap_is_refused() {
        let mut text = newer();
        while u64::try_from(text.len()).unwrap() <= MAX_LIST_BYTES {
            text.push_str("// padding\n");
        }
        assert_eq!(accept(&text).unwrap_err(), Refused::TooLarge);
    }

    #[test]
    fn a_list_that_does_not_read_is_refused() {
        let text = newer();
        let begin = "// ===BEGIN ICANN DOMAINS===\n";
        for bad in ["a..b\n", "!com\n"] {
            let broken = text.replacen(begin, &format!("{begin}{bad}"), 1);
            assert!(
                matches!(accept(&broken), Err(Refused::Unreadable(_))),
                "{bad}"
            );
        }
        let mut bytes = text.into_bytes();
        bytes.insert(0, 0xff);
        assert!(matches!(accept(&bytes), Err(Refused::Unreadable(_))));
    }

    #[test]
    fn an_undated_list_is_refused() {
        let text = newer();
        let undated: String = text
            .lines()
            .filter(|line| !line.starts_with(VERSION_LINE))
            .flat_map(|line| [line, "\n"])
            .collect();
        assert_eq!(accept(&undated).unwrap_err(), Refused::Undated);
    }

    #[test]
    fn a_list_much_smaller_than_the_one_in_use_is_refused() {
        assert!(matches!(
            accept(without_every(&newer(), 4)),
            Err(Refused::TooSmall { .. })
        ));
        // A list may lose a few rules from one week to the next.
        accept(without_every(&newer(), 20)).unwrap();
    }

    const DAY: i64 = 24 * 60 * 60 * 1000;

    #[test]
    fn a_list_dated_after_today_is_refused() {
        let at = |now| SuffixList::accept(newer().as_bytes(), &SuffixList::built_in(), now);
        assert_eq!(
            at(UnixMillis(testing::NEWER_AT.0 - 2 * DAY)).unwrap_err(),
            Refused::FromTheFuture
        );
        // A clock a few hours behind still takes it.
        at(UnixMillis(testing::NEWER_AT.0 - DAY / 2)).unwrap();
    }

    #[test]
    fn a_list_much_larger_than_the_one_in_use_is_refused() {
        let built_in = SuffixList::built_in().count;
        let padding: Vec<String> = (0..=built_in).map(|n| format!("pad{n}.example")).collect();
        let padding: Vec<&str> = padding.iter().map(String::as_str).collect();
        assert!(matches!(
            accept(testing::list(testing::NEWER, &padding)),
            Err(Refused::TooMany { .. })
        ));
        // The list grows a little each week.
        accept(testing::list(testing::NEWER, &padding[..built_in / 10])).unwrap();
    }

    #[test]
    fn a_list_that_trades_icann_rules_for_private_ones_is_refused() {
        let text = newer();
        let (icann, rest) = text.split_at(text.find(SECTIONS[1]).unwrap());
        let traded = format!("{}{rest}", without_every(icann, 4));
        let lost =
            SuffixList::built_in().icann - SuffixList::read(traded.as_bytes()).unwrap().icann;
        let padding: Vec<String> = (0..lost).map(|n| format!("pad{n}.example")).collect();
        let padding: Vec<&str> = padding.iter().map(String::as_str).collect();
        let private = format!(
            "{}
",
            SECTIONS[2]
        );
        let added: String = padding
            .iter()
            .flat_map(|rule| {
                [
                    *rule, "
",
                ]
            })
            .collect();
        let traded = traded.replacen(&private, &format!("{private}{added}"), 1);
        assert!(matches!(
            accept(traded),
            Err(Refused::TooSmall {
                section: "ICANN ",
                ..
            })
        ));
    }

    // A list kept when the clock was ahead, or from a server that dated it ahead, would
    // otherwise outlast every list after it, an update of the app's included.
    #[test]
    fn a_kept_list_dated_after_today_gives_way_to_the_built_in_one() {
        let dir = private_dir("suffix-kept-ahead").unwrap();
        keep(dir.path(), newer().as_bytes());
        assert_eq!(
            SuffixList::load(dir.path(), UnixMillis(testing::NEWER_AT.0 - 2 * DAY)).published(),
            SuffixList::built_in().published()
        );
    }

    #[test]
    fn a_kept_list_newer_than_the_built_in_one_is_read_back() {
        let dir = private_dir("suffix-kept").unwrap();
        keep(dir.path(), newer().as_bytes());
        assert_eq!(
            SuffixList::load(dir.path(), testing::NEWER_AT).published(),
            testing::NEWER_AT
        );
    }

    // An update of the app may bring a built-in list newer than the one kept, and a kept
    // list may be damaged; neither is read.
    #[test]
    fn a_kept_list_that_is_older_or_damaged_gives_way_to_the_built_in_one() {
        let dir = private_dir("suffix-kept-old").unwrap();
        let built_in = SuffixList::built_in().published();
        assert_eq!(
            SuffixList::load(dir.path(), testing::NEWER_AT).published(),
            built_in
        );
        let older = testing::list("2020-01-01_00-00-00_UTC", &[]);
        keep(dir.path(), older.as_bytes());
        assert_eq!(
            SuffixList::load(dir.path(), testing::NEWER_AT).published(),
            built_in
        );
        let text = newer();
        keep(dir.path(), &text.as_bytes()[..text.len() / 2]);
        assert_eq!(
            SuffixList::load(dir.path(), testing::NEWER_AT).published(),
            built_in
        );
    }

    fn keep(dir: &Path, bytes: &[u8]) {
        std::fs::write(dir.join(KEPT_FILE), bytes).unwrap();
    }
}

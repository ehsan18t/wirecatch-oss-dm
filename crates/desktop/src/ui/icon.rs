//! Icons from gpui-kit's complete Lucide catalog, plus lookup and search for queue icons.

use std::collections::HashMap;
use std::sync::OnceLock;

use gpui_kit::assets::IconName;
use wirecatch_protocol::app::Category;

pub use gpui_kit::assets::AllAssets as Assets;

/// The icon a queue draws when none has been picked.
pub const DEFAULT_QUEUE_ICON: IconName = IconName::ListOrdered;

/// Curated queue icons shown before a search query is typed.
const FEATURED_QUEUE_ICONS: &[IconName] = &[
    IconName::ListOrdered,
    IconName::Film,
    IconName::Video,
    IconName::Tv,
    IconName::Clapperboard,
    IconName::Music,
    IconName::Headphones,
    IconName::Album,
    IconName::Radio,
    IconName::Mic,
    IconName::Image,
    IconName::Camera,
    IconName::Folder,
    IconName::FolderArchive,
    IconName::FileArchive,
    IconName::FileText,
    IconName::BookOpen,
    IconName::Newspaper,
    IconName::Inbox,
    IconName::Download,
    IconName::HardDrive,
    IconName::Database,
    IconName::Package,
    IconName::Box,
    IconName::Magnet,
    IconName::Globe,
    IconName::Cloud,
    IconName::Wifi,
    IconName::Link,
    IconName::Rss,
    IconName::Share2,
    IconName::Compass,
    IconName::Gamepad2,
    IconName::Terminal,
    IconName::Code,
    IconName::Cpu,
    IconName::Wrench,
    IconName::Settings,
    IconName::Bot,
    IconName::Sparkles,
    IconName::Blocks,
    IconName::Briefcase,
    IconName::GraduationCap,
    IconName::FlaskConical,
    IconName::Palette,
    IconName::Brush,
    IconName::PenTool,
    IconName::Lightbulb,
    IconName::Star,
    IconName::Heart,
    IconName::Bookmark,
    IconName::Tag,
    IconName::Flag,
    IconName::Shield,
    IconName::Lock,
    IconName::Key,
    IconName::Clock,
    IconName::Moon,
    IconName::Sun,
    IconName::Calendar,
    IconName::Timer,
    IconName::Zap,
    IconName::Flame,
    IconName::Rocket,
];

#[derive(Debug)]
pub struct IconEntry {
    pub icon: IconName,
    pub slug: String,
    pub label: String,
    search: String,
}

#[derive(Debug)]
pub struct SearchResult {
    pub entries: Vec<&'static IconEntry>,
    pub total: usize,
    pub searched: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum MatchRank {
    Exact,
    Prefix,
    WordPrefix,
    Substring,
}

struct Catalog {
    all: Vec<IconEntry>,
    featured: Vec<usize>,
    by_slug: HashMap<String, usize>,
}

static CATALOG: OnceLock<Catalog> = OnceLock::new();

fn catalog() -> &'static Catalog {
    CATALOG.get_or_init(|| {
        let mut all = Vec::with_capacity(IconName::ALL.len());
        for &icon in IconName::ALL {
            let path = icon.path();
            let Some(slug) = slug_from_path(path.as_ref()) else {
                continue;
            };
            // Window chrome icons are for title bars, not queue icons.
            if slug.starts_with("window-") {
                continue;
            }
            let search = slug.replace('-', " ");
            let label = human_label(&search);
            all.push(IconEntry {
                icon,
                slug: slug.to_owned(),
                label,
                search,
            });
        }
        all.sort_by(|a, b| a.slug.cmp(&b.slug));
        let by_slug: HashMap<String, usize> = all
            .iter()
            .enumerate()
            .map(|(index, entry)| (entry.slug.clone(), index))
            .collect();
        let index_by_icon: HashMap<IconName, usize> = all
            .iter()
            .enumerate()
            .map(|(index, entry)| (entry.icon, index))
            .collect();
        let featured = FEATURED_QUEUE_ICONS
            .iter()
            .filter_map(|icon| index_by_icon.get(icon).copied())
            .collect();
        Catalog {
            all,
            featured,
            by_slug,
        }
    })
}

fn slug_from_path(path: &str) -> Option<&str> {
    path.strip_prefix("icons/")?.strip_suffix(".svg")
}

fn human_label(words: &str) -> String {
    let mut chars = words.chars();
    match chars.next() {
        Some(first) => format!("{}{}", first.to_ascii_uppercase(), chars.as_str()),
        None => String::new(),
    }
}

/// The pickable catalog entry for `slug`, or `None` when no pickable icon has that slug.
pub fn entry_by_slug(slug: &str) -> Option<&'static IconEntry> {
    let cat = catalog();
    cat.by_slug.get(slug).map(|&index| &cat.all[index])
}

/// The bundled icon named by `slug`, or `None` when no pickable icon has that slug.
pub fn by_slug(slug: &str) -> Option<IconName> {
    entry_by_slug(slug).map(|entry| entry.icon)
}

/// The icon a queue draws: its chosen bundled icon, or [`DEFAULT_QUEUE_ICON`].
pub fn queue_icon(slug: Option<&str>) -> IconName {
    slug.and_then(by_slug).unwrap_or(DEFAULT_QUEUE_ICON)
}

pub fn catalog_count() -> usize {
    catalog().all.len()
}

/// Searches the full catalog for icons matching every whitespace-separated term in `query`.
/// An empty query returns the curated icons, optionally pinning `pinned` first when it is outside
/// the curated set.
pub fn search(query: &str, pinned: Option<&str>, limit: usize) -> SearchResult {
    let cat = catalog();
    if query.trim().is_empty() {
        let mut entries: Vec<&'static IconEntry> =
            cat.featured.iter().map(|&index| &cat.all[index]).collect();
        if let Some(selected) = pinned.and_then(entry_by_slug)
            && !entries.iter().any(|entry| entry.icon == selected.icon)
        {
            entries.insert(0, selected);
        }
        let total = entries.len();
        entries.truncate(limit);
        return SearchResult {
            entries,
            total,
            searched: false,
        };
    }
    let cleaned = query.to_ascii_lowercase().replace(['-', '_'], " ");
    let terms: Vec<&str> = cleaned.split_whitespace().collect();
    if terms.is_empty() {
        return SearchResult {
            entries: Vec::new(),
            total: 0,
            searched: true,
        };
    }
    let normalized = terms.join(" ");
    let mut matched: Vec<(MatchRank, &'static IconEntry)> = cat
        .all
        .iter()
        .filter_map(|entry| {
            if !terms.iter().all(|term| entry.search.contains(term)) {
                return None;
            }
            let rank = if entry.search == normalized {
                MatchRank::Exact
            } else if entry.search.starts_with(&normalized) {
                MatchRank::Prefix
            } else if terms
                .iter()
                .all(|term| entry.search.split(' ').any(|word| word.starts_with(term)))
            {
                MatchRank::WordPrefix
            } else {
                MatchRank::Substring
            };
            Some((rank, entry))
        })
        .collect();
    matched.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.slug.cmp(&b.1.slug)));
    let total = matched.len();
    let entries = matched
        .into_iter()
        .take(limit)
        .map(|(_, entry)| entry)
        .collect();
    SearchResult {
        entries,
        total,
        searched: true,
    }
}

/// The picture of a download's kind of file.
pub fn category(category: Category) -> IconName {
    match category {
        Category::Video => IconName::Film,
        Category::Audio => IconName::Music,
        Category::Archive => IconName::FileArchive,
        Category::Document => IconName::FileText,
        Category::Program => IconName::Package,
        Category::Image => IconName::Image,
        Category::Other => IconName::File,
    }
}

#[cfg(test)]
mod tests {
    use gpui_kit::AssetSource as _;
    use wirecatch_protocol::app::is_valid_queue_icon;

    use super::*;

    /// `IconName::ArrowUpDown` is drawn from `icons/arrow-up-down.svg`.
    fn file_of(name: &str) -> String {
        let mut file = String::from("icons/");
        let chars: Vec<char> = name.chars().collect();
        for (at, c) in chars.iter().enumerate() {
            let starts_word = at > 0
                && (c.is_ascii_uppercase()
                    || (c.is_ascii_digit() && !chars[at - 1].is_ascii_digit()));
            if starts_word {
                file.push('-');
            }
            file.push(c.to_ascii_lowercase());
        }
        file + ".svg"
    }

    fn sources(folder: &std::path::Path, found: &mut Vec<(std::path::PathBuf, String)>) {
        for entry in std::fs::read_dir(folder).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                sources(&path, found);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                let text = std::fs::read_to_string(&path).unwrap();
                found.push((path, text));
            }
        }
    }

    // An icon missing from the assets draws nothing, with no error.
    #[test]
    fn every_icon_the_app_names_is_bundled() {
        let mut files = Vec::new();
        sources(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
            &mut files,
        );
        let mut missing = Vec::new();
        for (path, text) in &files {
            for prefix in ["IconName::", "Lucide::"] {
                for (at, _) in text.match_indices(prefix) {
                    let name: String = text[at + prefix.len()..]
                        .chars()
                        .take_while(char::is_ascii_alphanumeric)
                        .collect();
                    if name.starts_with(|c: char| c.is_ascii_uppercase())
                        && name != "ALL"
                        && Assets.load(&file_of(&name)).ok().flatten().is_none()
                    {
                        missing.push(format!("{name} in {}", path.display()));
                    }
                }
            }
        }
        assert!(missing.is_empty(), "icons not bundled: {missing:?}");
    }

    #[test]
    fn queue_icon_resolves_slugs_and_excludes_window_chrome() {
        assert_eq!(queue_icon(None), DEFAULT_QUEUE_ICON);
        assert_eq!(queue_icon(Some("film")), IconName::Film);
        assert_eq!(queue_icon(Some("does-not-exist")), DEFAULT_QUEUE_ICON);
        assert_eq!(by_slug("film"), Some(IconName::Film));
        assert_eq!(by_slug("window-close"), None);
        assert_eq!(queue_icon(Some("window-close")), DEFAULT_QUEUE_ICON);
    }

    #[test]
    fn every_pickable_catalog_slug_satisfies_protocol_rules() {
        assert!(catalog_count() > 1500);
        for entry in &catalog().all {
            assert!(
                is_valid_queue_icon(&entry.slug),
                "{} violates queue icon slug rules",
                entry.slug
            );
        }
    }

    #[test]
    fn search_ranks_queries_pins_selected_icon_and_handles_separators() {
        let initial = search("", None, 100);
        assert!(!initial.searched);
        assert_eq!(initial.total, FEATURED_QUEUE_ICONS.len());

        let featured_pinned = search("", Some("film"), 100);
        assert!(!featured_pinned.searched);
        assert_eq!(featured_pinned.total, FEATURED_QUEUE_ICONS.len());
        assert_eq!(featured_pinned.entries[0].slug, "list-ordered");

        let pinned = search("", Some("telescope"), 100);
        assert_eq!(pinned.entries[0].slug, "telescope");

        let ranked = search("lock", None, 200);
        assert!(ranked.searched);
        let pos = |slug: &str| {
            ranked
                .entries
                .iter()
                .position(|entry| entry.slug == slug)
                .unwrap_or_else(|| panic!("{slug} missing from lock results"))
        };
        // Exact ("lock") < Prefix ("lock-open") < WordPrefix ("folder-lock") < Substring ("clock").
        assert!(pos("lock") < pos("lock-open"));
        assert!(pos("lock-open") < pos("folder-lock"));
        assert!(pos("folder-lock") < pos("clock"));

        let spaced = search("hard   drive", None, 20);
        assert_eq!(spaced.entries[0].slug, "hard-drive");

        let dashed = search("hard--drive", None, 20);
        assert_eq!(dashed.entries[0].slug, "hard-drive");

        let punctuation_only = search("---", None, 20);
        assert!(punctuation_only.searched);
        assert_eq!(punctuation_only.total, 0);
        assert!(punctuation_only.entries.is_empty());

        let non_ascii = search("caf\u{00e9}", None, 20);
        assert!(non_ascii.searched);
        assert_eq!(non_ascii.total, 0);
        assert!(non_ascii.entries.is_empty());

        let missing = search("zzzznotanicon", None, 20);
        assert!(missing.searched);
        assert_eq!(missing.total, 0);
    }
}

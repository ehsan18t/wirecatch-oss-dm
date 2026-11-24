//! Icons from gpui-kit's complete Lucide catalogue, plus lookup and search for queue icons.

use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::OnceLock;

use gpui_kit::assets::{AllAssets, IconName};
use gpui_kit::{AssetSource, Result, SharedString};
use wirecatch_protocol::app::Category;

/// The icon a queue draws when the person has not picked one.
pub const DEFAULT_QUEUE_ICON: IconName = IconName::ListOrdered;

/// Curated queue icons shown before the person types a search query.
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

/// Every icon in gpui-kit's bundled Lucide and component catalogue.
pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        AllAssets.load(path)
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        AllAssets.list(path)
    }
}

/// One icon in the searchable queue icon catalogue.
#[derive(Debug)]
pub struct IconEntry {
    pub icon: IconName,
    pub slug: String,
    pub label: String,
    search: String,
}

struct Catalog {
    all: Vec<IconEntry>,
    featured: Vec<usize>,
    by_slug: HashMap<String, IconName>,
}

static CATALOG: OnceLock<Catalog> = OnceLock::new();

fn catalog() -> &'static Catalog {
    CATALOG.get_or_init(|| {
        let mut all = Vec::with_capacity(IconName::ALL.len());
        let mut by_slug = HashMap::with_capacity(IconName::ALL.len());
        for &icon in IconName::ALL {
            let path = icon.path();
            let Some(slug) = slug_from_path(path.as_ref()) else {
                continue;
            };
            by_slug.insert(slug.to_owned(), icon);
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

/// The bundled icon named by `slug`, or `None` when no bundled icon has that slug.
pub fn by_slug(slug: &str) -> Option<IconName> {
    catalog().by_slug.get(slug).copied()
}

/// The icon a queue draws: its chosen bundled icon, or [`DEFAULT_QUEUE_ICON`].
pub fn queue_icon(slug: Option<&str>) -> IconName {
    slug.and_then(by_slug).unwrap_or(DEFAULT_QUEUE_ICON)
}

/// Total number of pickable icons in the catalogue.
pub fn catalog_count() -> usize {
    catalog().all.len()
}

/// Curated icons for the picker's initial view before a query is typed.
pub fn featured() -> Vec<&'static IconEntry> {
    let cat = catalog();
    cat.featured.iter().map(|&index| &cat.all[index]).collect()
}

/// Searches the full catalogue for icons matching every whitespace-separated term in `query`.
/// Returns up to `limit` ranked matches and the total number of matches.
pub fn search(query: &str, limit: usize) -> (Vec<&'static IconEntry>, usize) {
    let cat = catalog();
    let normalized = query.trim().to_ascii_lowercase().replace(['-', '_'], " ");
    let terms: Vec<&str> = normalized.split_whitespace().collect();
    if terms.is_empty() {
        let items = featured();
        let total = items.len();
        return (items.into_iter().take(limit).collect(), total);
    }
    let mut matched: Vec<(u8, &'static IconEntry)> = cat
        .all
        .iter()
        .filter_map(|entry| {
            let all_hit = terms.iter().all(|term| entry.search.contains(term));
            if !all_hit {
                return None;
            }
            let rank = if entry.search == normalized {
                0
            } else if entry.search.starts_with(&normalized) {
                1
            } else if terms
                .iter()
                .all(|term| entry.search.split(' ').any(|word| word.starts_with(term)))
            {
                2
            } else {
                3
            };
            Some((rank, entry))
        })
        .collect();
    matched.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.slug.cmp(&b.1.slug)));
    let total = matched.len();
    let items = matched
        .into_iter()
        .take(limit)
        .map(|(_, entry)| entry)
        .collect();
    (items, total)
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
    fn queue_icon_resolves_slugs_and_falls_back_to_default() {
        assert_eq!(queue_icon(None), DEFAULT_QUEUE_ICON);
        assert_eq!(queue_icon(Some("film")), IconName::Film);
        assert_eq!(queue_icon(Some("does-not-exist")), DEFAULT_QUEUE_ICON);
        assert_eq!(by_slug("film"), Some(IconName::Film));
    }

    #[test]
    fn catalog_includes_all_featured_icons_and_searches_by_terms() {
        assert_eq!(featured().len(), FEATURED_QUEUE_ICONS.len());
        assert!(catalog_count() > 1500);
        let (hits, total) = search("film", 20);
        assert!(total >= 1);
        assert_eq!(hits[0].slug, "film");
        let (hits, total) = search("file-video", 20);
        assert!(total >= 1);
        assert!(hits.iter().any(|entry| entry.slug.contains("video")));
    }
}

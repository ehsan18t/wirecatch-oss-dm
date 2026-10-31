//! Icons come from gpui-kit's Lucide catalogue: its default set, plus the ones named here.

use std::borrow::Cow;

use gpui_kit::assets::IconName;
use gpui_kit::{AssetSource, Result, SharedString};
use wirecatch_protocol::app::Category;

gpui_kit::assets::icon_assets!(
    Extra,
    [
        ArrowRightLeft,
        ArrowUpDown,
        ArrowUp,
        Ban,
        CircleQuestionMark,
        CirclePause,
        Clock,
        Download,
        File,
        FileArchive,
        FileText,
        Film,
        Image,
        Layers,
        LayoutList,
        ListOrdered,
        Music,
        Package,
        Pencil,
        Trash,
        Zap,
        ZapOff,
    ]
);

/// The icons named above, then gpui-kit's default set.
pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        match Extra.load(path)? {
            Some(bytes) => Ok(Some(bytes)),
            None => gpui_kit::assets::Assets.load(path),
        }
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut listed = gpui_kit::assets::Assets.list(path)?;
        listed.extend(Extra.list(path)?);
        Ok(listed)
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
                        && Assets.load(&file_of(&name)).unwrap().is_none()
                    {
                        missing.push(format!("{name} in {}", path.display()));
                    }
                }
            }
        }
        assert!(missing.is_empty(), "icons not bundled: {missing:?}");
    }
}

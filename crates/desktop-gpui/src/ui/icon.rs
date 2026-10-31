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
        CirclePause,
        Clock,
        Download,
        File,
        FileArchive,
        FileText,
        Film,
        Image,
        Layers,
        ListOrdered,
        Music,
        Package,
        Pencil,
        Trash,
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

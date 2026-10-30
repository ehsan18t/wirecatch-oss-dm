//! The Lucide icons the app draws beyond gpui-kit's default set, served before that set
//! (`assets/icons`, Lucide's ISC licence beside them).

use std::borrow::Cow;

use gpui_kit::component::IconNamed;
use gpui_kit::{AssetSource, Result, SharedString};

macro_rules! app_icons {
    ($($name:ident => $file:literal,)*) => {
        /// An icon of the app's own set.
        #[derive(Clone, Copy)]
        pub enum AppIcon {
            $($name,)*
        }

        impl IconNamed for AppIcon {
            fn path(self) -> SharedString {
                match self {
                    $(Self::$name => concat!("icons/", $file).into(),)*
                }
            }
        }

        fn own(path: &str) -> Option<&'static [u8]> {
            match path {
                $(concat!("icons/", $file) => Some(include_bytes!(concat!("../assets/icons/", $file))),)*
                _ => None,
            }
        }

        const OWN: &[&str] = &[$(concat!("icons/", $file),)*];
    };
}

app_icons! {
    ArrowUp => "arrow-up.svg",
    Ban => "ban.svg",
    CirclePause => "circle-pause.svg",
    Clock => "clock.svg",
    Download => "download.svg",
    Layers => "layers.svg",
    LayoutList => "layout-list.svg",
}

/// The app's icons, then gpui-kit's.
pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        match own(path) {
            Some(bytes) => Ok(Some(Cow::Borrowed(bytes))),
            None => gpui_kit::assets::Assets.load(path),
        }
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut listed = gpui_kit::assets::Assets.list(path)?;
        listed.extend(
            OWN.iter()
                .filter(|own| own.starts_with(path))
                .map(|own| (*own).into()),
        );
        Ok(listed)
    }
}

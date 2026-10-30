//! Browser: what the extension catches and how a caught download is asked about. The
//! extension's popup changes these same settings.

use gpui_kit::component::setting::{
    NumberFieldOptions, SettingField, SettingGroup, SettingItem, SettingPage,
};
use gpui_kit::{App, Axis, Entity, SharedString};
use wirecatch_engine::numeric::{approx_f64, f64_to_u64};
use wirecatch_engine::units::MIB;
use wirecatch_protocol::app::AppSettingsPatch;
use wirecatch_protocol::extension::{ExtensionSettings, ExtensionSettingsPatch};

use super::{Typing, change, current, list_field, switch};
use crate::library::Library;

pub(super) fn page(library: &Entity<Library>, cx: &App) -> SettingPage {
    SettingPage::new("Browser").resettable(false).groups([
        capture(library),
        what_is_caught(library, cx),
        download_window(library),
    ])
}

fn extension(patch: ExtensionSettingsPatch) -> AppSettingsPatch {
    AppSettingsPatch {
        extension: Some(patch),
        ..AppSettingsPatch::default()
    }
}

fn capture(library: &Entity<Library>) -> SettingGroup {
    SettingGroup::new()
        .title("Capture")
        .item(
            SettingItem::new(
                "Capture downloads from the browser",
                switch(library, |s| s.extension.capture_enabled, |on| {
                    extension(ExtensionSettingsPatch {
                        capture_enabled: Some(on),
                        ..ExtensionSettingsPatch::default()
                    })
                }),
            )
            .description("Wirecatch takes over downloads you start in Chrome, Edge, Firefox and other browsers.")
            .keywords(["extension", "capture", "intercept", "chrome", "firefox", "edge"]),
        )
        .item(
            SettingItem::new(
                "Work in private windows",
                switch(library, |s| s.extension.private_windows, |on| {
                    extension(ExtensionSettingsPatch {
                        private_windows: Some(on),
                        ..ExtensionSettingsPatch::default()
                    })
                }),
            )
            .description("Also captures downloads and videos in private windows. The browser must also allow Wirecatch there, in its extension settings.")
            .keywords(["incognito", "private", "inprivate"]),
        )
        .item(
            SettingItem::new(
                "Show a download button on videos",
                switch(library, |s| s.extension.show_media_button, |on| {
                    extension(ExtensionSettingsPatch {
                        show_media_button: Some(on),
                        ..ExtensionSettingsPatch::default()
                    })
                }),
            )
            .description("Adds a small button over video and audio on web pages.")
            .keywords(["video", "media", "button", "overlay"]),
        )
}

/// Which downloads are caught, and the queue they join.
fn what_is_caught(library: &Entity<Library>, cx: &App) -> SettingGroup {
    let sites = extension_list(
        library,
        "ignored-sites",
        Typing::Lines,
        "example.com",
        |s| s.ignored_sites.clone(),
        |sites| ExtensionSettingsPatch {
            ignored_sites: Some(sites),
            ..ExtensionSettingsPatch::default()
        },
    );
    let types = extension_list(
        library,
        "capture-extensions",
        Typing::Extensions,
        "zip, mp4",
        |s| s.capture_extensions.clone(),
        |extensions| ExtensionSettingsPatch {
            capture_extensions: Some(extensions),
            ..ExtensionSettingsPatch::default()
        },
    );
    SettingGroup::new()
        .title("What is caught")
        .item(
            SettingItem::new("Sites to ignore", sites)
                .layout(Axis::Vertical)
                .description("Downloads from these sites are left to the browser. One per line.")
                .keywords(["ignore", "exclude", "block", "skip"]),
        )
        .item(
            SettingItem::new("Only capture these file types", types)
                .layout(Axis::Vertical)
                .description("Leave empty to capture all file types.")
                .keywords(["extensions", "types", "filter"]),
        )
        .item(
            SettingItem::new("Skip files smaller than, in MB", min_size(library))
                .description("Smaller files, like images on a page, stay in the browser. 0 captures every size.")
                .keywords(["size", "small", "minimum"]),
        )
        .item(
            SettingItem::new("Queue for browser downloads", default_queue(library, cx))
                .description("Downloads captured from the browser are added to this queue.")
                .keywords(["queue", "default"]),
        )
}

fn download_window(library: &Entity<Library>) -> SettingGroup {
    SettingGroup::new()
        .title("Download window")
        .item(
            SettingItem::new(
                "Show a window before each download",
                switch(library, |s| s.extension.ask_before_download, |on| {
                    extension(ExtensionSettingsPatch {
                        ask_before_download: Some(on),
                        ..ExtensionSettingsPatch::default()
                    })
                }),
            )
            .description("Rename the file, pick its folder and queue, or save it for later. Videos follow Settings, Media.")
            .keywords(["ask", "confirm", "prompt", "dialog"]),
        )
        .item(
            SettingItem::new(
                "Keep the download window open to show progress",
                switch(library, |s| s.show_progress_windows, |on| AppSettingsPatch {
                    show_progress_windows: Some(on),
                    ..AppSettingsPatch::default()
                }),
            )
            .description("After you click Download now, the window stays open and shows the download's progress.")
            .keywords(["progress", "window", "popup"]),
        )
}

/// A list setting of the extension.
fn extension_list(
    library: &Entity<Library>,
    key: &'static str,
    typing: Typing,
    placeholder: &'static str,
    read: fn(&ExtensionSettings) -> Vec<String>,
    patch: fn(Vec<String>) -> ExtensionSettingsPatch,
) -> SettingField<SharedString> {
    let library = library.clone();
    SettingField::render(move |_, window, cx| {
        list_field(
            key.into(),
            &current(&library, cx, |s| read(&s.extension), Vec::new()),
            typing,
            placeholder,
            move |values, cx| change(extension(patch(values)), cx),
            window,
            cx,
        )
    })
}

/// The smallest file caught, typed in MB to a tenth.
fn min_size(library: &Entity<Library>) -> SettingField<f64> {
    let reading = library.clone();
    SettingField::number_input(
        NumberFieldOptions {
            min: 0.,
            max: 100_000.,
            step: 0.5,
        },
        move |cx| {
            let bytes = current(&reading, cx, |s| s.extension.min_capture_size, 0);
            (approx_f64(bytes) / approx_f64(MIB) * 10.).round() / 10.
        },
        |typed, cx| {
            if let Some(bytes) = f64_to_u64((typed * approx_f64(MIB)).round()) {
                change(
                    extension(ExtensionSettingsPatch {
                        min_capture_size: Some(bytes),
                        ..ExtensionSettingsPatch::default()
                    }),
                    cx,
                );
            }
        },
    )
}

fn default_queue(library: &Entity<Library>, cx: &App) -> SettingField<SharedString> {
    let options = library
        .read(cx)
        .queues
        .iter()
        .map(|queue| {
            (
                SharedString::from(queue.id.0.to_string()),
                SharedString::from(queue.settings.name.clone()),
            )
        })
        .collect();
    let (reading, picking) = (library.clone(), library.clone());
    SettingField::dropdown(
        options,
        move |cx| {
            current(&reading, cx, |s| Some(s.extension.default_queue), None)
                .map(|id| id.0.to_string().into())
                .unwrap_or_default()
        },
        move |key, cx| {
            let picked = picking
                .read(cx)
                .queues
                .iter()
                .find(|queue| queue.id.0.to_string() == key.as_ref())
                .map(|queue| queue.id);
            if let Some(queue) = picked {
                change(
                    extension(ExtensionSettingsPatch {
                        default_queue: Some(queue),
                        ..ExtensionSettingsPatch::default()
                    }),
                    cx,
                );
            }
        },
    )
}

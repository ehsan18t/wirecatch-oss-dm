//! General (D85.2): the look, how the app starts and closes, what updates itself, and its logs.

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::progress::Progress;
use gpui_kit::component::setting::{SettingField, SettingGroup, SettingItem, SettingPage};
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{ActiveTheme as _, Icon, IconName, Sizable as _, WindowExt as _, h_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, ElementId, Entity, InteractiveElement as _, IntoElement, ParentElement as _,
    StatefulInteractiveElement as _, Styled as _, div, px,
};
use wirecatch_protocol::app::{ACCENTS, Accent, AppSettingsPatch, ClockFormat, Theme};
use wirecatch_shell::host;

use super::super::backup_dialog::{self, Direction};
use super::{change, choice, current, field_row, switch};
use crate::app_update::{NoUpdates, Stage};
use crate::library::Library;
use crate::theme;
use crate::ui::{Role, Typed as _, tokens};

const CLOCKS: &[(ClockFormat, &str)] = &[
    (ClockFormat::TwelveHour, "12-hour"),
    (ClockFormat::TwentyFourHour, "24-hour"),
];

const THEMES: &[(Theme, &str)] = &[
    (Theme::System, "Match the system"),
    (Theme::Light, "Light"),
    (Theme::Dark, "Dark"),
];

const SWATCH: f32 = 22.;

pub(super) fn page(library: &Entity<Library>) -> SettingPage {
    SettingPage::new(super::Page::General.title())
        .resettable(false)
        .groups([
            appearance(library),
            startup(library),
            updates(library),
            backup(library),
            troubleshooting(),
        ])
}

fn appearance(library: &Entity<Library>) -> SettingGroup {
    let swatches = {
        let library = library.clone();
        SettingField::render(move |_, _, cx| accent_swatches(&library, cx))
    };
    SettingGroup::new()
        .title("Appearance")
        .item(
            SettingItem::new(
                "Theme",
                choice(
                    library,
                    THEMES,
                    |s| s.theme,
                    |value| AppSettingsPatch {
                        theme: Some(value),
                        ..AppSettingsPatch::default()
                    },
                ),
            )
            .description("Follow the system, or always use light or dark.")
            .keywords(["dark mode", "light mode", "appearance", "colours"]),
        )
        .item(
            SettingItem::new("Accent colour", swatches)
                .description("Marks what is selected and what is downloading.")
                .keywords(["color", "highlight", "tint"]),
        )
        .item(
            SettingItem::new(
                "Time format",
                choice(
                    library,
                    CLOCKS,
                    |s| s.clock,
                    |value| AppSettingsPatch {
                        clock: Some(value),
                        ..AppSettingsPatch::default()
                    },
                ),
            )
            .description("How times show, such as 3:42:07 PM or 15:42:07.")
            .keywords(["clock", "12-hour", "24-hour", "am", "pm", "time"]),
        )
}

fn startup(library: &Entity<Library>) -> SettingGroup {
    SettingGroup::new()
        .title("Startup")
        .item(
            SettingItem::new(
                format!("Start {} when you sign in", app_name()),
                switch(
                    library,
                    |s| s.start_with_system,
                    |on| AppSettingsPatch {
                        start_with_system: Some(on),
                        ..AppSettingsPatch::default()
                    },
                ),
            )
            .description("Runs in the tray, so browser downloads are caught right away.")
            .keywords(["startup", "boot", "login", "autostart"]),
        )
        .item(
            SettingItem::new(
                "Keep downloading when the window is closed",
                switch(
                    library,
                    |s| s.close_to_tray,
                    |on| AppSettingsPatch {
                        close_to_tray: Some(on),
                        ..AppSettingsPatch::default()
                    },
                ),
            )
            .description(format!(
                "Closing the window leaves {} in the tray instead of quitting.",
                app_name()
            ))
            .keywords(["tray", "background", "minimize", "quit", "exit"]),
        )
}

fn updates(library: &Entity<Library>) -> SettingGroup {
    let no_updates = crate::app_update::no_updates();
    let (description, field) = match no_updates {
        None => (
            "Looks for a newer version once a day and asks before installing it.",
            SettingField::render(|_, _, cx| app_update_field(cx)),
        ),
        Some(NoUpdates::DevBuild) => (
            "Development builds don't update themselves.",
            SettingField::render(|_, _, _| div()),
        ),
        Some(NoUpdates::Unofficial) => (
            "This is an unofficial build, so it can't update itself. Get the official version from the website.",
            SettingField::render(|_, _, _| website_button()),
        ),
    };
    SettingGroup::new()
        .title("Updates")
        .item(
            SettingItem::new(app_name(), field)
                .description(description)
                .keywords(["update", "upgrade", "version", "new version"]),
        )
        .items(no_updates.is_none().then(|| {
                SettingItem::new(
                    "Get pre-release versions",
                    switch(library, |s| s.prerelease_updates, |on| AppSettingsPatch {
                        prerelease_updates: Some(on),
                        ..AppSettingsPatch::default()
                    }),
                )
                .description("Also offers versions still being tested, which come sooner and may have problems.")
                .keywords(["beta", "preview", "pre-release", "test"])
        }))
        .item(
            SettingItem::new(
                "Keep add-ons up to date",
                switch(library, |s| s.auto_update_addons, |on| AppSettingsPatch {
                    auto_update_addons: Some(on),
                    ..AppSettingsPatch::default()
                }),
            )
            .description("Installs new versions of the media add-on automatically.")
            .keywords(["addon", "ffmpeg", "yt-dlp", "media"]),
        )
        .item(
            SettingItem::new(
                "Update the Public Suffix List automatically",
                switch(library, |s| s.update_suffix_list, |on| AppSettingsPatch {
                    update_suffix_list: Some(on),
                    ..AppSettingsPatch::default()
                }),
            )
            .description(
                "Weekly, while nothing downloads. The list tells which sites share an owner; publicsuffix.org sees only your IP address and the app\u{2019}s version.",
            )
            .keywords(["psl", "domains", "privacy"]),
        )
}

fn backup(library: &Entity<Library>) -> SettingGroup {
    let button = |label: &'static str, direction: Direction| {
        let library = library.clone();
        SettingField::render(move |_, _, _| {
            let library = library.clone();
            Button::new(label)
                .small()
                .outline()
                .label(label)
                .on_click(move |_, window, cx| backup_dialog::open(&library, direction, window, cx))
        })
    };
    SettingGroup::new()
        .title("Backup")
        .item(
            SettingItem::new("Export settings", button("Export", Direction::Export))
                .description(
                    "Saves your settings, queues, schedules and folder rules to a single file.",
                )
                .keywords(["backup", "export", "save", "move", "profile"]),
        )
        .item(
            SettingItem::new("Import settings", button("Import", Direction::Import))
                .description("Shows what will change before replacing your settings and queues.")
                .keywords(["backup", "import", "restore", "load", "profile"]),
        )
}

fn troubleshooting() -> SettingGroup {
    SettingGroup::new().title("Troubleshooting").item(
        SettingItem::new(
            "Log folder",
            SettingField::render(|_, _, _| {
                Button::new("open-log-folder")
                    .small()
                    .outline()
                    .label("Open")
                    .on_click(|_, window, cx| match log_folder() {
                        Some(folder) => cx.open_with_system(&folder),
                        None => window.push_notification("There are no logs yet.", cx),
                    })
            }),
        )
        .description("For a support request. Logs leave out passwords and other secrets.")
        .keywords(["logs", "diagnostics", "troubleshooting", "support", "debug"]),
    )
}

/// Where the app's own update stands, with what can be done about it (D111).
fn app_update_field(cx: &App) -> AnyElement {
    let stage = crate::app_update::entity(cx).read(cx).stage.clone();
    let check = |label: &'static str| {
        Button::new("check-app-update")
            .small()
            .outline()
            .label(label)
            .on_click(|_, _, cx| crate::app_update::check(false, cx))
    };
    let status = |text: String, cx: &App| div().text_right().text_role(Role::Meta, cx).child(text);
    let row = field_row();
    match stage {
        Stage::Unchecked => row.child(check("Check for updates")),
        Stage::Checking => row
            .child(status("Checking.".into(), cx))
            .child(check("Check for updates").loading(true)),
        Stage::Current => row
            .child(status("You have the newest version.".into(), cx))
            .child(check("Check again")),
        Stage::Available(update) => {
            let page = update.page.clone();
            let installs = update.download.is_some();
            row.child(status(
                format!("Version {} is available.", update.version),
                cx,
            ))
            .child(
                Button::new("app-update-notes")
                    .small()
                    .ghost()
                    .label("What's new")
                    .on_click(move |_, _, cx| cx.open_url(&page)),
            )
            .child(
                Button::new("install-app-update")
                    .small()
                    .primary()
                    .label(if installs {
                        "Install and restart"
                    } else {
                        "Download"
                    })
                    .on_click(move |_, _, cx| crate::app_update::install(&update, cx)),
            )
        }
        Stage::Downloading { update, progress } => {
            // In tenths of a percent, as whole numbers.
            let done = progress
                .total()
                .filter(|total| *total > 0)
                .and_then(|total| u16::try_from(progress.done().min(total) * 1000 / total).ok())
                .map(|tenths| f32::from(tenths) / 10.);
            row.child(status(
                done.map_or_else(
                    || format!("Downloading version {}.", update.version),
                    |done| format!("Downloading version {}, {done:.0}%.", update.version),
                ),
                cx,
            ))
            .child(
                div().w(px(160.)).child(
                    Progress::new("app-update-progress")
                        .loading(done.is_none())
                        .value(done.unwrap_or_default()),
                ),
            )
        }
        Stage::Installing => row.child(status("Installing.".into(), cx)),
        Stage::Failed(message) => row
            .child(
                div()
                    .text_right()
                    .text_role(Role::Meta, cx)
                    .text_color(cx.theme().danger)
                    .child(message),
            )
            .child(check("Try again")),
    }
    .into_any_element()
}

/// The official website, for an unofficial build; nothing while the product has no website yet.
fn website_button() -> AnyElement {
    let url = wirecatch_product::WEBSITE_URL;
    if url.contains("TODO-PRODUCT-") {
        return div().into_any_element();
    }
    Button::new("open-website")
        .small()
        .outline()
        .label("Open website")
        .on_click(move |_, _, cx| cx.open_url(url))
        .into_any_element()
}

fn app_name() -> &'static str {
    host::CHANNEL.name()
}

fn log_folder() -> Option<std::path::PathBuf> {
    host::log_folder(std::env::var_os(host::DATA_DIR_VAR)).filter(|folder| folder.is_dir())
}

/// One round swatch per accent, the chosen one ticked. Teal and indigo, too close to others,
/// and the system's, not read yet, show only while chosen (D85.9).
fn accent_swatches(library: &Entity<Library>, cx: &App) -> impl IntoElement + use<> {
    let chosen = current(library, cx, |s| s.accent, Accent::default());
    let dark = cx.theme().mode.is_dark();
    let system = wirecatch_shell::appearance::system_accent().is_some();
    h_flex().gap_2().children(
        ACCENTS
            .iter()
            .copied()
            .filter(move |&accent| {
                accent == chosen
                    || match accent {
                        Accent::Teal | Accent::Indigo => false,
                        Accent::System => system,
                        _ => true,
                    }
            })
            .map(|accent| {
                let (tone, on_tone) = theme::accent_colors(accent, dark);
                let picked = accent == chosen;
                div()
                    .id(ElementId::Name(format!("accent-{accent:?}").into()))
                    .size(px(SWATCH))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_full()
                    .bg(tone)
                    .cursor_pointer()
                    .hover(|swatch| swatch.opacity(0.85))
                    .when(picked, |swatch| {
                        swatch.child(
                            Icon::new(IconName::Check)
                                .with_size(tokens::ICON_SMALL)
                                .text_color(on_tone),
                        )
                    })
                    .tooltip(move |window, cx| Tooltip::new(accent_name(accent)).build(window, cx))
                    .on_click(move |_, _, cx| {
                        change(
                            AppSettingsPatch {
                                accent: Some(accent),
                                ..AppSettingsPatch::default()
                            },
                            cx,
                        );
                    })
            }),
    )
}

fn accent_name(accent: Accent) -> &'static str {
    match accent {
        Accent::Cyan => "Mint",
        Accent::Purple => "Purple",
        Accent::Blue => "Blue",
        Accent::Green => "Green",
        Accent::Orange => "Orange",
        Accent::Pink => "Pink",
        Accent::Red => "Red",
        Accent::Teal => "Teal",
        Accent::Indigo => "Indigo",
        Accent::Amber => "Amber",
        Accent::System => "Match the system",
    }
}

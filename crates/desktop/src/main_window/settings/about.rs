//! About: the app, its version and the media add-on's, and where its licence, source and help
//! live, all from `config/product.toml`.

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::setting::{SettingField, SettingGroup, SettingItem, SettingPage};
use gpui_kit::component::{
    ActiveTheme as _, IconName, Sizable as _, WindowExt as _, h_flex, v_flex,
};
use gpui_kit::{
    App, ClipboardItem, Entity, ParentElement as _, SharedString, Styled as _, div, px,
};
use wirecatch_product::{
    ISSUES_URL, PRODUCT_LICENSE, PRODUCT_VERSION, PUBLISHER_NAME, REPOSITORY_URL, SUPPORT_EMAIL,
    WEBSITE_URL,
};
use wirecatch_protocol::app::{AddonId, AddonState};
use wirecatch_shell::host;

use crate::library::Library;
use crate::ui::{Role, Typed as _, mark};

const MARK_HEIGHT: f32 = 36.;

pub(super) fn page(library: &Entity<Library>) -> SettingPage {
    SettingPage::new(super::Page::About.title())
        .resettable(false)
        .groups([product(library), project(), help()])
}

/// The app with its version, and the media add-on's programs with theirs.
fn product(library: &Entity<Library>) -> SettingGroup {
    let library = library.clone();
    SettingGroup::new()
        .item(
            SettingItem::render(|_, _, cx| {
                h_flex()
                    .gap_4()
                    .py_2()
                    .child(mark::mark(px(MARK_HEIGHT), cx.theme().primary))
                    .child(
                        v_flex()
                            .flex_1()
                            .gap_1()
                            .child(
                                div()
                                    .text_role(Role::PageTitle, cx)
                                    .child(host::CHANNEL.name()),
                            )
                            .child(
                                div()
                                    .text_role(Role::Secondary, cx)
                                    .child(wirecatch_product::PRODUCT_SUMMARY),
                            ),
                    )
                    .child(
                        h_flex()
                            .gap_2()
                            .child(value(
                                format!("Version {PRODUCT_VERSION}, {}", system()),
                                cx,
                            ))
                            .child(
                                Button::new("copy-version")
                                    .small()
                                    .ghost()
                                    .icon(IconName::Copy)
                                    .tooltip("Copy the version, for a problem report")
                                    .on_click(|_, window, cx| {
                                        cx.write_to_clipboard(ClipboardItem::new_string(build()));
                                        window.push_notification("Version copied.", cx);
                                    }),
                            ),
                    )
            })
            .keywords(["version", "build", "release"]),
        )
        .item(
            SettingItem::new(
                "Media add-on",
                SettingField::render(move |_, _, cx| value(addon_versions(&library, cx), cx)),
            )
            .description("The programs it installs, with their versions.")
            .keywords(["yt-dlp", "ffmpeg", "deno", "addon", "version"]),
        )
}

fn project() -> SettingGroup {
    SettingGroup::new()
        .title("Project")
        .item(
            SettingItem::new(
                "Licence",
                SettingField::render(|_, _, cx| {
                    h_flex()
                        .gap_2()
                        .child(value(PRODUCT_LICENSE, cx))
                        .child(link(
                            "open-licence",
                            "Open",
                            format!("{REPOSITORY_URL}/blob/main/LICENSE"),
                        ))
                }),
            )
            .description("Free software: you may use, study, share and change it.")
            .keywords(["license", "gpl", "open source"]),
        )
        .item(
            SettingItem::new(
                "Publisher",
                SettingField::render(|_, _, cx| value(PUBLISHER_NAME, cx)),
            )
            .keywords(["maintainer", "author", "company"]),
        )
        .item(
            SettingItem::new(
                "Website",
                SettingField::render(|_, _, _| link("open-website", "Open", WEBSITE_URL)),
            )
            .description(WEBSITE_URL)
            .keywords(["homepage", "site"]),
        )
        .item(
            SettingItem::new(
                "Source code",
                SettingField::render(|_, _, _| link("open-repository", "Open", REPOSITORY_URL)),
            )
            .description(REPOSITORY_URL)
            .keywords(["github", "repository", "code"]),
        )
}

fn help() -> SettingGroup {
    SettingGroup::new()
        .title("Help")
        .item(
            SettingItem::new(
                "Report a problem",
                SettingField::render(|_, _, _| link("open-issues", "Open", ISSUES_URL)),
            )
            .description("Describe what happened, with the version above and your logs.")
            .keywords(["bug", "issue", "feedback", "support"]),
        )
        .item(
            SettingItem::new(
                "Email support",
                SettingField::render(|_, _, _| {
                    link("email-support", "Email", format!("mailto:{SUPPORT_EMAIL}"))
                }),
            )
            .description(SUPPORT_EMAIL)
            .keywords(["contact", "help", "email"]),
        )
}

fn value(text: impl Into<SharedString>, cx: &App) -> gpui_kit::Div {
    div().text_role(Role::Secondary, cx).child(text.into())
}

/// A button, `label`led, that hands `url` to the system.
fn link(id: &'static str, label: &'static str, url: impl Into<String>) -> Button {
    let url = url.into();
    Button::new(id)
        .small()
        .outline()
        .icon(IconName::ExternalLink)
        .label(label)
        .on_click(move |_, _, cx| cx.open_url(&url))
}

/// The app, its version and the system it runs on, as a problem report wants them.
fn build() -> String {
    format!("{} {PRODUCT_VERSION} ({})", host::CHANNEL.name(), system())
}

fn system() -> String {
    let os = match std::env::consts::OS {
        "windows" => "Windows",
        "linux" => "Linux",
        other => other,
    };
    let arch = match std::env::consts::ARCH {
        "x86_64" => "x64",
        "aarch64" => "ARM64",
        other => other,
    };
    format!("{os} {arch}")
}

fn addon_versions(library: &Entity<Library>, cx: &App) -> String {
    let addon = library
        .read(cx)
        .addons
        .iter()
        .find(|addon| addon.id == AddonId::Media)
        .cloned();
    match addon {
        Some(addon)
            if matches!(
                addon.state,
                AddonState::Installed | AddonState::UpdateAvailable
            ) =>
        {
            addon
                .installed_version
                .unwrap_or_else(|| "Installed".to_owned())
        }
        Some(_) => "Not installed".to_owned(),
        None => "Checking".to_owned(),
    }
}

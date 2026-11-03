//! An add-on's state and what can be done with it (D2, D20): one main button that installs,
//! updates, tries again or stops, and once installed, checking for updates and removal.

use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants as _};
use gpui_kit::component::progress::Progress;
use gpui_kit::component::setting::SettingItem;
use gpui_kit::component::{ActiveTheme as _, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, Entity, IntoElement, ParentElement as _, Styled as _, Window, div, px,
};
use wirecatch_protocol::app::{AddonId, AddonInfo, AddonState};
use wirecatch_shell::activity::format_bytes;
use wirecatch_shell::api;

use crate::backend::command;
use crate::library::Library;
use crate::ui::{Role, Typed as _};

const PER_MILLE: u64 = 1000;
/// Wide enough for "Check for updates" and "Install (999.9 MB)", so stacked buttons match.
const BUTTONS_WIDTH: f32 = 148.;
/// The settings card's own padding, which the strip at its foot reaches past.
const CARD_PADDING: f32 = 16.;

/// How far an install is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Installing {
    SizeUnknown,
    /// In tenths of a percent.
    Done(u16),
}

/// The add-on `id` as a settings card: what it does beside its buttons, then where it stands.
pub(super) fn item(library: &Entity<Library>, id: AddonId) -> SettingItem {
    let library = library.clone();
    SettingItem::render(move |_, _, cx| card(&library, id, cx)).keywords([
        "media", "addon", "add-on", "yt-dlp", "ffmpeg", "youtube", "install", "update",
    ])
}

fn card(library: &Entity<Library>, id: AddonId, cx: &App) -> AnyElement {
    let Some(addon) = library
        .read(cx)
        .addons
        .iter()
        .find(|addon| addon.id == id)
        .cloned()
    else {
        return div()
            .text_role(Role::Secondary, cx)
            .child("Checking.")
            .into_any_element();
    };
    v_flex()
        .w_full()
        .gap_3()
        .child(
            h_flex()
                .w_full()
                .gap_4()
                .child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .child(div().text_role(Role::Strong, cx).child(addon.name.clone()))
                        .child(
                            div()
                                .text_role(Role::Secondary, cx)
                                .child(addon.description.clone()),
                        ),
                )
                .child(
                    v_flex()
                        .flex_shrink_0()
                        .w(px(BUTTONS_WIDTH))
                        .gap_1p5()
                        .children(buttons(&addon, cx)),
                ),
        )
        .child(standing(&addon, cx))
        .into_any_element()
}

/// The strip along the card's foot, edge to edge: where the add-on stands, and how far an install
/// is.
fn standing(addon: &AddonInfo, cx: &App) -> impl IntoElement {
    let theme = cx.theme();
    let failed = matches!(addon.state, AddonState::Failed { .. });
    v_flex()
        .mx(px(-CARD_PADDING))
        .mb(px(-CARD_PADDING))
        .px(px(CARD_PADDING))
        .py_2()
        .gap_1()
        .border_t_1()
        .border_color(theme.border)
        .child(
            h_flex()
                .gap_3()
                .child(
                    div()
                        .text_role(Role::Meta, cx)
                        .when(failed, |text| text.text_color(theme.danger))
                        .child(status(addon)),
                )
                .when_some(installing(&addon.state), |line, progress| {
                    let done = match progress {
                        Installing::Done(done) => Some(done),
                        Installing::SizeUnknown => None,
                    };
                    line.child(
                        div().flex_1().child(
                            Progress::new("addon-progress")
                                .loading(done.is_none())
                                .value(done.map_or(0., |done| f32::from(done) / 10.)),
                        ),
                    )
                }),
        )
        .when_some(addon.note.clone(), |strip, note| {
            strip.child(div().text_role(Role::Meta, cx).child(note))
        })
}

/// The add-on's buttons, its main one first and Remove last.
fn buttons(addon: &AddonInfo, cx: &App) -> Vec<Button> {
    let id = addon.id;
    let mut buttons: Vec<Button> = main_button(addon).into_iter().collect();
    if addon.state == AddonState::Installed {
        buttons.push(
            Button::new("check-addon-updates")
                .small()
                .outline()
                .label("Check for updates")
                .on_click(|_, window, cx| {
                    command(window.window_handle(), cx, |manager| async move {
                        api::check_addon_updates(&manager).await.map(|_| ())
                    });
                }),
        );
    }
    if matches!(
        addon.state,
        AddonState::Installed | AddonState::UpdateAvailable
    ) {
        let name = addon.name.to_lowercase();
        buttons.push(
            Button::new("remove-addon")
                .small()
                .outline()
                .text_color(cx.theme().danger)
                .label("Remove")
                .on_click(move |_, window, cx| ask_to_remove(id, &name, window, cx)),
        );
    }
    buttons
}

/// Install, Update, Try again or Stop, by the add-on's state.
fn main_button(addon: &AddonInfo) -> Option<Button> {
    let id = addon.id;
    let button = Button::new("addon-main").small();
    match &addon.state {
        AddonState::Installing { .. } => Some(button.outline().label("Stop").on_click(
            move |_, window, cx| {
                // Stopping never removes a working add-on: an update keeps the installed version.
                command(window.window_handle(), cx, move |manager| async move {
                    api::cancel_addon_operation(&manager, id).await
                });
            },
        )),
        AddonState::Installed => None,
        AddonState::UpdateAvailable => Some(button.primary().label("Update").on_click(
            move |_, window, cx| {
                command(window.window_handle(), cx, move |manager| async move {
                    api::update_addon(&manager, id).await
                });
            },
        )),
        AddonState::NotInstalled | AddonState::Failed { .. } => {
            let label = match (&addon.state, addon.size) {
                (AddonState::Failed { .. }, _) => "Try again".to_owned(),
                (_, Some(size)) => format!("Install ({})", format_bytes(size)),
                (_, None) => "Install".to_owned(),
            };
            Some(
                button
                    .primary()
                    .label(label)
                    .on_click(move |_, window, cx| {
                        command(window.window_handle(), cx, move |manager| async move {
                            api::install_addon(&manager, id).await
                        });
                    }),
            )
        }
    }
}

/// How far an install is; `None` when it is not installing.
fn installing(state: &AddonState) -> Option<Installing> {
    let AddonState::Installing { done, total } = state else {
        return None;
    };
    Some(
        total
            .filter(|total| *total > 0)
            .and_then(|total| u16::try_from((*done).min(total) * PER_MILLE / total).ok())
            .map_or(Installing::SizeUnknown, Installing::Done),
    )
}

/// One line on where the add-on stands.
fn status(addon: &AddonInfo) -> String {
    match &addon.state {
        AddonState::NotInstalled => "Not installed".to_owned(),
        AddonState::Installing { .. } => match installing(&addon.state) {
            Some(Installing::Done(done)) if u64::from(done) >= PER_MILLE => {
                // Everything is downloaded; the files are being checked and unpacked.
                "Installing: verifying files".to_owned()
            }
            Some(Installing::Done(done)) => format!("Installing, {}%", done / 10),
            Some(Installing::SizeUnknown) | None => "Installing".to_owned(),
        },
        AddonState::Installed => addon.installed_version.as_ref().map_or_else(
            || "Installed".to_owned(),
            |version| format!("Installed: {version}"),
        ),
        AddonState::UpdateAvailable => addon.latest_version.as_ref().map_or_else(
            || "A newer version is ready to install.".to_owned(),
            |version| format!("Version {version} is ready to install."),
        ),
        AddonState::Failed { message } => message.clone(),
    }
}

/// Asks before removing the add-on: what stops working until it is installed again.
fn ask_to_remove(id: AddonId, name: &str, window: &mut Window, cx: &mut App) {
    let title = format!("Remove the {name}?");
    crate::ui::dialog::open_alert(window, cx, move |alert, _, _| {
        alert
            .title(title.clone())
            .description(
                "Downloads from video sites, and videos whose video and audio must be merged, won’t work until you install it again. Your downloaded files are kept.",
            )
            .confirm()
            .ok_text("Remove")
            .ok_variant(ButtonVariant::Danger)
            .cancel_text("Keep")
            .on_ok(move |_, window, cx| {
                command(window.window_handle(), cx, move |manager| async move {
                    api::remove_addon(&manager, id).await
                });
                true
            })
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_install_reads_as_how_far_it_is() {
        let at = |done, total| installing(&AddonState::Installing { done, total });
        assert_eq!(at(250, Some(1000)), Some(Installing::Done(250)));
        assert_eq!(at(9, None), Some(Installing::SizeUnknown));
        assert_eq!(at(2000, Some(1000)), Some(Installing::Done(1000)));
        assert_eq!(installing(&AddonState::Installed), None);
    }
}

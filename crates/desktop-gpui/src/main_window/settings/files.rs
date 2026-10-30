//! Saving files: the download folder, what a download does when its file is already there
//! (D88), and a folder for each type of file.

use gpui_kit::component::button::Button;
use gpui_kit::component::setting::{SettingField, SettingGroup, SettingItem, SettingPage};
use gpui_kit::component::{ActiveTheme as _, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, Axis, ElementId, Entity, IntoElement as _, ParentElement as _, SharedString,
    Styled as _, Window, div, px,
};
use wirecatch_protocol::app::{
    AppSettingsPatch, Category, CategoryRule, PortablePath, WhenFileExists,
};

use super::{Typing, change, choice, current, list_field};
use crate::library::Library;
use crate::ui::{Role, Typed as _};
use crate::{files, text};

const WHEN_FILE_EXISTS: &[(WhenFileExists, &str)] = &[
    (WhenFileExists::Ask, "Ask me"),
    (WhenFileExists::Replace, "Replace the file"),
    (WhenFileExists::KeepExisting, "Keep the existing file"),
    (WhenFileExists::KeepBoth, "Keep both"),
];
/// The types with folders of their own; the rest go to the download folder.
const TYPES: [Category; 6] = [
    Category::Video,
    Category::Audio,
    Category::Archive,
    Category::Document,
    Category::Program,
    Category::Image,
];
const FOLDER_WIDTH: f32 = 220.;

pub(super) fn page(library: &Entity<Library>) -> SettingPage {
    SettingPage::new("Saving files")
        .resettable(false)
        .groups([where_files_go(library)])
}

fn where_files_go(library: &Entity<Library>) -> SettingGroup {
    let folder = {
        let library = library.clone();
        SettingField::render(move |_, _, cx| download_folder(&library, cx))
    };
    let types = {
        let library = library.clone();
        SettingField::render(move |_, window, cx| folders_by_type(&library, window, cx))
    };
    SettingGroup::new()
        .title("Where files go")
        .item(
            SettingItem::new("Download folder", folder)
                .description("Used when no file type rule below matches.")
                .keywords(["location", "directory", "path", "save", "destination"]),
        )
        .item(
            SettingItem::new(
                "If the file is already in the folder",
                choice(
                    library,
                    WHEN_FILE_EXISTS,
                    |s| s.when_file_exists,
                    |when| AppSettingsPatch {
                        when_file_exists: Some(when),
                        ..AppSettingsPatch::default()
                    },
                ),
            )
            .description("A file counts as already there when it has the same name and size, and came from the same link. A file that only shares the name is always saved under a new name.")
            .keywords(["duplicate", "exists", "overwrite", "replace", "skip", "rename", "keep both", "ask"]),
        )
        .item(
            SettingItem::new("Folders by file type", types)
                .layout(Axis::Vertical)
                .description("Files are saved to these folders based on their extension.")
                .keywords(["categories", "video", "music", "archive", "documents", "programs", "images", "extensions"]),
        )
}

fn download_folder(library: &Entity<Library>, cx: &App) -> AnyElement {
    let folder = current(
        library,
        cx,
        |s| text::folder(&s.default_folder.0),
        String::new(),
    );
    h_flex()
        .gap_2()
        .child(
            div()
                .max_w(px(FOLDER_WIDTH * 1.5))
                .truncate()
                .text_role(Role::Secondary, cx)
                .child(folder),
        )
        .child(
            Button::new("change-download-folder")
                .small()
                .outline()
                .label("Change")
                .on_click(|_, _, cx| {
                    let picked = files::choose_folder(cx);
                    cx.spawn(async move |cx| {
                        if let Some(folder) = picked.await {
                            cx.update(|cx| {
                                change(
                                    AppSettingsPatch {
                                        default_folder: Some(PortablePath(folder)),
                                        ..AppSettingsPatch::default()
                                    },
                                    cx,
                                );
                            });
                        }
                    })
                    .detach();
                }),
        )
        .into_any_element()
}

/// Each type of file with its extensions and its folder.
fn folders_by_type(library: &Entity<Library>, window: &mut Window, cx: &mut App) -> AnyElement {
    let rules = current(library, cx, |s| s.categories.clone(), Vec::new());
    let (border, radius) = (cx.theme().border, cx.theme().radius);
    let rows: Vec<AnyElement> = TYPES
        .iter()
        .enumerate()
        .map(|(index, &category)| {
            let rule = rules.iter().find(|rule| rule.category == category);
            let extensions = rule.map(|rule| rule.extensions.clone()).unwrap_or_default();
            let folder = rule.map_or_else(
                || "Download folder".to_owned(),
                |rule| text::folder(&rule.folder.0),
            );
            let (typing, picking) = (library.clone(), library.clone());
            h_flex()
                .px_3()
                .py_2()
                .gap_3()
                .when(index > 0, |row| row.border_t_1().border_color(border))
                .child(
                    div()
                        .w(px(132.))
                        .flex_shrink_0()
                        .text_role(Role::Strong, cx)
                        .child(text::category_label(category)),
                )
                .child(div().flex_1().min_w_0().child(list_field(
                    SharedString::from(format!("extensions-{index}")),
                    &extensions,
                    Typing::Extensions,
                    "Add extensions",
                    move |extensions, cx| {
                        change_rule(&typing, category, cx, |rule| rule.extensions = extensions);
                    },
                    window,
                    cx,
                )))
                .child(
                    div()
                        .w(px(FOLDER_WIDTH))
                        .truncate()
                        .text_role(Role::Meta, cx)
                        .child(folder),
                )
                .child(
                    Button::new(ElementId::NamedInteger("type-folder".into(), index as u64))
                        .small()
                        .outline()
                        .label("Change")
                        .on_click(move |_, _, cx| {
                            let picked = files::choose_folder(cx);
                            let library = picking.clone();
                            cx.spawn(async move |cx| {
                                if let Some(folder) = picked.await {
                                    cx.update(|cx| {
                                        change_rule(&library, category, cx, |rule| {
                                            rule.folder = PortablePath(folder);
                                        });
                                    });
                                }
                            })
                            .detach();
                        }),
                )
                .into_any_element()
        })
        .collect();
    v_flex()
        .w_full()
        .rounded(radius)
        .border_1()
        .border_color(border)
        .children(rows)
        .into_any_element()
}

/// Sends the type rules with `edit` made to `category`'s, which starts in the download folder
/// when it has none yet.
fn change_rule(
    library: &Entity<Library>,
    category: Category,
    cx: &mut App,
    edit: impl FnOnce(&mut CategoryRule),
) {
    let Some(settings) = library.read(cx).settings.as_ref() else {
        return;
    };
    let mut rules = settings.categories.clone();
    let at = if let Some(at) = rules.iter().position(|rule| rule.category == category) {
        at
    } else {
        rules.push(CategoryRule {
            category,
            extensions: Vec::new(),
            folder: settings.default_folder.clone(),
        });
        rules.len() - 1
    };
    edit(&mut rules[at]);
    change(
        AppSettingsPatch {
            categories: Some(rules),
            ..AppSettingsPatch::default()
        },
        cx,
    );
}

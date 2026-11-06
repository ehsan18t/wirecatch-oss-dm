//! Saving the settings to a file, or loading them from one: what goes in the file, then for an
//! import what it would change before anything is applied, then what was done.

use std::future::Future;

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Icon, IconName, Sizable as _, WindowExt as _, h_flex,
    v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, AppContext as _, Context, Div, Entity, IntoElement, ParentElement as _,
    PathPromptOptions, Render, Styled as _, Window, div, px,
};
use wirecatch_product::PRODUCT_SLUG;
use wirecatch_protocol::app::{ImportPreview, ImportedFolder};
use wirecatch_protocol::wire::ProtocolError;
use wirecatch_shell::api;

use wirecatch_core::Manager;

use crate::backend::{Backend, said};
use crate::files;
use crate::library::Library;
use crate::ui::{Callout, Role, Typed as _};

const WIDTH: f32 = 600.;
const INCLUDED: [&str; 7] = [
    "All settings, including the browser extension settings",
    "Queues with their schedules and speed limits",
    "Proxy servers",
    "Folders by file type",
    "Sites to ignore",
    "Local network sites",
    "Which add-ons are installed (an import never installs any)",
];
const EXCLUDED: [&str; 3] = [
    "Download history",
    "Saved site passwords",
    "Proxy passwords",
];

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Direction {
    Export,
    Import,
}

enum Step {
    /// What the file holds, before a file is chosen.
    Start,
    /// What importing the chosen file would change.
    Preview {
        path: String,
        preview: Box<ImportPreview>,
    },
    /// The file written or read.
    Done { path: String },
}

pub(super) struct BackupForm {
    library: Entity<Library>,
    direction: Direction,
    step: Step,
    busy: bool,
    problem: Option<String>,
}

pub(super) fn open(
    library: &Entity<Library>,
    direction: Direction,
    window: &mut Window,
    cx: &mut App,
) {
    let form = cx.new(|_| BackupForm {
        library: library.clone(),
        direction,
        step: Step::Start,
        busy: false,
        problem: None,
    });
    let title = match direction {
        Direction::Export => "Export settings",
        Direction::Import => "Import settings",
    };
    crate::ui::dialog::open(
        window,
        cx,
        move |_| title.into(),
        move |dialog, _, cx| {
            dialog
                .w(px(WIDTH))
                .child(form.clone())
                .footer(footer(&form, cx))
        },
    );
}

fn footer(form: &Entity<BackupForm>, cx: &App) -> Div {
    let state = form.read(cx);
    let row = h_flex().w_full().gap_2().justify_end();
    let go = form.clone();
    if matches!(state.step, Step::Done { .. }) {
        return row.child(
            Button::new("backup-done")
                .primary()
                .label("Done")
                .on_click(|_, window, cx| window.close_dialog(cx)),
        );
    }
    let label = match (&state.step, state.direction) {
        (Step::Preview { .. }, _) => "Replace my settings",
        (_, Direction::Export) => "Choose where to save",
        (_, Direction::Import) => "Choose a settings file",
    };
    row.child(
        Button::new("backup-cancel")
            .label("Cancel")
            .on_click(|_, window, cx| window.close_dialog(cx)),
    )
    .child(
        Button::new("backup-go")
            .primary()
            .label(label)
            .loading(state.busy)
            .disabled(state.busy)
            .on_click(move |_, window, cx| go.update(cx, |form, cx| form.next(window, cx))),
    )
}

impl BackupForm {
    fn next(&mut self, window: &mut Window, cx: &mut Context<'_, Self>) {
        if self.busy {
            return;
        }
        self.problem = None;
        match (&self.step, self.direction) {
            (Step::Start, Direction::Export) => self.export(window, cx),
            (Step::Start, Direction::Import) => self.preview(window, cx),
            (Step::Preview { path, .. }, _) => {
                let path = path.clone();
                self.import(path, window, cx);
            }
            (Step::Done { .. }, _) => {}
        }
    }

    fn export(&mut self, window: &mut Window, cx: &mut Context<'_, Self>) {
        let folder = dirs::download_dir()
            .or_else(dirs::home_dir)
            .unwrap_or_default();
        let Some(turn) = files::chooser_turn(cx) else {
            return;
        };
        let picked =
            cx.prompt_for_new_path(&folder, Some(&format!("{PRODUCT_SLUG}-settings.json")));
        self.run(window, cx, move |manager| async move {
            let answer = picked.await;
            drop(turn);
            let Some(path) = files::chooser_answer(answer)? else {
                return Ok(None);
            };
            let path = api::path_text(&path)?;
            api::export_settings(&manager, &path).await?;
            Ok(Some(Step::Done { path }))
        });
    }

    fn preview(&mut self, window: &mut Window, cx: &mut Context<'_, Self>) {
        let Some(turn) = files::chooser_turn(cx) else {
            return;
        };
        let picked = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Import".into()),
        });
        self.run(window, cx, move |manager| async move {
            let answer = picked.await;
            drop(turn);
            let Some(paths) = files::chooser_answer(answer)? else {
                return Ok(None);
            };
            let Some(path) = paths.first() else {
                return Ok(None);
            };
            let path = api::path_text(path)?;
            let preview = api::preview_import(&manager, &path).await?;
            Ok(Some(Step::Preview {
                path,
                preview: Box::new(preview),
            }))
        });
    }

    fn import(&mut self, path: String, window: &mut Window, cx: &mut Context<'_, Self>) {
        self.run(window, cx, move |manager| async move {
            api::import_settings(&manager, &path).await?;
            Ok(Some(Step::Done { path }))
        });
    }

    /// Runs `work` on the manager; the step it ends at shows, or why it failed. `None` leaves
    /// the step as it is, as when no file was chosen.
    fn run<Fut>(
        &mut self,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
        work: impl FnOnce(Manager) -> Fut + Send + 'static,
    ) where
        Fut: Future<Output = Result<Option<Step>, ProtocolError>> + Send + 'static,
    {
        self.busy = true;
        cx.notify();
        let done = Backend::run(cx, work);
        cx.spawn_in(window, async move |form, cx| {
            let Some(outcome) = said(done.await) else {
                return;
            };
            crate::closing::may_have_closed(form.update(cx, |form, cx| {
                form.busy = false;
                match outcome {
                    Ok(Some(step)) => form.step = step,
                    Ok(None) => {}
                    Err(problem) => form.problem = Some(problem),
                }
                cx.notify();
            }));
        })
        .detach();
    }
}

impl Render for BackupForm {
    fn render(&mut self, _: &mut Window, cx: &mut Context<'_, Self>) -> impl IntoElement {
        let body = match &self.step {
            Step::Start => self.start(cx),
            Step::Preview { preview, .. } => self.preview_body(preview, cx),
            Step::Done { path } => done(self.direction, path, cx),
        };
        v_flex()
            .gap_4()
            .pb_1()
            .text_sm()
            .when_some(self.problem.clone(), |form, problem| {
                form.child(Callout::danger(problem))
            })
            .child(body)
    }
}

impl BackupForm {
    fn start(&self, cx: &App) -> AnyElement {
        let theme = cx.theme();
        let intro = match self.direction {
            Direction::Export => {
                "Saves your setup to one file, to load on another Windows or Linux computer. Folders like Downloads are matched to that computer."
            }
            Direction::Import => {
                "Loads a file made with Export settings. You see what it changes before anything is replaced; your downloads stay as they are."
            }
        };
        let list = |title: &'static str, items: &[&'static str], included: bool| {
            v_flex()
                .flex_1()
                .gap_1p5()
                .child(div().text_role(Role::Label, cx).child(title))
                .children(items.iter().map(|item| {
                    h_flex()
                        .gap_2()
                        .items_start()
                        .child(
                            Icon::new(if included {
                                IconName::Check
                            } else {
                                IconName::Minus
                            })
                            .small()
                            .text_color(if included {
                                theme.primary
                            } else {
                                theme.muted_foreground
                            }),
                        )
                        .child(
                            div()
                                .text_role(
                                    if included {
                                        Role::Body
                                    } else {
                                        Role::Secondary
                                    },
                                    cx,
                                )
                                .child(*item),
                        )
                }))
        };
        v_flex()
            .gap_4()
            .child(div().text_role(Role::Secondary, cx).child(intro))
            .child(
                v_flex()
                    .gap_4()
                    .child(list(
                        match self.direction {
                            Direction::Export => "In the file",
                            Direction::Import => "Replaced from the file",
                        },
                        &INCLUDED,
                        true,
                    ))
                    .child(list("Not included", &EXCLUDED, false)),
            )
            .into_any_element()
    }

    fn preview_body(&self, preview: &ImportPreview, cx: &App) -> AnyElement {
        let library = self.library.read(cx);
        let addons: Vec<String> = preview
            .addons_to_offer
            .iter()
            .filter_map(|id| library.addons.iter().find(|addon| addon.id == *id))
            .map(|addon| {
                format!(
                    "The file lists the {}, which is not installed here. Install it afterwards from Settings > Media; nothing is downloaded on import.",
                    addon.name.to_lowercase()
                )
            })
            .collect();
        v_flex()
            .gap_4()
            .child(Callout::info(
                "Nothing has changed yet. Importing this file makes the changes below.",
            ))
            .child(
                v_flex()
                    .gap_1p5()
                    .child(div().text_role(Role::Label, cx).child("Folders"))
                    .child(folders(&preview.folders, cx)),
            )
            .children(
                changes(preview)
                    .into_iter()
                    .chain(addons)
                    .map(|line| div().text_role(Role::Secondary, cx).child(line)),
            )
            .into_any_element()
    }
}

/// One line for each kind of change an import makes besides its folders.
fn changes(preview: &ImportPreview) -> Vec<String> {
    let said = |text: &str, names: &[String]| {
        (!names.is_empty()).then(|| format!("{text}: {}.", names.join(", ")))
    };
    let line = |parts: Vec<Option<String>>| {
        let parts: Vec<String> = parts.into_iter().flatten().collect();
        (!parts.is_empty()).then(|| parts.join(" "))
    };
    let tested = (!preview.proxies_added.is_empty() || !preview.proxies_updated.is_empty())
        .then(|| "They are tested after the import.".to_owned());
    [
        line(vec![
            said("Adds queues", &preview.queues_added),
            said("Changes queues", &preview.queues_updated),
        ]),
        line(vec![
            said("Adds proxy servers", &preview.proxies_added),
            said("Changes proxy servers", &preview.proxies_updated),
            tested,
        ]),
        said(
            "Removes the saved password from these proxy servers",
            &preview.proxy_passwords_removed,
        ),
        said(
            "Lets these sites reach your local network",
            &preview.local_sites_added,
        ),
        said(
            "Stops these sites from reaching your local network",
            &preview.local_sites_removed,
        ),
    ]
    .into_iter()
    .flatten()
    .collect()
}

/// The folders an import sets, each with why it is not the one the file asked for, or that it
/// needs a look.
fn folders(folders: &[ImportedFolder], cx: &App) -> Div {
    let theme = cx.theme();
    v_flex()
        .rounded(theme.radius)
        .border_1()
        .border_color(theme.border)
        .children(folders.iter().enumerate().map(|(index, folder)| {
            let flagged = folder.reason.is_some() || folder.outside_user_folders;
            let note = match (&folder.reason, folder.outside_user_folders) {
                (Some(reason), _) => Some(format!(
                    "{reason} Downloads go to the folder above instead."
                )),
                (None, true) => Some(
                    "This folder is outside your own folders. Check that you want downloads there."
                        .to_owned(),
                ),
                (None, false) => None,
            };
            v_flex()
                .px_3()
                .py_2()
                .gap_0p5()
                .when(index > 0, |row| row.border_t_1().border_color(theme.border))
                .child(
                    h_flex()
                        .gap_1()
                        .child(
                            div()
                                .text_role(Role::Secondary, cx)
                                .child(format!("{}:", folder.setting)),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_role(Role::Strong, cx)
                                .when(flagged, |text| text.text_color(theme.danger))
                                .child(folder.resolved.clone()),
                        ),
                )
                .when_some(note, |row, note| {
                    row.child(div().text_role(Role::Meta, cx).child(note))
                })
        }))
}

fn done(direction: Direction, path: &str, cx: &App) -> AnyElement {
    h_flex()
        .gap_2()
        .items_start()
        .child(Icon::new(IconName::CircleCheck).text_color(cx.theme().success))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .text_role(Role::Body, cx)
                .child(format!(
                    "{} {path}",
                    match direction {
                        Direction::Export => "Settings saved to",
                        Direction::Import => "Settings loaded from",
                    }
                )),
        )
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn preview() -> ImportPreview {
        ImportPreview {
            folders: Vec::new(),
            queues_added: Vec::new(),
            queues_updated: Vec::new(),
            proxies_added: Vec::new(),
            proxies_updated: Vec::new(),
            proxy_passwords_removed: Vec::new(),
            local_sites_added: Vec::new(),
            local_sites_removed: Vec::new(),
            addons_to_offer: Vec::new(),
        }
    }

    #[test]
    fn an_import_that_changes_nothing_else_says_nothing() {
        assert_eq!(changes(&preview()), Vec::<String>::new());
    }

    #[test]
    fn every_change_of_an_import_is_said() {
        let names = |name: &str| vec![name.to_owned()];
        let lines = changes(&ImportPreview {
            queues_added: names("Night"),
            queues_updated: names("Main"),
            proxies_added: names("Home"),
            proxies_updated: names("Office"),
            proxy_passwords_removed: names("Office"),
            local_sites_added: names("nas.example.com"),
            local_sites_removed: names("old.example.com"),
            ..preview()
        });
        assert_eq!(
            lines,
            [
                "Adds queues: Night. Changes queues: Main.",
                "Adds proxy servers: Home. Changes proxy servers: Office. They are tested after the import.",
                "Removes the saved password from these proxy servers: Office.",
                "Lets these sites reach your local network: nas.example.com.",
                "Stops these sites from reaching your local network: old.example.com.",
            ]
        );
    }
}

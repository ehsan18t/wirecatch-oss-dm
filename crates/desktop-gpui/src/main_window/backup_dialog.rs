//! Saving the settings to a file, or loading them from one: what goes in the file, then for an
//! import what it would change before anything is applied, then what was done.

use std::future::Future;
use std::path::PathBuf;

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

use crate::backend::Backend;
use crate::library::Library;
use crate::ui::{Callout, Role, Typed as _};

const WIDTH: f32 = 600.;
const INCLUDED: [&str; 5] = [
    "All settings, including the browser extension settings",
    "Queues with their schedules and speed limits",
    "Folders by file type",
    "Sites to ignore",
    "Which add-ons are installed (on import, you are asked before any is installed)",
];
const EXCLUDED: [&str; 2] = ["Download history", "Saved site passwords"];

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
        preview: ImportPreview,
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
    crate::ui::dialog::open(window, cx, move |dialog, _, cx| {
        dialog
            .title(match direction {
                Direction::Export => "Export settings",
                Direction::Import => "Import settings",
            })
            .w(px(WIDTH))
            .child(form.clone())
            .footer(footer(&form, cx))
    });
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
        let picked =
            cx.prompt_for_new_path(&folder, Some(&format!("{PRODUCT_SLUG}-settings.json")));
        self.run(window, cx, move |manager| async move {
            let Ok(Ok(Some(path))) = picked.await else {
                return Ok(None);
            };
            let path = shown(&path);
            api::export_settings(&manager, &path).await?;
            Ok(Some(Step::Done { path }))
        });
    }

    fn preview(&mut self, window: &mut Window, cx: &mut Context<'_, Self>) {
        let picked = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Import".into()),
        });
        self.run(window, cx, move |manager| async move {
            let Ok(Ok(Some(paths))) = picked.await else {
                return Ok(None);
            };
            let Some(path) = paths.first().map(|path| shown(path)) else {
                return Ok(None);
            };
            let preview = api::preview_import(&manager, &path).await?;
            Ok(Some(Step::Preview { path, preview }))
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
            let outcome = match done.await {
                Ok(Ok(result)) => result.map_err(|error| error.message),
                Ok(Err(unopened)) => Err(unopened.reason.message),
                Err(_) => return,
            };
            _ = form.update(cx, |form, cx| {
                form.busy = false;
                match outcome {
                    Ok(Some(step)) => form.step = step,
                    Ok(None) => {}
                    Err(problem) => form.problem = Some(problem),
                }
                cx.notify();
            });
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
        let names = |list: &[String]| list.join(", ");
        let queues = [
            (!preview.queues_added.is_empty())
                .then(|| format!("Adds queues: {}.", names(&preview.queues_added))),
            (!preview.queues_updated.is_empty())
                .then(|| format!("Changes queues: {}.", names(&preview.queues_updated))),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" ");
        let addons: Vec<String> = preview
            .addons_to_offer
            .iter()
            .filter_map(|id| library.addons.iter().find(|addon| addon.id == *id))
            .map(|addon| {
                format!(
                    "The file lists the {}, which is not installed here. Install it afterwards from Settings, Media; nothing is downloaded on import.",
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
            .when(!queues.is_empty(), |body| {
                body.child(div().text_role(Role::Secondary, cx).child(queues))
            })
            .children(
                addons
                    .into_iter()
                    .map(|line| div().text_role(Role::Secondary, cx).child(line)),
            )
            .into_any_element()
    }
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

fn shown(path: &std::path::Path) -> String {
    PathBuf::from(path).to_string_lossy().into_owned()
}

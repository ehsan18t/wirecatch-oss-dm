//! Find any action (Ctrl+Shift+P): every command the window offers, searched by name, and the
//! settings pages. The selected downloads' commands come first while some are selected.

use std::rc::Rc;

use gpui_kit::component::WindowExt;
use gpui_kit::component::command::{Command, CommandGroup, CommandItem, CommandState};
use gpui_kit::{App, AppContext as _, Context, ParentElement as _, WeakEntity, Window, px};
use wirecatch_protocol::DownloadId;
use wirecatch_protocol::app::DownloadStatus;
use wirecatch_shell::api;

use super::applies::{self, Applicable};
use super::{Group, MainWindow, Screen, View, confirm, details, keymap, settings};
use crate::backend::act;
use crate::files;

const WIDTH: f32 = 560.;

/// What a command does, run on the main window it was found in.
type Run = Rc<dyn Fn(&mut MainWindow, &mut Window, &mut Context<'_, MainWindow>)>;

struct Entry {
    title: String,
    keywords: &'static str,
    run: Run,
}

fn entry(
    title: impl Into<String>,
    keywords: &'static str,
    run: impl Fn(&mut MainWindow, &mut Window, &mut Context<'_, MainWindow>) + 'static,
) -> Entry {
    Entry {
        title: title.into(),
        keywords,
        run: Rc::new(run),
    }
}

impl MainWindow {
    /// Opens the palette over the window.
    pub(super) fn open_palette(&mut self, window: &mut Window, cx: &mut Context<'_, Self>) {
        let groups = self.palette_groups(cx);
        let main = cx.entity().downgrade();
        let state = cx.new(|cx| CommandState::new(window, cx));
        let groups = Rc::new(groups);
        let focused = state.clone();
        crate::ui::dialog::open(window, cx, move |dialog, _, _| {
            let (main, chosen) = (main.clone(), groups.clone());
            let command =
                groups.iter().fold(
                    Command::new(&state).placeholder("Find an action or a setting"),
                    |command, (heading, entries)| {
                        command.group(CommandGroup::new().label(*heading).items(
                            entries.iter().map(|e| {
                                CommandItem::new()
                                    .label(e.title.clone())
                                    .keywords(e.keywords.split(' ').filter(|word| !word.is_empty()))
                            }),
                        ))
                    },
                );
            dialog.w(px(WIDTH)).child(
                command
                    .max_h(px(420.))
                    .on_confirm(move |at, window, cx| {
                        let Some((_, entries)) = chosen.get(at.section) else {
                            return;
                        };
                        let Some(run) = entries.get(at.row).map(|e| e.run.clone()) else {
                            return;
                        };
                        window.close_dialog(cx);
                        run_on(&main, &run, window, cx);
                    })
                    .on_cancel(WindowExt::close_dialog),
            )
        });
        focused.update(cx, |state, cx| state.focus(window, cx));
    }

    /// The commands, by group, in the order the palette lists them.
    fn palette_groups(&self, cx: &App) -> Vec<(&'static str, Vec<Entry>)> {
        let mut groups = Vec::new();
        let selected = self.selected_commands(cx);
        if !selected.is_empty() {
            groups.push(("Selected downloads", selected));
        }
        groups.push(("General", general()));
        groups.push(("Go to", self.views(cx)));
        groups.push(("Settings", settings_pages()));
        groups
    }

    /// What the selected downloads can do.
    fn selected_commands(&self, cx: &App) -> Vec<Entry> {
        let rows = self.selected_rows(cx);
        if rows.is_empty() {
            return Vec::new();
        }
        let refs: Vec<_> = rows.iter().collect();
        let can = Applicable::to(&refs);
        let ids: Vec<DownloadId> = rows.iter().map(|row| row.id).collect();
        let named = match rows.as_slice() {
            [only] => format!("\u{201c}{}\u{201d}", only.file_name),
            _ => format!("{} downloads", rows.len()),
        };
        let mut entries = Vec::new();
        let each = |title: String, takes: fn(&_) -> bool, command: super::selection::Command| {
            let rows = rows.clone();
            entry(title, "", move |_, window, cx| {
                let rows: Vec<_> = rows.iter().collect();
                super::selection::each(&rows, takes, command, window, cx);
            })
        };
        if can.pause {
            entries.push(each(
                format!("Pause {named}"),
                applies::can_pause,
                api::pause,
            ));
        }
        if can.resume {
            entries.push(each(
                format!("Resume {named}"),
                applies::can_resume,
                api::resume,
            ));
        }
        if can.retry {
            entries.push(each(
                format!("Try {named} again"),
                applies::can_retry,
                api::retry,
            ));
        }
        if let [only] = rows.as_slice() {
            entries.extend(about_one(only, &named));
        }
        entries.extend(self.moves(&refs, &ids, &named, cx));
        let removed = ids.clone();
        entries.push(entry(
            format!("Remove {named} from the list"),
            "delete clear",
            move |main, w, cx| confirm::remove(&main.library, &removed, w, cx),
        ));
        entries.push(entry("Clear the selection", "deselect", |main, _, cx| {
            main.clear_selection();
            cx.notify();
        }));
        entries
    }

    /// Moving the selection to each queue some of it is not in.
    fn moves(
        &self,
        refs: &[&wirecatch_protocol::app::DownloadSummary],
        ids: &[DownloadId],
        named: &str,
        cx: &App,
    ) -> Vec<Entry> {
        let mut entries = Vec::new();
        let queues = &self.library.read(cx).queues;
        if queues.len() > 1 {
            for queue in queues.iter().filter(|q| applies::can_move_to(refs, q.id)) {
                let (queue_id, ids) = (queue.id, ids.to_vec());
                entries.push(entry(
                    format!("Move {named} to {}", queue.settings.name),
                    "queue transfer",
                    move |_, w, cx| {
                        let ids = ids.clone();
                        act(w.window_handle(), cx, move |m| {
                            api::move_to_queue(m, &ids, queue_id)
                        });
                    },
                ));
            }
        }
        entries
    }

    /// Every view of the list, and the queues.
    fn views(&self, cx: &App) -> Vec<Entry> {
        let show = |title: String, view: View| {
            entry(title, "view show list", move |main, window, cx| {
                main.screen = Screen::Downloads;
                main.show(view, window, cx);
            })
        };
        let mut entries = vec![show("Show all downloads".to_owned(), View::All)];
        entries.extend(
            Group::ALL
                .into_iter()
                .map(|group| show(format!("Show {}", group.label()), View::Group(group))),
        );
        entries.extend(self.library.read(cx).queues.iter().map(|queue| {
            show(
                format!("Show the {} queue", queue.settings.name),
                View::Queue(queue.id),
            )
        }));
        entries.push(entry("New queue", "add create queue", |main, w, cx| {
            main.edit_queue(None, w, cx);
        }));
        entries
    }
}

/// What one selected download can do besides what any selection can.
fn about_one(only: &wirecatch_protocol::app::DownloadSummary, named: &str) -> Vec<Entry> {
    let mut entries = Vec::new();
    let id = only.id;
    if only.status == DownloadStatus::Completed {
        entries.push(entry(
            format!("Open {named}"),
            "launch run file",
            move |_, w, cx| {
                files::open(id, w, cx);
            },
        ));
    }
    entries.push(entry(
        format!("Show {named} in its folder"),
        "folder explorer",
        move |_, w, cx| files::show_in_folder(id, w, cx),
    ));
    entries.push(entry(
        format!("Show the details of {named}"),
        "info properties",
        move |main, w, cx| details::open(main.library.clone(), id, w, cx),
    ));
    entries
}

/// Commands that work anywhere in the window.
fn general() -> Vec<Entry> {
    vec![
        entry("New download", "add link url", |main, w, cx| {
            main.new_download(&[], w, cx);
        }),
        entry(
            "Download the link you copied",
            "paste clipboard",
            |main, w, cx| {
                main.paste_links(w, cx);
            },
        ),
        entry("Search the list", "find filter", |main, w, cx| {
            main.screen = Screen::Downloads;
            main.search.update(cx, |search, cx| search.focus(w, cx));
        }),
        entry("Pause every download", "stop all", |_, w, cx| {
            act(w.window_handle(), cx, api::pause_all);
        }),
        entry("Resume every download", "start all continue", |_, w, cx| {
            act(w.window_handle(), cx, api::resume_all);
        }),
        entry("Keyboard shortcuts", "keys help", |_, w, cx| {
            keymap::open(w, cx);
        }),
    ]
}

/// Each settings page, opened on its own.
fn settings_pages() -> Vec<Entry> {
    settings::PAGES
        .into_iter()
        .map(|page| {
            entry(
                format!("Open {} settings", page.title()),
                "settings preferences options",
                move |main, _, cx| main.open_settings(page, cx),
            )
        })
        .collect()
}

/// Runs `run` on the main window `main`.
fn run_on(main: &WeakEntity<MainWindow>, run: &Run, window: &mut Window, cx: &mut App) {
    let run = run.clone();
    _ = main.update(cx, |main, cx| run(main, window, cx));
}

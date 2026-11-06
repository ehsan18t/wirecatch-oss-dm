//! New queue (D255): a name and, if the person likes, the sites or file types it takes. Everything
//! else is on the queue's own page, which More settings opens with what is typed here.

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::{ActiveTheme as _, Disableable as _, WindowExt as _, h_flex, v_flex};
use gpui_kit::{
    App, AppContext as _, Context, Div, Entity, IntoElement, ParentElement as _, Render,
    Styled as _, Subscription, WeakEntity, Window, div, px,
};
use wirecatch_protocol::app::QueueSettings;
use wirecatch_protocol::rules::{AddDownloads, QueueRules};
use wirecatch_shell::api;

use super::MainWindow;
use super::queue_editor::Opening;
use super::queue_rules::{Kind, values_field};
use crate::backend::{Backend, said};
use crate::closing::may_have_closed;
use crate::ui::tag_field::TagField;
use crate::ui::{Field, Role, Typed as _};

const WIDTH: f32 = 440.;
/// The kinds of rule offered here; the rest need choices the queue's page has room for.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Takes {
    Sites,
    FileTypes,
}

impl Takes {
    const ALL: [Self; 2] = [Self::Sites, Self::FileTypes];

    const fn label(self) -> &'static str {
        match self {
            Self::Sites => Kind::Sites.label(),
            Self::FileTypes => Kind::FileTypes.label(),
        }
    }
}

struct NewQueue {
    main: WeakEntity<MainWindow>,
    name: Entity<InputState>,
    kind: Takes,
    sites: Entity<TagField>,
    file_types: Entity<TagField>,
    problem: Option<String>,
    saving: bool,
    _typing: Subscription,
}

impl NewQueue {
    /// The queue as typed, or why it cannot be made.
    fn settings(&self, cx: &App) -> Result<QueueSettings, String> {
        let settings = self.typed(cx);
        if settings.name.is_empty() {
            return Err("Give the queue a name.".to_owned());
        }
        Ok(settings)
    }

    /// The queue as typed, its name maybe empty.
    fn typed(&self, cx: &App) -> QueueSettings {
        let mut settings = QueueSettings::new(self.name.read(cx).value().trim());
        let values = self.field().read(cx).values(cx);
        if !values.is_empty() {
            let mut rules = QueueRules {
                enabled: true,
                ..QueueRules::default()
            };
            match self.kind {
                Takes::Sites => rules.sites = values,
                Takes::FileTypes => rules.file_types = values,
            }
            settings.add_downloads = AddDownloads::Auto(rules);
        }
        settings
    }

    fn field(&self) -> &Entity<TagField> {
        match self.kind {
            Takes::Sites => &self.sites,
            Takes::FileTypes => &self.file_types,
        }
    }

    fn create(&mut self, window: &mut Window, cx: &mut Context<'_, Self>) {
        if self.saving {
            return;
        }
        let settings = match self.settings(cx) {
            Ok(settings) => settings,
            Err(problem) => {
                self.problem = Some(problem);
                cx.notify();
                return;
            }
        };
        self.saving = true;
        self.problem = None;
        cx.notify();
        let created = Backend::run(cx, move |manager| async move {
            api::create_queue(&manager, settings).map(|queue| queue.id)
        });
        let main = self.main.clone();
        cx.spawn_in(window, async move |form, cx| {
            let Some(outcome) = said(created.await) else {
                return;
            };
            may_have_closed(form.update_in(cx, |form, window, cx| {
                form.saving = false;
                match outcome {
                    Ok(id) => {
                        window.close_dialog(cx);
                        may_have_closed(
                            main.update(cx, |main, cx| main.leave_queue_page(id, window, cx)),
                        );
                    }
                    Err(problem) => {
                        form.problem = Some(problem);
                        cx.notify();
                    }
                }
            }));
        })
        .detach();
    }

    /// The queue's own page, holding what is typed here.
    fn more_settings(&self, window: &mut Window, cx: &mut App) {
        let settings = Box::new(self.typed(cx));
        window.close_dialog(cx);
        may_have_closed(self.main.update(cx, |main, cx| {
            main.open_queue_page(Opening::New(settings), window, cx);
        }));
    }
}

impl Render for NewQueue {
    fn render(&mut self, _: &mut Window, cx: &mut Context<'_, Self>) -> impl IntoElement {
        let me = cx.entity().downgrade();
        let kind = self.kind;
        let picker = Button::new("new-queue-kind")
            .outline()
            .label(kind.label())
            .dropdown_caret(true)
            .dropdown_menu(move |menu, _, _| {
                Takes::ALL.into_iter().fold(menu, |menu, offered| {
                    let me = me.clone();
                    menu.item(
                        PopupMenuItem::new(offered.label())
                            .checked(offered == kind)
                            .on_click(move |_, window, cx| {
                                may_have_closed(me.update(cx, |form, cx| {
                                    form.kind = offered;
                                    TagField::focus(form.field(), window, cx);
                                    cx.notify();
                                }));
                            }),
                    )
                })
            });
        v_flex()
            .gap_4()
            .text_sm()
            .child(Field::new("Name", Input::new(&self.name)))
            .child(
                Field::new(
                    "Add downloads automatically",
                    h_flex()
                        .gap_2()
                        .items_start()
                        .child(picker)
                        .child(div().flex_1().min_w_0().child(self.field().clone())),
                )
                .note("Optional. More kinds of rule are in More settings."),
            )
    }
}

/// Opens New queue.
pub(super) fn open(main: WeakEntity<MainWindow>, window: &mut Window, cx: &mut App) {
    let name = crate::ui::dialog::text_field("", "For example: Videos", window, cx);
    let sites = values_field(Kind::Sites, &[], window, cx);
    let file_types = values_field(Kind::FileTypes, &[], window, cx);
    let (Some(sites), Some(file_types)) = (sites, file_types) else {
        return;
    };
    let typed = name.clone();
    let form = cx.new(|cx| {
        let typing = cx.subscribe(&typed, |form: &mut NewQueue, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) && form.problem.take().is_some() {
                cx.notify();
            }
        });
        NewQueue {
            main,
            name: typed,
            kind: Takes::Sites,
            sites,
            file_types,
            problem: None,
            saving: false,
            _typing: typing,
        }
    });
    crate::ui::dialog::focus_after_opening(&name, window, cx);
    crate::ui::dialog::open(window, cx, move |dialog, _, cx| {
        let creating = form.clone();
        dialog
            .title("New queue")
            .w(px(WIDTH))
            .child(form.clone())
            .on_ok(move |_, window, cx| {
                creating.update(cx, |form, cx| form.create(window, cx));
                false
            })
            .footer(footer(&form, cx))
    });
}

/// Cancel, More settings and Create queue, under what stops the queue being made.
fn footer(form: &Entity<NewQueue>, cx: &App) -> Div {
    let state = form.read(cx);
    let (more, create) = (form.clone(), form.clone());
    v_flex()
        .w_full()
        .gap_2()
        .children(state.problem.as_ref().map(|problem| {
            div()
                .text_role(Role::Secondary, cx)
                .text_color(cx.theme().danger)
                .child(problem.clone())
        }))
        .child(
            h_flex()
                .w_full()
                .gap_2()
                .justify_end()
                .child(
                    Button::new("cancel")
                        .label("Cancel")
                        .on_click(|_, window, cx| window.close_dialog(cx)),
                )
                .child(
                    Button::new("more-settings")
                        .outline()
                        .label("More settings")
                        .on_click(move |_, window, cx| {
                            more.update(cx, |form, cx| form.more_settings(window, cx));
                        }),
                )
                .child(
                    Button::new("create-queue")
                        .primary()
                        .label("Create queue")
                        .loading(state.saving)
                        .disabled(state.saving)
                        .on_click(move |_, window, cx| {
                            create.update(cx, |form, cx| form.create(window, cx));
                        }),
                ),
        )
}

//! A queue's rules (D72.2): switched on, new downloads that match any rule join the queue. Each
//! rule is a row, what it looks at and then its values; a row removed keeps its values until the
//! queue is saved (D84.5, D255). Test a link asks the core what the rules as typed do (D72.7).

use std::rc::Rc;

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::InputState;
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::{ActiveTheme as _, Icon, IconName, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, AppContext as _, Context, Div, Entity, InteractiveElement as _, IntoElement,
    ParentElement as _, Render, StatefulInteractiveElement as _, Styled as _, Subscription, Window,
    div, px,
};
use wirecatch_protocol::QueueId;
use wirecatch_protocol::app::Category;
use wirecatch_protocol::rules::{AddDownloads, DraftRules, LinkProtocol, NameSyntax, QueueRules};

use super::link_trial::{Draft, Drafted, LinkTrial};
use crate::library::Library;
use crate::text;
use crate::ui::switch_row::panel;
use crate::ui::tag_field::{self, Split, TagField};
use crate::ui::{Role, SwitchRow, Typed as _, choice, tooltip};

const HELP: &str = "Rules are checked in this order: site, file name, file type, category, protocol. If several queues match a site, the most specific site wins; otherwise the queue highest in the sidebar wins.";
const TIES: &str = "When several queues match, the most specific site wins, then the queue highest in the sidebar.";
/// The width of a rule's first column, which says what the rule looks at.
const KIND_WIDTH: f32 = 168.;
const CATEGORIES: [Category; 7] = [
    Category::Video,
    Category::Audio,
    Category::Archive,
    Category::Document,
    Category::Program,
    Category::Image,
    Category::Other,
];
const PROTOCOLS: [(LinkProtocol, &str); 3] = [
    (LinkProtocol::Http, "HTTP"),
    (LinkProtocol::Https, "HTTPS"),
    (LinkProtocol::Ftp, "FTP and FTPS"),
];

/// What a rule looks at. A queue has at most one rule of each kind.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Kind {
    Sites,
    FileNames,
    FileTypes,
    Categories,
    Protocols,
}

impl Kind {
    pub(super) const ALL: [Self; 5] = [
        Self::Sites,
        Self::FileNames,
        Self::FileTypes,
        Self::Categories,
        Self::Protocols,
    ];

    pub(super) const fn label(self) -> &'static str {
        match self {
            Self::Sites => "Site is",
            Self::FileNames => "File name matches",
            Self::FileTypes => "File type is",
            Self::Categories => "Category is",
            Self::Protocols => "Protocol is",
        }
    }
}

/// A site list as typed: split at commas and spaces, as a site never holds one.
pub(super) fn typed_sites(typed: &str) -> Vec<String> {
    typed
        .split([',', ';', ' ', '\n'])
        .map(str::trim)
        .filter(|site| !site.is_empty())
        .map(str::to_owned)
        .collect()
}

const SITES: (Split, &str) = (Split::Words(typed_sites), "Add a site, such as youtube.com");
const FILE_NAMES: (Split, &str) = (Split::Lines, "Add a name, such as *.iso");
const FILE_TYPES: (Split, &str) = (
    Split::Words(text::typed_extensions),
    "Add a type, such as mp4",
);

/// The field a kind of rule types its values in; `None` for the kinds picked from a list.
pub(super) fn values_field(
    kind: Kind,
    values: &[String],
    window: &mut Window,
    cx: &mut App,
) -> Option<Entity<TagField>> {
    let typing = match kind {
        Kind::Sites => SITES,
        Kind::FileNames => FILE_NAMES,
        Kind::FileTypes => FILE_TYPES,
        Kind::Categories | Kind::Protocols => return None,
    };
    Some(tags(typing, values, window, cx))
}

fn tags(
    (split, placeholder): (Split, &'static str),
    values: &[String],
    window: &mut Window,
    cx: &mut App,
) -> Entity<TagField> {
    cx.new(|cx| TagField::new(values, split, placeholder, window, cx))
}

pub struct RulesForm {
    /// The queue edited; `None` for one not made yet.
    queue: Option<QueueId>,
    /// The queue's name as its form has it.
    name: Entity<InputState>,
    on: bool,
    /// The rules shown, in their order on screen.
    rows: Vec<Kind>,
    sites: Entity<TagField>,
    file_names: Entity<TagField>,
    syntax: NameSyntax,
    file_types: Entity<TagField>,
    categories: Vec<Category>,
    protocols: Vec<LinkProtocol>,
    trial: Entity<LinkTrial>,
    _subscriptions: Vec<Subscription>,
}

impl RulesForm {
    pub fn new(
        library: &Entity<Library>,
        queue: Option<QueueId>,
        name: Entity<InputState>,
        saved: &AddDownloads,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) -> Self {
        let (on, rules) = match saved {
            AddDownloads::Auto(rules) => (rules.enabled, rules.clone()),
            AddDownloads::Manual => (false, QueueRules::default()),
        };
        let sites = tags(SITES, &rules.sites, window, cx);
        let file_names = tags(FILE_NAMES, &rules.file_names, window, cx);
        let file_types = tags(FILE_TYPES, &rules.file_types, window, cx);
        let me = cx.entity().downgrade();
        let drafted: Drafted = Rc::new(move |cx| me.upgrade().map(|form| form.read(cx).draft(cx)));
        let trial = cx.new(|cx| LinkTrial::new(library.clone(), Some(drafted), window, cx));
        let mut rows: Vec<Kind> = [
            (Kind::Sites, !rules.sites.is_empty()),
            (Kind::FileNames, !rules.file_names.is_empty()),
            (Kind::FileTypes, !rules.file_types.is_empty()),
            (Kind::Categories, !rules.categories.is_empty()),
            (Kind::Protocols, !rules.protocols.is_empty()),
        ]
        .into_iter()
        .filter_map(|(kind, used)| used.then_some(kind))
        .collect();
        if rows.is_empty() {
            rows.push(Kind::Sites);
        }
        let subscriptions = [&sites, &file_names, &file_types]
            .map(|field| {
                cx.subscribe(field, |form: &mut Self, _, _: &tag_field::Changed, cx| {
                    form.forget_answer(cx);
                })
            })
            .into();
        Self {
            queue,
            name,
            on,
            rows,
            sites,
            file_names,
            syntax: rules.file_name_syntax,
            file_types,
            categories: rules.categories,
            protocols: rules.protocols,
            trial,
            _subscriptions: subscriptions,
        }
    }

    /// What the queue saves: its rules when switched on, kept switched off when it has any, or
    /// none (D72.4).
    pub fn add_downloads(&self, cx: &App) -> AddDownloads {
        let rules = self.rules(cx);
        if self.on || has_rules(&rules) {
            AddDownloads::Auto(rules)
        } else {
            AddDownloads::Manual
        }
    }

    /// Whether new downloads that match join the queue.
    pub fn is_on(&self) -> bool {
        self.on
    }

    fn rules(&self, cx: &App) -> QueueRules {
        let shown = |kind: Kind| self.rows.contains(&kind);
        let values = |kind: Kind, field: &Entity<TagField>| {
            if shown(kind) {
                field.read(cx).values(cx)
            } else {
                Vec::new()
            }
        };
        QueueRules {
            enabled: self.on,
            sites: values(Kind::Sites, &self.sites),
            file_names: values(Kind::FileNames, &self.file_names),
            file_name_syntax: self.syntax,
            file_types: values(Kind::FileTypes, &self.file_types),
            categories: if shown(Kind::Categories) {
                self.categories.clone()
            } else {
                Vec::new()
            },
            protocols: if shown(Kind::Protocols) {
                self.protocols.clone()
            } else {
                Vec::new()
            },
        }
    }

    /// An answer is for the link and the rules it was given: a change to either takes it away.
    fn forget_answer(&mut self, cx: &mut Context<'_, Self>) {
        self.trial.update(cx, LinkTrial::forget);
        cx.notify();
    }

    /// The rules as typed, to try a link against in place of the saved ones.
    fn draft(&self, cx: &App) -> Draft {
        Draft {
            rules: DraftRules {
                queue: self.queue,
                add_downloads: self.add_downloads(cx),
            },
            name: self.name.read(cx).value().trim().to_owned(),
        }
    }

    /// Shows the first kind of rule not shown yet, ready to type in.
    fn add_row(&mut self, window: &mut Window, cx: &mut Context<'_, Self>) {
        if let Some(kind) = Kind::ALL.into_iter().find(|kind| !self.rows.contains(kind)) {
            self.rows.push(kind);
            self.focus(kind, window, cx);
            self.forget_answer(cx);
        }
    }

    /// Row `index` looks at `kind` instead; a kind another row shows is not offered.
    fn set_kind(
        &mut self,
        index: usize,
        kind: Kind,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        if self.rows.contains(&kind) {
            return;
        }
        if let Some(row) = self.rows.get_mut(index) {
            *row = kind;
            self.focus(kind, window, cx);
            self.forget_answer(cx);
        }
    }

    fn remove_row(&mut self, index: usize, cx: &mut Context<'_, Self>) {
        if index < self.rows.len() {
            self.rows.remove(index);
            self.forget_answer(cx);
        }
    }

    fn focus(&self, kind: Kind, window: &mut Window, cx: &mut App) {
        let field = match kind {
            Kind::Sites => &self.sites,
            Kind::FileNames => &self.file_names,
            Kind::FileTypes => &self.file_types,
            Kind::Categories | Kind::Protocols => return,
        };
        TagField::focus(field, window, cx);
    }
}

impl Render for RulesForm {
    fn render(&mut self, _: &mut Window, cx: &mut Context<'_, Self>) -> impl IntoElement {
        let rules = self.rules(cx);
        let switch = SwitchRow::new("rules-on", "Add matching downloads automatically", self.on)
            .help("A new download joins this queue when it matches any rule below.")
            .on_toggle(cx.listener(|form, on: &bool, _, cx| {
                form.on = *on;
                form.forget_answer(cx);
            }));
        let rows: Vec<AnyElement> = self
            .rows
            .clone()
            .into_iter()
            .enumerate()
            .map(|(index, kind)| self.row(index, kind, cx))
            .collect();
        let card = panel(cx).child(switch).when(self.on, |card| {
            card.children(rows)
                .when(self.rows.len() < Kind::ALL.len(), |card| {
                    card.child(
                        h_flex()
                            .px_4()
                            .py_2p5()
                            .border_t_1()
                            .border_color(cx.theme().border)
                            .child(
                                Button::new("add-rule")
                                    .ghost()
                                    .small()
                                    .icon(IconName::Plus)
                                    .label("Add a rule")
                                    .on_click(cx.listener(|form, _, window, cx| {
                                        form.add_row(window, cx);
                                    })),
                            ),
                    )
                })
                .when(!has_rules(&rules), |card| {
                    card.child(
                        div()
                            .px_4()
                            .pb_3()
                            .text_role(Role::Meta, cx)
                            .text_color(cx.theme().warning)
                            .child("Add a rule, or this queue adds nothing by itself."),
                    )
                })
        });
        v_flex()
            .gap_4()
            .child(card)
            .when(self.on && has_rules(&rules), |page| {
                page.child(
                    panel(cx).child(
                        v_flex()
                            .px_4()
                            .py_3()
                            .gap_2()
                            .child(div().text_role(Role::Strong, cx).child("Test a link"))
                            .child(
                                div()
                                    .text_role(Role::Meta, cx)
                                    .child("See which queue a link goes to, and why."),
                            )
                            .child(self.trial.clone()),
                    ),
                )
            })
            .child(ties(cx))
    }
}

impl RulesForm {
    /// One rule: what it looks at, its values, and its remove button.
    fn row(&self, index: usize, kind: Kind, cx: &mut Context<'_, Self>) -> AnyElement {
        let me = cx.entity().downgrade();
        let shown = self.rows.clone();
        let picker = Button::new(("rule-kind", index))
            .outline()
            .w(px(KIND_WIDTH))
            .flex_shrink_0()
            .label(kind.label())
            .dropdown_caret(true)
            .dropdown_menu(move |menu, _, _| {
                Kind::ALL.into_iter().fold(menu, |menu, offered| {
                    let me = me.clone();
                    menu.item(
                        PopupMenuItem::new(offered.label())
                            .checked(offered == kind)
                            .disabled(offered != kind && shown.contains(&offered))
                            .on_click(move |_, window, cx| {
                                crate::closing::may_have_closed(me.update(cx, |form, cx| {
                                    form.set_kind(index, offered, window, cx);
                                }));
                            }),
                    )
                })
            });
        h_flex()
            .px_4()
            .py_2p5()
            .gap_2p5()
            .items_start()
            .border_t_1()
            .border_color(cx.theme().border)
            .child(picker)
            .child(div().flex_1().min_w_0().child(self.values(kind, cx)))
            .child(
                Button::new(("remove-rule", index))
                    .ghost()
                    .small()
                    .flex_shrink_0()
                    .icon(IconName::Close)
                    .tooltip("Remove rule")
                    .on_click(cx.listener(move |form, _, _, cx| form.remove_row(index, cx))),
            )
            .into_any_element()
    }

    fn values(&self, kind: Kind, cx: &mut Context<'_, Self>) -> AnyElement {
        match kind {
            Kind::Sites => v_flex()
                .gap_1p5()
                .child(self.sites.clone())
                .child(hint(cx, "A site covers its subdomains. *.example.com, IP addresses and ranges (10.0.0.0/8) work too, and <local> takes the devices on your own network."))
                .into_any_element(),
            Kind::FileNames => v_flex()
                .gap_1p5()
                .child(self.file_names.clone())
                .child(
                    h_flex()
                        .gap_3()
                        .child(choice::one_of(
                            "syntax",
                            &["Wildcards", "Regular expression"],
                            usize::from(self.syntax == NameSyntax::Regex),
                            cx.listener(|form, picked: &usize, _, cx| {
                                form.syntax = if *picked == 1 {
                                    NameSyntax::Regex
                                } else {
                                    NameSyntax::Wildcards
                                };
                                form.forget_answer(cx);
                            }),
                            cx,
                        ))
                        .child(hint(cx, match self.syntax {
                            NameSyntax::Wildcards => {
                                "* stands for any text and ? for one character; case is ignored."
                            }
                            NameSyntax::Regex => "Case is ignored.",
                        })),
                )
                .into_any_element(),
            Kind::FileTypes => self.file_types.clone().into_any_element(),
            Kind::Categories => self.categories_fields(cx),
            Kind::Protocols => self.protocols_fields(cx),
        }
    }

    fn categories_fields(&self, cx: &mut Context<'_, Self>) -> AnyElement {
        let options: Vec<(&'static str, bool)> = CATEGORIES
            .iter()
            .map(|category| {
                (
                    text::category_label(*category),
                    self.categories.contains(category),
                )
            })
            .collect();
        h_flex()
            .flex_wrap()
            .child(choice::any_of(
                "categories",
                &options,
                cx.listener(|form, picked: &[usize], _, cx| {
                    form.categories = picked
                        .iter()
                        .filter_map(|i| CATEGORIES.get(*i))
                        .copied()
                        .collect();
                    form.forget_answer(cx);
                }),
                cx,
            ))
            .into_any_element()
    }

    fn protocols_fields(&self, cx: &mut Context<'_, Self>) -> AnyElement {
        let options: Vec<(&'static str, bool)> = PROTOCOLS
            .iter()
            .map(|(protocol, label)| (*label, self.protocols.contains(protocol)))
            .collect();
        choice::any_of(
            "protocols",
            &options,
            cx.listener(|form, picked: &[usize], _, cx| {
                form.protocols = picked
                    .iter()
                    .filter_map(|i| PROTOCOLS.get(*i))
                    .map(|(protocol, _)| *protocol)
                    .collect();
                form.forget_answer(cx);
            }),
            cx,
        )
        .into_any_element()
    }
}

fn hint(cx: &App, hint: &'static str) -> Div {
    div().text_role(Role::Meta, cx).child(hint)
}

/// How a tie between queues is settled, in one line, with the order rules are checked in behind
/// its info icon (D84.2).
fn ties(cx: &App) -> impl IntoElement {
    h_flex()
        .gap_1p5()
        .text_role(Role::Meta, cx)
        .child(TIES)
        .child(
            div()
                .id("rules-order")
                .flex_shrink_0()
                .child(Icon::new(Lucide::Info).xsmall())
                .tooltip(tooltip::plain(HELP)),
        )
}

pub(super) fn has_rules(rules: &QueueRules) -> bool {
    !(rules.sites.is_empty()
        && rules.file_names.is_empty()
        && rules.file_types.is_empty()
        && rules.categories.is_empty()
        && rules.protocols.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extensions_are_kept_without_the_dot_in_lower_case() {
        assert_eq!(
            text::typed_extensions(".ISO, tar.gz;  zip"),
            ["iso", "tar.gz", "zip"]
        );
        assert_eq!(text::typed_extensions(" , ."), Vec::<String>::new());
        assert_eq!(text::typed_extensions("iso\nzip"), ["iso", "zip"]);
    }

    #[test]
    fn sites_are_split_at_commas_and_spaces() {
        assert_eq!(
            typed_sites(" youtube.com, *.example.com  10.0.0.0/8;<local>"),
            ["youtube.com", "*.example.com", "10.0.0.0/8", "<local>"]
        );
        assert_eq!(typed_sites(" , "), Vec::<String>::new());
    }
}

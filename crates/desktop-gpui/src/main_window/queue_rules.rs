//! A queue's rules (D72.2): switched on, new downloads that match any rule join the queue. Each
//! kind of rule sits behind its own checkbox, and unchecking keeps its values until the queue is
//! saved (D84.5). Test a link asks the core what the rules as typed do with a link (D72.7).

use std::rc::Rc;

use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::input::{Input, InputEvent, InputState, Textarea, TextareaState};
use gpui_kit::component::{ActiveTheme as _, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, AppContext as _, Context, Entity, IntoElement, ParentElement as _, Render,
    Styled as _, Subscription, Window, div,
};
use wirecatch_protocol::QueueId;
use wirecatch_protocol::app::Category;
use wirecatch_protocol::rules::{AddDownloads, DraftRules, LinkProtocol, NameSyntax, QueueRules};

use super::link_trial::{Draft, Drafted, LinkTrial};
use crate::library::Library;
use crate::text;
use crate::ui::{Role, SwitchRow, Typed as _, choice};

const HELP: &str = "New downloads that match any rule below go to this queue. Rules are checked in this order: site, file name, file type, category, protocol; if several queues match, the one highest in the sidebar wins.";
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

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Sites,
    FileNames,
    FileTypes,
    Categories,
    Protocols,
}

pub struct RulesForm {
    /// The queue edited; `None` for one not made yet.
    queue: Option<QueueId>,
    /// The queue's name as its form has it.
    name: Entity<InputState>,
    on: bool,
    checked: Vec<Kind>,
    sites: Entity<TextareaState>,
    file_names: Entity<TextareaState>,
    syntax: NameSyntax,
    file_types: Entity<InputState>,
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
        let sites = lines_field(&rules.sites, "youtube.com", window, cx);
        let file_names = lines_field(&rules.file_names, "*.iso", window, cx);
        let file_types = cx.new(|cx| {
            let mut state = InputState::new(window, cx).placeholder("iso, tar.gz");
            state.set_value(rules.file_types.join(", "), window, cx);
            state
        });
        let me = cx.entity().downgrade();
        let drafted: Drafted = Rc::new(move |cx| me.upgrade().map(|form| form.read(cx).draft(cx)));
        let trial = cx.new(|cx| LinkTrial::new(library.clone(), Some(drafted), window, cx));
        let checked = [
            (Kind::Sites, !rules.sites.is_empty()),
            (Kind::FileNames, !rules.file_names.is_empty()),
            (Kind::FileTypes, !rules.file_types.is_empty()),
            (Kind::Categories, !rules.categories.is_empty()),
            (Kind::Protocols, !rules.protocols.is_empty()),
        ]
        .into_iter()
        .filter_map(|(kind, used)| used.then_some(kind))
        .collect();
        let mut subscriptions: Vec<Subscription> = [&sites, &file_names]
            .map(|field| {
                cx.subscribe(field, |form: &mut Self, _, event: &InputEvent, cx| {
                    if matches!(event, InputEvent::Change) {
                        form.forget_answer(cx);
                    }
                })
            })
            .into();
        subscriptions.push(cx.subscribe(
            &file_types,
            |form: &mut Self, _, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    form.forget_answer(cx);
                }
            },
        ));
        Self {
            queue,
            name,
            on,
            checked,
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

    fn rules(&self, cx: &App) -> QueueRules {
        let when = |kind: Kind, values: Vec<String>| {
            if self.checked.contains(&kind) {
                values
            } else {
                Vec::new()
            }
        };
        QueueRules {
            enabled: self.on,
            sites: when(Kind::Sites, lines(&self.sites.read(cx).value())),
            file_names: when(Kind::FileNames, lines(&self.file_names.read(cx).value())),
            file_name_syntax: self.syntax,
            file_types: when(
                Kind::FileTypes,
                file_types(&self.file_types.read(cx).value()),
            ),
            categories: if self.checked.contains(&Kind::Categories) {
                self.categories.clone()
            } else {
                Vec::new()
            },
            protocols: if self.checked.contains(&Kind::Protocols) {
                self.protocols.clone()
            } else {
                Vec::new()
            },
        }
    }

    /// An answer is for the link and the rules it was given: a change to either takes it away.
    fn forget_answer(&mut self, cx: &mut Context<'_, Self>) {
        self.trial.update(cx, LinkTrial::forget);
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

    fn check(&mut self, kind: Kind, on: bool, cx: &mut Context<'_, Self>) {
        self.checked.retain(|checked| *checked != kind);
        if on {
            self.checked.push(kind);
        }
        self.forget_answer(cx);
        cx.notify();
    }
}

impl Render for RulesForm {
    fn render(&mut self, _: &mut Window, cx: &mut Context<'_, Self>) -> impl IntoElement {
        let rules = self.rules(cx);
        let fields = v_flex()
            .gap_3()
            .child(self.kind(
                Kind::Sites,
                "By site",
                Some("One per line. A site covers its subdomains; *.example.com, IP addresses, ranges (10.0.0.0/8) and <local> work too."),
                Textarea::new(&self.sites).into_any_element(),
                cx,
            ))
            .child(self.kind(
                Kind::FileNames,
                "By file name",
                Some(match self.syntax {
                    NameSyntax::Wildcards => {
                        "One per line. * stands for any text and ? for one character; case is ignored."
                    }
                    NameSyntax::Regex => "One regular expression per line; case is ignored.",
                }),
                self.file_names_fields(cx),
                cx,
            ))
            .child(self.kind(
                Kind::FileTypes,
                "By file type",
                Some("Extensions, with or without the dot, separated by commas."),
                Input::new(&self.file_types).into_any_element(),
                cx,
            ))
            .child(self.kind(
                Kind::Categories,
                "By category",
                None,
                self.categories_fields(cx),
                cx,
            ))
            .child(self.kind(
                Kind::Protocols,
                "By protocol",
                None,
                self.protocols_fields(cx),
                cx,
            ))
            .map(|fields| {
                if has_rules(&rules) {
                    fields.child(self.trial(cx))
                } else {
                    fields.child(
                        div()
                            .text_role(Role::Meta, cx)
                            .text_color(cx.theme().warning)
                            .child("Add a rule, or this queue adds nothing by itself."),
                    )
                }
            });
        SwitchRow::new("rules-on", "Add matching downloads automatically", self.on)
            .help(HELP)
            .on_toggle(cx.listener(|form, on: &bool, _, cx| {
                form.on = *on;
                form.forget_answer(cx);
                cx.notify();
            }))
            .fields(fields)
    }
}

impl RulesForm {
    /// One kind of rule: its checkbox, and its values under it while it is checked.
    fn kind(
        &self,
        kind: Kind,
        label: &'static str,
        help: Option<&'static str>,
        values: AnyElement,
        cx: &mut Context<'_, Self>,
    ) -> AnyElement {
        let checked = self.checked.contains(&kind);
        v_flex()
            .gap_2()
            .child(
                Checkbox::new(label)
                    .label(label)
                    .checked(checked)
                    .on_click(cx.listener(move |form, on: &bool, _, cx| form.check(kind, *on, cx))),
            )
            .when(checked, |row| {
                row.child(
                    v_flex()
                        .pl_6()
                        .gap_1p5()
                        .child(values)
                        .when_some(help, |row, help| {
                            row.child(div().text_role(Role::Meta, cx).child(help))
                        }),
                )
            })
            .into_any_element()
    }

    fn file_names_fields(&self, cx: &mut Context<'_, Self>) -> AnyElement {
        v_flex()
            .gap_2()
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
                    cx.notify();
                }),
                cx,
            ))
            .child(Textarea::new(&self.file_names))
            .into_any_element()
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
        choice::any_of(
            "categories",
            &options,
            cx.listener(|form, picked: &[usize], _, cx| {
                form.categories = picked
                    .iter()
                    .filter_map(|i| CATEGORIES.get(*i))
                    .copied()
                    .collect();
                form.forget_answer(cx);
                cx.notify();
            }),
            cx,
        )
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
                cx.notify();
            }),
            cx,
        )
        .into_any_element()
    }

    /// Test a link, under the rules it tries.
    fn trial(&self, cx: &mut Context<'_, Self>) -> AnyElement {
        v_flex()
            .pt_2()
            .gap_2()
            .border_t_1()
            .border_color(cx.theme().border)
            .child(
                div()
                    .pt_2()
                    .text_role(Role::Strong, cx)
                    .child("Test a link"),
            )
            .child(self.trial.clone())
            .into_any_element()
    }
}

fn lines_field(
    values: &[String],
    placeholder: &'static str,
    window: &mut Window,
    cx: &mut Context<'_, RulesForm>,
) -> Entity<TextareaState> {
    let value = values.join("\n");
    cx.new(|cx| {
        let mut state = TextareaState::new(window, cx)
            .auto_grow(2, 6)
            .placeholder(placeholder);
        state.set_value(value, window, cx);
        state
    })
}

/// The lines of `typed` that hold something, trimmed.
fn lines(typed: &str) -> Vec<String> {
    typed
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect()
}

/// Extensions as a rule keeps them: without the dot, in lower case.
fn file_types(typed: &str) -> Vec<String> {
    typed
        .split([',', ' ', ';'])
        .map(|typed| typed.trim().trim_start_matches('.').to_lowercase())
        .filter(|extension| !extension.is_empty())
        .collect()
}

fn has_rules(rules: &QueueRules) -> bool {
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
        assert_eq!(file_types(".ISO, tar.gz;  zip"), ["iso", "tar.gz", "zip"]);
        assert_eq!(file_types(" , ."), Vec::<String>::new());
    }

    #[test]
    fn blank_lines_are_left_out() {
        assert_eq!(
            lines(" youtube.com \n\n  \n*.example.com"),
            ["youtube.com", "*.example.com"]
        );
    }
}

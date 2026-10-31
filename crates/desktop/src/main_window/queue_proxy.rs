//! A queue's proxy (D73.2): the global one or its own, with New proxy; and while a proxy is in
//! use, the hosts it reaches directly: the global list, or its own pick of the system's bypass
//! list, the global hosts and hosts of its own (D73.7).

use std::rc::Rc;

use gpui_kit::component::button::Button;
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::input::{InputEvent, Textarea, TextareaState};
use gpui_kit::component::select::{Select, SelectEvent, SelectItem, SelectState};
use gpui_kit::component::{ActiveTheme as _, IconName, IndexPath, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, AppContext as _, Context, Entity, IntoElement, ParentElement as _, Render,
    SharedString, Styled as _, Subscription, Window, div,
};
use wirecatch_protocol::app::Setting;
use wirecatch_protocol::proxy::{ProxySettings, ProxyUse, QueueExceptions};

use super::proxy_dialog;
use crate::library::Library;
use crate::ui::{Field, Role, Typed as _, choice};

/// One entry of the proxy list: the global proxy, or one of the queue's own.
#[derive(Clone)]
struct ProxyChoice {
    title: SharedString,
    proxy: Setting<ProxyUse>,
}

impl SelectItem for ProxyChoice {
    type Value = Setting<ProxyUse>;

    fn title(&self) -> SharedString {
        self.title.clone()
    }

    fn value(&self) -> &Self::Value {
        &self.proxy
    }
}

pub struct ProxyForm {
    library: Entity<Library>,
    proxy: Setting<ProxyUse>,
    exceptions: Setting<QueueExceptions>,
    choices: Entity<SelectState<Vec<ProxyChoice>>>,
    hosts: Entity<TextareaState>,
    _subscriptions: Vec<Subscription>,
}

impl ProxyForm {
    pub fn new(
        library: Entity<Library>,
        proxy: Setting<ProxyUse>,
        exceptions: Setting<QueueExceptions>,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) -> Self {
        let items = proxy_choices(&library, cx);
        let picked = items.iter().position(|item| item.proxy == proxy);
        let choices = cx.new(|cx| {
            SelectState::new(
                items,
                picked.map(|row| IndexPath::default().row(row)),
                window,
                cx,
            )
        });
        let own_hosts = exceptions
            .own_ref()
            .map(|own| own.hosts.join("\n"))
            .unwrap_or_default();
        let hosts = cx.new(|cx| {
            TextareaState::new(window, cx)
                .auto_grow(2, 6)
                .placeholder("intranet.example.com")
                .default_value(own_hosts)
        });
        let picking = cx.subscribe(
            &choices,
            |form: &mut Self, _, event: &SelectEvent<Vec<ProxyChoice>>, cx| {
                let SelectEvent::Confirm(Some(picked)) = event else {
                    return;
                };
                form.proxy = *picked;
                cx.notify();
            },
        );
        // A proxy added or renamed while the editor is open shows in the list.
        let listing = cx.observe_in(&library, window, |form: &mut Self, library, window, cx| {
            let items = proxy_choices(&library, cx);
            let proxy = form.proxy;
            form.choices.update(cx, |state, cx| {
                state.set_items(items, window, cx);
                state.set_selected_value(&proxy, window, cx);
            });
        });
        let typing = cx.subscribe(&hosts, |_, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                cx.notify();
            }
        });
        Self {
            library,
            proxy,
            exceptions,
            choices,
            hosts,
            _subscriptions: vec![picking, listing, typing],
        }
    }

    pub fn proxy(&self) -> Setting<ProxyUse> {
        self.proxy
    }

    /// The queue's exceptions, its own hosts read from their field.
    pub fn exceptions(&self, cx: &App) -> Setting<QueueExceptions> {
        match &self.exceptions {
            Setting::Global => Setting::Global,
            Setting::Own(own) => Setting::Own(QueueExceptions {
                hosts: self
                    .hosts
                    .read(cx)
                    .value()
                    .lines()
                    .map(str::trim)
                    .filter(|line| !line.is_empty())
                    .map(str::to_owned)
                    .collect(),
                ..own.clone()
            }),
        }
    }

    fn app_proxy(&self, cx: &App) -> ProxySettings {
        self.library
            .read(cx)
            .settings
            .as_ref()
            .map(|settings| settings.proxy.clone())
            .unwrap_or_default()
    }

    fn new_proxy(&self, window: &mut Window, cx: &mut Context<'_, Self>) {
        let form = cx.entity().downgrade();
        let use_it: proxy_dialog::Created = Rc::new(move |id, window, cx| {
            _ = form.update(cx, |form, cx| {
                form.proxy = Setting::Own(ProxyUse::Server(id));
                let picked = form.proxy;
                form.choices.update(cx, |state, cx| {
                    state.set_selected_value(&picked, window, cx);
                });
                cx.notify();
            });
        });
        proxy_dialog::open(&self.library, None, Some(use_it), window, cx);
    }

    fn edit_exceptions(
        &mut self,
        edit: impl FnOnce(&mut QueueExceptions),
        cx: &mut Context<'_, Self>,
    ) {
        if let Setting::Own(own) = &mut self.exceptions {
            edit(own);
            cx.notify();
        }
    }
}

impl Render for ProxyForm {
    fn render(&mut self, _: &mut Window, cx: &mut Context<'_, Self>) -> impl IntoElement {
        let app = self.app_proxy(cx);
        let resulting = match self.proxy {
            Setting::Global => app.proxy,
            Setting::Own(proxy) => proxy,
        };
        let picker = h_flex()
            .gap_2()
            .child(div().flex_1().child(Select::new(&self.choices)))
            .child(
                Button::new("queue-new-proxy")
                    .outline()
                    .icon(IconName::Plus)
                    .label("New proxy")
                    .on_click(cx.listener(|form, _, window, cx| form.new_proxy(window, cx))),
            );
        v_flex()
            .child(
                v_flex().px_4().py_3().child(
                    Field::new("Proxy", picker)
                        .note("Downloads already running keep the connection they started with."),
                ),
            )
            .when(resulting != ProxyUse::Direct, |form| {
                form.child(self.exceptions_row(&app, cx))
            })
    }
}

impl ProxyForm {
    fn exceptions_row(&self, app: &ProxySettings, cx: &mut Context<'_, Self>) -> AnyElement {
        let own = self.exceptions.own_ref().cloned();
        let from_app = QueueExceptions {
            system: app.exceptions.system,
            app_hosts: app.exceptions.use_hosts,
            use_hosts: false,
            hosts: Vec::new(),
        };
        let source = choice::one_of(
            "queue-exceptions",
            &["Same as global", "Custom"],
            usize::from(own.is_some()),
            cx.listener(move |form, picked: &usize, _, cx| {
                form.exceptions = if *picked == 1 {
                    Setting::Own(from_app.clone())
                } else {
                    Setting::Global
                };
                cx.notify();
            }),
            cx,
        );
        let global_hosts = match app.exceptions.hosts.len() {
            0 => "Global hosts (empty)".to_owned(),
            n => format!("Global hosts ({n})"),
        };
        let border = cx.theme().border;
        v_flex()
            .px_4()
            .py_3()
            .gap_3()
            .border_t_1()
            .border_color(border)
            .child(
                h_flex()
                    .gap_4()
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .gap_0p5()
                            .child(div().text_role(Role::Strong, cx).child("Hosts that skip the proxy"))
                            .child(div().text_role(Role::Meta, cx).child(
                                "Hosts this queue connects to directly. Same as global uses the list in Settings, Network.",
                            )),
                    )
                    .child(source),
            )
            .when_some(own, |row, own| row.child(self.own_list(&own, global_hosts, cx)))
            .into_any_element()
    }
}

impl ProxyForm {
    /// The lists a queue of its own picks from, and its own hosts.
    fn own_list(
        &self,
        own: &QueueExceptions,
        global_hosts: String,
        cx: &mut Context<'_, Self>,
    ) -> gpui_kit::Div {
        v_flex()
            .gap_3()
            .child(
                Checkbox::new("queue-system-exceptions")
                    .label("System proxy exceptions")
                    .checked(own.system)
                    .on_click(cx.listener(|form, on: &bool, _, cx| {
                        let on = *on;
                        form.edit_exceptions(|own| own.system = on, cx);
                    })),
            )
            .child(
                Checkbox::new("queue-app-hosts")
                    .label(global_hosts)
                    .checked(own.app_hosts)
                    .on_click(cx.listener(|form, on: &bool, _, cx| {
                        let on = *on;
                        form.edit_exceptions(|own| own.app_hosts = on, cx);
                    })),
            )
            .child(
                Checkbox::new("queue-own-hosts")
                    .label("This queue's hosts")
                    .checked(own.use_hosts)
                    .on_click(cx.listener(|form, on: &bool, _, cx| {
                        let on = *on;
                        form.edit_exceptions(|own| own.use_hosts = on, cx);
                    })),
            )
            .when(own.use_hosts, |list| {
                list.child(
                    v_flex()
                        .pl_6()
                        .gap_1p5()
                        .child(Textarea::new(&self.hosts))
                        .child(div().text_role(Role::Meta, cx).child(
                            "One per line. A host covers its subdomains; IP addresses, ranges (10.0.0.0/8) and <local> work too.",
                        )),
                )
            })
    }
}

/// Every proxy a queue may use, the global one first, named after what it is now.
fn proxy_choices(library: &Entity<Library>, cx: &App) -> Vec<ProxyChoice> {
    let read = library.read(cx);
    let global = read
        .settings
        .as_ref()
        .map_or(ProxyUse::System, |s| s.proxy.proxy);
    let name = |proxy: ProxyUse| match proxy {
        ProxyUse::System => "System proxy".to_owned(),
        ProxyUse::Direct => "No proxy".to_owned(),
        ProxyUse::Server(id) => read
            .proxies
            .iter()
            .find(|server| server.id == id)
            .map_or_else(
                || "A deleted proxy".to_owned(),
                |server| proxy_dialog::shown_name(&server.settings),
            ),
    };
    let mut items = vec![ProxyChoice {
        title: format!("Global proxy ({})", name(global)).into(),
        proxy: Setting::Global,
    }];
    items.extend(
        [ProxyUse::System, ProxyUse::Direct]
            .into_iter()
            .chain(
                read.proxies
                    .iter()
                    .map(|server| ProxyUse::Server(server.id)),
            )
            .map(|proxy| ProxyChoice {
                title: name(proxy).into(),
                proxy: Setting::Own(proxy),
            }),
    );
    items
}

//! One proxy server (D73.1): its name, type, server, port and login, with Test connection before
//! it is saved; and deleting one, with what its users switch to. A saved password is never shown;
//! it stays while the type, server, port and user name stay (D71.2).

use std::rc::Rc;

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputEvent, InputState, NumberInput};
use gpui_kit::component::select::{Select, SelectItem, SelectState};
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, IndexPath, WindowExt as _, h_flex, v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    App, AppContext as _, Context, Div, Entity, IntoElement, ParentElement as _, Render,
    SharedString, Styled as _, Subscription, Window, div, px,
};
use wirecatch_protocol::app::{Setting, Update};
use wirecatch_protocol::proxy::{
    ProxyKind, ProxyServer, ProxyServerDraft, ProxyServerSettings, ProxyTestResult, ProxyUse,
};
use wirecatch_protocol::{ProxyServerId, Secret};
use wirecatch_shell::api;

use crate::backend::{Backend, act};
use crate::library::Library;
use crate::ui::{Field, Role, Typed as _, choice};

const WIDTH: f32 = 520.;
const KINDS: [(ProxyKind, &str); 3] = [
    (ProxyKind::Http, "HTTP"),
    (ProxyKind::Https, "HTTPS"),
    (ProxyKind::Socks5, "SOCKS5"),
];
/// The port SOCKS proxies listen on (RFC 1928).
const SOCKS_PORT: u16 = 1080;
const HTTP_PROXY_PORT: u16 = 8080;

/// Called with a new proxy's id once it is saved.
pub(super) type Created = Rc<dyn Fn(ProxyServerId, &mut Window, &mut App)>;

/// Opens the editor on proxy server `id`, or on a new one, which `created` then gets.
pub(super) fn open(
    library: &Entity<Library>,
    id: Option<ProxyServerId>,
    created: Option<Created>,
    window: &mut Window,
    cx: &mut App,
) {
    let form = cx.new(|cx| ProxyForm::new(library, id, created, window, cx));
    crate::ui::dialog::open(window, cx, move |dialog, _, cx| {
        let saving = form.clone();
        let state = form.read(cx);
        let title = match &state.original {
            Some(proxy) => format!("Edit {}", shown_name(&proxy.settings)),
            None => "New proxy".to_owned(),
        };
        dialog
            .title(title)
            .w(px(WIDTH))
            .child(form.clone())
            .on_ok(move |_, window, cx| {
                saving.update(cx, |form, cx| form.save(window, cx));
                false
            })
            .footer(footer(&form, cx))
    });
}

fn footer(form: &Entity<ProxyForm>, cx: &App) -> Div {
    let state = form.read(cx);
    let (test, save) = (form.clone(), form.clone());
    h_flex()
        .w_full()
        .gap_2()
        .child(
            Button::new("test-proxy")
                .label("Test connection")
                .loading(state.stage == Stage::Testing)
                .disabled(state.stage != Stage::Editing)
                .on_click(move |_, window, cx| test.update(cx, |form, cx| form.test(window, cx))),
        )
        .child(div().flex_1())
        .child(
            Button::new("cancel")
                .label("Cancel")
                .on_click(|_, window, cx| window.close_dialog(cx)),
        )
        .child(
            Button::new("save-proxy")
                .primary()
                .label("Save")
                .loading(state.stage == Stage::Saving)
                .disabled(state.stage != Stage::Editing)
                .on_click(move |_, window, cx| save.update(cx, |form, cx| form.save(window, cx))),
        )
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Stage {
    Editing,
    Testing,
    Saving,
}

/// What the form does with the password.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Password {
    /// The saved one, or none; a typed one replaces it.
    Keep,
    Remove,
}

struct ProxyForm {
    original: Option<ProxyServer>,
    created: Option<Created>,
    name: Entity<InputState>,
    kind: ProxyKind,
    host: Entity<InputState>,
    port: Entity<InputState>,
    user: Entity<InputState>,
    password: Entity<InputState>,
    saved_password: Password,
    /// What Test connection found, or why the form could not be tried or saved.
    outcome: Option<Result<ProxyTestResult, String>>,
    stage: Stage,
    _subscriptions: Vec<Subscription>,
}

impl ProxyForm {
    fn new(
        library: &Entity<Library>,
        id: Option<ProxyServerId>,
        created: Option<Created>,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) -> Self {
        let original = id.and_then(|id| {
            library
                .read(cx)
                .proxies
                .iter()
                .find(|proxy| proxy.id == id)
                .cloned()
        });
        let opened = original.as_ref().map(|proxy| proxy.settings.clone());
        let kind = opened.as_ref().map_or(ProxyKind::Http, |s| s.kind);
        let name = text_field(
            opened.as_ref().map(|s| s.name.clone()).unwrap_or_default(),
            "For example: Office",
            window,
            cx,
        );
        let host = text_field(
            opened.as_ref().map(|s| s.host.clone()).unwrap_or_default(),
            "proxy.example.com",
            window,
            cx,
        );
        let user = text_field(
            opened.as_ref().map(|s| s.user.clone()).unwrap_or_default(),
            "Leave empty if it needs no sign-in",
            window,
            cx,
        );
        let port = cx.new(|cx| {
            let mut state = InputState::new(window, cx)
                .step(1.)
                .min(1.)
                .max(f64::from(u16::MAX));
            state.set_value(
                opened
                    .as_ref()
                    .map_or(usual_port(kind), |s| s.port)
                    .to_string(),
                window,
                cx,
            );
            state
        });
        let password = cx.new(|cx| {
            InputState::new(window, cx).masked(true).placeholder(
                if original.as_ref().is_some_and(|p| p.has_password) {
                    "Leave empty to keep the saved password"
                } else {
                    ""
                },
            )
        });
        let edited = [&name, &host, &port, &user, &password].map(|field| {
            cx.subscribe(field, |form: &mut Self, _, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    form.outcome = None;
                    cx.notify();
                }
            })
        });
        Self {
            original,
            created,
            name,
            kind,
            host,
            port,
            user,
            password,
            saved_password: Password::Keep,
            outcome: None,
            stage: Stage::Editing,
            _subscriptions: edited.into(),
        }
    }

    fn settings(&self, cx: &App) -> ProxyServerSettings {
        ProxyServerSettings {
            name: self.name.read(cx).value().trim().to_owned(),
            kind: self.kind,
            host: self.host.read(cx).value().trim().to_owned(),
            port: self
                .port
                .read(cx)
                .value()
                .trim()
                .parse()
                .unwrap_or_else(|_| usual_port(self.kind)),
            user: self.user.read(cx).value().trim().to_owned(),
        }
    }

    fn draft(&self, cx: &App) -> ProxyServerDraft {
        let typed = self.password.read(cx).value().to_string();
        ProxyServerDraft {
            settings: self.settings(cx),
            password: if !typed.is_empty() {
                Update::Set(Some(Secret::from(typed)))
            } else if self.saved_password == Password::Remove {
                Update::Set(None)
            } else {
                Update::Keep
            },
        }
    }

    /// Whether a login was given that the proxy could refuse.
    fn login_entered(&self, cx: &App) -> bool {
        !self.user.read(cx).value().trim().is_empty()
            || !self.password.read(cx).value().is_empty()
            || self.original.as_ref().is_some_and(|p| p.has_password)
                && self.saved_password == Password::Keep
    }

    fn test(&mut self, window: &mut Window, cx: &mut Context<'_, Self>) {
        if self.stage != Stage::Editing {
            return;
        }
        let id = self.original.as_ref().map(|proxy| proxy.id);
        let draft = self.draft(cx);
        self.stage = Stage::Testing;
        self.outcome = None;
        cx.notify();
        let tested = Backend::run(cx, move |manager| async move {
            api::test_proxy_server(&manager, id, draft).await
        });
        cx.spawn_in(window, async move |form, cx| {
            let outcome = match tested.await {
                Ok(Ok(result)) => result.map_err(|error| error.message),
                Ok(Err(unopened)) => Err(unopened.reason.message),
                Err(_) => return,
            };
            _ = form.update(cx, |form, cx| {
                form.stage = Stage::Editing;
                form.outcome = Some(outcome);
                cx.notify();
            });
        })
        .detach();
    }

    fn save(&mut self, window: &mut Window, cx: &mut Context<'_, Self>) {
        if self.stage != Stage::Editing {
            return;
        }
        let id = self.original.as_ref().map(|proxy| proxy.id);
        let draft = self.draft(cx);
        self.stage = Stage::Saving;
        cx.notify();
        let saved = Backend::run(cx, move |manager| async move {
            match id {
                Some(id) => api::update_proxy_server(&manager, id, draft).map(|()| None),
                None => api::create_proxy_server(&manager, draft).map(Some),
            }
        });
        cx.spawn_in(window, async move |form, cx| {
            let outcome = match saved.await {
                Ok(Ok(result)) => result.map_err(|error| error.message),
                Ok(Err(unopened)) => Err(unopened.reason.message),
                Err(_) => return,
            };
            _ = form.update_in(cx, |form, window, cx| {
                form.stage = Stage::Editing;
                match outcome {
                    Ok(made) => {
                        window.close_dialog(cx);
                        if let (Some(id), Some(created)) = (made, form.created.clone()) {
                            created(id, window, cx);
                        }
                    }
                    Err(problem) => {
                        form.outcome = Some(Err(problem));
                        cx.notify();
                    }
                }
            });
        })
        .detach();
    }

    fn set_kind(&mut self, kind: ProxyKind, window: &mut Window, cx: &mut Context<'_, Self>) {
        let typed = self.port.read(cx).value().trim().parse::<u16>().ok();
        // A port still at the old type's usual one follows the type.
        if typed.is_none_or(|port| port == usual_port(self.kind)) {
            self.port.update(cx, |state, cx| {
                state.set_value(usual_port(kind).to_string(), window, cx);
            });
        }
        self.kind = kind;
        self.outcome = None;
        cx.notify();
    }
}

impl Render for ProxyForm {
    fn render(&mut self, _: &mut Window, cx: &mut Context<'_, Self>) -> impl IntoElement {
        let has_saved = self.original.as_ref().is_some_and(|p| p.has_password);
        let kinds = choice::one_of(
            "proxy-kind",
            &KINDS.map(|(_, label)| label),
            KINDS
                .iter()
                .position(|(kind, _)| *kind == self.kind)
                .unwrap_or_default(),
            cx.listener(|form, picked: &usize, window, cx| {
                if let Some((kind, _)) = KINDS.get(*picked) {
                    form.set_kind(*kind, window, cx);
                }
            }),
            cx,
        );
        let password = h_flex()
            .gap_2()
            .child(
                div()
                    .flex_1()
                    .child(Input::new(&self.password).mask_toggle()),
            )
            .when(has_saved && self.saved_password == Password::Keep, |row| {
                row.child(
                    Button::new("remove-password")
                        .ghost()
                        .label("Remove saved")
                        .on_click(cx.listener(|form, _, _, cx| {
                            form.saved_password = Password::Remove;
                            form.outcome = None;
                            cx.notify();
                        })),
                )
            });
        let outcome = self.outcome.as_ref().map(|outcome| {
            let (text, color) = match outcome {
                Ok(result) => test_result(result, self.login_entered(cx), cx),
                Err(problem) => (problem.clone(), cx.theme().danger),
            };
            div()
                .text_role(Role::Body, cx)
                .text_color(color)
                .child(text)
        });
        v_flex()
            .gap_4()
            .pb_1()
            .text_sm()
            .child(
                Field::new("Name", Input::new(&self.name))
                    .note("Optional. Without one, the proxy's address is shown."),
            )
            .child(Field::new("Type", kinds))
            .child(
                h_flex()
                    .gap_3()
                    .items_start()
                    .child(
                        div()
                            .flex_1()
                            .child(Field::new("Server", Input::new(&self.host))),
                    )
                    .child(
                        div()
                            .w(px(140.))
                            .child(Field::new("Port", NumberInput::new(&self.port))),
                    ),
            )
            .child(Field::new("User name", Input::new(&self.user)))
            .child(
                Field::new("Password", password)
                    .when(self.saved_password == Password::Remove, |field| {
                        field.note("The saved password is removed when you save.")
                    }),
            )
            .children(outcome)
    }
}

fn text_field(
    value: String,
    placeholder: &'static str,
    window: &mut Window,
    cx: &mut Context<'_, ProxyForm>,
) -> Entity<InputState> {
    cx.new(|cx| {
        let mut state = InputState::new(window, cx).placeholder(placeholder);
        state.set_value(value, window, cx);
        state
    })
}

/// What a test found, in words, and its colour.
fn test_result(
    result: &ProxyTestResult,
    login_entered: bool,
    cx: &App,
) -> (String, gpui_kit::Hsla) {
    let theme = cx.theme();
    match result {
        ProxyTestResult::Works => (
            "The proxy works. A test request went through it.".to_owned(),
            theme.success,
        ),
        ProxyTestResult::LoginRefused => (
            if login_entered {
                "The proxy rejected the user name or password. Check them and try again."
            } else {
                "The proxy requires a user name and password. Enter them and try again."
            }
            .to_owned(),
            theme.danger,
        ),
        ProxyTestResult::Unreachable { reason } => (
            format!(
                "The proxy could not be reached: {}.",
                reason.trim_end_matches('.')
            ),
            theme.danger,
        ),
        ProxyTestResult::TimedOut => (
            "The proxy did not answer in time. Check its address and port.".to_owned(),
            theme.warning,
        ),
    }
}

fn usual_port(kind: ProxyKind) -> u16 {
    match kind {
        ProxyKind::Socks5 => SOCKS_PORT,
        ProxyKind::Http | ProxyKind::Https => HTTP_PROXY_PORT,
    }
}

/// `host:port`, with an IPv6 address in brackets.
pub(super) fn address(settings: &ProxyServerSettings) -> String {
    if settings.host.contains(':') {
        format!("[{}]:{}", settings.host, settings.port)
    } else {
        format!("{}:{}", settings.host, settings.port)
    }
}

/// A proxy's name, or its address when it has none.
pub(super) fn shown_name(settings: &ProxyServerSettings) -> String {
    let name = settings.name.trim();
    if name.is_empty() {
        address(settings)
    } else {
        name.to_owned()
    }
}

pub(super) fn kind_label(kind: ProxyKind) -> &'static str {
    KINDS
        .iter()
        .find(|(known, _)| *known == kind)
        .map_or("", |(_, label)| label)
}

/// Who uses a proxy server: the global setting (with the queues that follow it), and queues of
/// their own.
pub(super) struct Users {
    pub app: bool,
    pub following: Vec<String>,
    pub queues: Vec<String>,
}

impl Users {
    pub(super) fn of(id: ProxyServerId, library: &Library) -> Self {
        let server = ProxyUse::Server(id);
        let app = library
            .settings
            .as_ref()
            .is_some_and(|settings| settings.proxy.proxy == server);
        let named = |own: bool| -> Vec<String> {
            library
                .queues
                .iter()
                .filter(|queue| match &queue.settings.proxy {
                    Setting::Global => !own && app,
                    Setting::Own(proxy) => own && *proxy == server,
                })
                .map(|queue| queue.settings.name.clone())
                .collect()
        };
        Self {
            app,
            following: named(false),
            queues: named(true),
        }
    }

    pub(super) fn is_empty(&self) -> bool {
        !self.app && self.queues.is_empty()
    }

    /// `Used by the global setting (Main) and Videos`, or `Not used yet`.
    pub(super) fn line(&self) -> String {
        let mut parts = Vec::new();
        if self.app {
            parts.push(if self.following.is_empty() {
                "the global setting".to_owned()
            } else {
                format!("the global setting ({})", self.following.join(", "))
            });
        }
        if !self.queues.is_empty() {
            parts.push(self.queues.join(", "));
        }
        if parts.is_empty() {
            "Not used yet".to_owned()
        } else {
            format!("Used by {}", parts.join(" and "))
        }
    }
}

/// One proxy its users may switch to.
#[derive(Clone)]
struct Replacement {
    title: SharedString,
    proxy: ProxyUse,
}

impl SelectItem for Replacement {
    type Value = ProxyUse;

    fn title(&self) -> SharedString {
        self.title.clone()
    }

    fn value(&self) -> &Self::Value {
        &self.proxy
    }
}

/// Asks before deleting proxy server `id`; whatever uses it switches to the proxy picked.
pub(super) fn ask_delete(
    library: &Entity<Library>,
    id: ProxyServerId,
    window: &mut Window,
    cx: &mut App,
) {
    let read = library.read(cx);
    let Some(proxy) = read.proxies.iter().find(|proxy| proxy.id == id) else {
        return;
    };
    let name = shown_name(&proxy.settings);
    let users = Users::of(id, read);
    let mut choices = vec![
        Replacement {
            title: "System proxy".into(),
            proxy: ProxyUse::System,
        },
        Replacement {
            title: "No proxy".into(),
            proxy: ProxyUse::Direct,
        },
    ];
    choices.extend(
        read.proxies
            .iter()
            .filter(|other| other.id != id)
            .map(|other| Replacement {
                title: shown_name(&other.settings).into(),
                proxy: ProxyUse::Server(other.id),
            }),
    );
    let in_use = !users.is_empty();
    let line = users.line();
    let replacement =
        cx.new(|cx| SelectState::new(choices, Some(IndexPath::default()), window, cx));
    crate::ui::dialog::open(window, cx, move |dialog, _, cx| {
        let picked = replacement.clone();
        dialog
            .title(format!("Delete {name}?"))
            .w(px(440.))
            .child(
                v_flex()
                    .gap_4()
                    .text_sm()
                    .child(div().text_role(Role::Secondary, cx).child(if in_use {
                        format!("{line}. They switch to the proxy you pick.")
                    } else {
                        "Nothing uses it.".to_owned()
                    }))
                    .when(in_use, |body| {
                        body.child(Field::new("Switch to", Select::new(&replacement)))
                    }),
            )
            .footer(
                h_flex()
                    .w_full()
                    .gap_2()
                    .justify_end()
                    .child(
                        Button::new("keep-proxy")
                            .label("Keep proxy")
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        Button::new("delete-proxy")
                            .danger()
                            .label("Delete proxy")
                            .on_click(move |_, window, cx| {
                                let to = picked
                                    .read(cx)
                                    .selected_value()
                                    .copied()
                                    .unwrap_or(ProxyUse::System);
                                window.close_dialog(cx);
                                act(window.window_handle(), cx, move |manager| {
                                    api::delete_proxy_server(manager, id, to)
                                });
                            }),
                    ),
            )
    });
}

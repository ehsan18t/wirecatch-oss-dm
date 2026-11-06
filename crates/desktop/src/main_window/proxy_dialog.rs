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
use wirecatch_core::same_proxy;
use wirecatch_protocol::app::{Setting, Update};
use wirecatch_protocol::proxy::{
    ProxyKind, ProxyServer, ProxyServerDraft, ProxyServerSettings, ProxyTestResult, ProxyUse,
};
use wirecatch_protocol::{ProxyServerId, Secret};
use wirecatch_shell::api;

use crate::backend::{Backend, act, said};
use crate::library::Library;
use crate::ui::{Field, Role, Sentence, Typed as _, choice};

const WIDTH: f32 = 520.;
const KINDS: [(ProxyKind, &str); 3] = [
    (ProxyKind::Http, "HTTP"),
    (ProxyKind::Https, "HTTPS"),
    (ProxyKind::Socks5, "SOCKS5"),
];
const KEEPS_PASSWORD: &str = "Leave empty to keep the saved password";
const FORGETS_PASSWORD: &str = "Enter the password: the saved one is for the old details";
const BAD_PORT: &str = "Enter a port from 1 to 65535.";

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
    if id.is_none() {
        let name = form.read(cx).name.clone();
        crate::ui::dialog::focus_after_opening(&name, window, cx);
    }
    crate::ui::dialog::open(window, cx, move |dialog, _, cx| {
        let saving = form.clone();
        let state = form.read(cx);
        let title = match &state.original {
            Some(proxy) => format!("Edit {}", proxy.settings.shown_name()),
            None => "New proxy".to_owned(),
        };
        dialog
            .title(crate::ui::dialog::Header::new(title))
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
        let name = crate::ui::dialog::text_field(
            opened.as_ref().map(|s| s.name.clone()).unwrap_or_default(),
            "For example: Office",
            window,
            cx,
        );
        let host = crate::ui::dialog::text_field(
            opened.as_ref().map(|s| s.host.clone()).unwrap_or_default(),
            "proxy.example.com",
            window,
            cx,
        );
        let user = crate::ui::dialog::text_field(
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
                    KEEPS_PASSWORD
                } else {
                    ""
                },
            )
        });
        let edited = [&name, &host, &port, &user, &password].map(|field| {
            cx.subscribe_in(
                field,
                window,
                |form: &mut Self, _, event: &InputEvent, window, cx| {
                    if matches!(event, InputEvent::Change) {
                        form.outcome = None;
                        form.say_what_happens_to_the_password(window, cx);
                        cx.notify();
                    }
                },
            )
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

    /// What the form holds; `None` while the port is not one.
    fn settings(&self, cx: &App) -> Option<ProxyServerSettings> {
        Some(ProxyServerSettings {
            name: self.name.read(cx).value().trim().to_owned(),
            kind: self.kind,
            host: self.host.read(cx).value().trim().to_owned(),
            port: typed_port(&self.port.read(cx).value())?,
            user: self.user.read(cx).value().trim().to_owned(),
        })
    }

    /// Whether the saved password goes with what the form holds: it is forgotten when the type,
    /// server, port or user name changes (D71.2).
    fn keeps_password(&self, cx: &App) -> bool {
        let now = self.settings(cx);
        self.original.as_ref().is_some_and(|proxy| {
            proxy.has_password
                && now
                    .as_ref()
                    .is_some_and(|now| same_proxy(&proxy.settings, now))
        })
    }

    fn say_what_happens_to_the_password(&self, window: &mut Window, cx: &mut Context<'_, Self>) {
        let has_saved = self.original.as_ref().is_some_and(|p| p.has_password);
        let said = password_hint(has_saved, self.saved_password, self.keeps_password(cx));
        self.password.update(cx, |state, cx| {
            state.set_placeholder(said, window, cx);
        });
    }

    fn draft(&self, cx: &App) -> Option<ProxyServerDraft> {
        let typed = self.password.read(cx).value().to_string();
        Some(ProxyServerDraft {
            settings: self.settings(cx)?,
            password: if !typed.is_empty() {
                Update::Set(Some(Secret::from(typed)))
            } else if self.saved_password == Password::Remove {
                Update::Set(None)
            } else {
                Update::Keep
            },
        })
    }

    /// The draft to try or save; without one, the form says why.
    fn draft_or_say(&mut self, cx: &mut Context<'_, Self>) -> Option<ProxyServerDraft> {
        let held = self.draft(cx);
        if held.is_none() {
            self.outcome = Some(Err(BAD_PORT.to_owned()));
            cx.notify();
        }
        held
    }

    /// Whether a login was given that the proxy could refuse.
    fn login_entered(&self, cx: &App) -> bool {
        !self.user.read(cx).value().trim().is_empty()
            || !self.password.read(cx).value().is_empty()
            || self.keeps_password(cx) && self.saved_password == Password::Keep
    }

    fn test(&mut self, window: &mut Window, cx: &mut Context<'_, Self>) {
        if self.stage != Stage::Editing {
            return;
        }
        let id = self.original.as_ref().map(|proxy| proxy.id);
        let Some(draft) = self.draft_or_say(cx) else {
            return;
        };
        self.stage = Stage::Testing;
        self.outcome = None;
        cx.notify();
        let tested = Backend::run(cx, move |manager| async move {
            api::test_proxy_server(&manager, id, draft).await
        });
        cx.spawn_in(window, async move |form, cx| {
            let Some(outcome) = said(tested.await) else {
                return;
            };
            crate::closing::may_have_closed(form.update(cx, |form, cx| {
                form.stage = Stage::Editing;
                form.outcome = Some(outcome);
                cx.notify();
            }));
        })
        .detach();
    }

    fn save(&mut self, window: &mut Window, cx: &mut Context<'_, Self>) {
        if self.stage != Stage::Editing {
            return;
        }
        let id = self.original.as_ref().map(|proxy| proxy.id);
        let Some(draft) = self.draft_or_say(cx) else {
            return;
        };
        self.stage = Stage::Saving;
        cx.notify();
        let saved = Backend::run(cx, move |manager| async move {
            match id {
                Some(id) => api::update_proxy_server(&manager, id, draft).map(|()| None),
                None => api::create_proxy_server(&manager, draft).map(Some),
            }
        });
        cx.spawn_in(window, async move |form, cx| {
            let Some(outcome) = said(saved.await) else {
                return;
            };
            crate::closing::may_have_closed(form.update_in(cx, |form, window, cx| {
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
            }));
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
        self.say_what_happens_to_the_password(window, cx);
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
                        .on_click(cx.listener(|form, _, window, cx| {
                            form.saved_password = Password::Remove;
                            form.outcome = None;
                            form.say_what_happens_to_the_password(window, cx);
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

/// A typed port; `None` for anything but a whole number from 1 to 65535.
fn typed_port(typed: &str) -> Option<u16> {
    typed.trim().parse().ok().filter(|port| *port != 0)
}

/// What the empty password field says of a saved password.
fn password_hint(has_saved: bool, saved: Password, keeps: bool) -> &'static str {
    match (has_saved, saved, keeps) {
        (false, ..) | (true, Password::Remove, _) => "",
        (true, Password::Keep, true) => KEEPS_PASSWORD,
        (true, Password::Keep, false) => FORGETS_PASSWORD,
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
    pub(super) fn line(&self) -> Sentence {
        if self.is_empty() {
            return "Not used yet".into();
        }
        let mut said = Sentence::new().text("Used by ");
        if self.app {
            said = said.text("the global setting");
            if !self.following.is_empty() {
                said = said
                    .text(" (")
                    .queues(self.following.iter().map(String::as_str))
                    .text(")");
            }
            if !self.queues.is_empty() {
                said = said.text(" and ");
            }
        }
        said.queues(self.queues.iter().map(String::as_str))
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
    let name = proxy.settings.shown_name();
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
                title: other.settings.shown_name().into(),
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
            .title(crate::ui::dialog::Header::new(format!("Delete {name}?")))
            .w(px(440.))
            .child(
                v_flex()
                    .gap_4()
                    .text_sm()
                    .child(div().text_role(Role::Secondary, cx).child(if in_use {
                        line.clone().text(". They switch to the proxy you pick.")
                    } else {
                        "Nothing uses it.".into()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_queues_using_a_proxy_stand_out() {
        let users = Users {
            app: true,
            following: vec!["Main".to_owned()],
            queues: vec!["Videos".to_owned(), "Music".to_owned()],
        };
        let line = users.line();
        assert_eq!(
            line.said(),
            "Used by the global setting (Main) and Videos, Music"
        );
        assert_eq!(line.marked(), ["Main", "Videos", "Music"]);
        let unused = Users {
            app: false,
            following: Vec::new(),
            queues: Vec::new(),
        };
        assert_eq!(unused.line().said(), "Not used yet");
    }

    #[test]
    fn an_impossible_port_is_refused() {
        assert_eq!(typed_port(" 8080 "), Some(8080));
        for typed in ["70000", "80a", "", "0", "-1", "1.5"] {
            assert_eq!(typed_port(typed), None, "{typed}");
        }
    }

    #[test]
    fn the_password_field_says_what_saving_will_do() {
        assert_eq!(password_hint(false, Password::Keep, false), "");
        assert_eq!(password_hint(true, Password::Keep, true), KEEPS_PASSWORD);
        assert_eq!(password_hint(true, Password::Keep, false), FORGETS_PASSWORD);
        assert_eq!(password_hint(true, Password::Remove, true), "");
    }
}

//! Network (D73, D39): the global proxy, the saved proxy servers, the hosts that skip the proxy,
//! FTP past other proxies, and the sites that may reach the person's own network. Each change
//! applies to downloads that start after it.

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::setting::{SettingField, SettingGroup, SettingItem, SettingPage};
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Icon, IconName, Sizable as _, h_flex, v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, Axis, ElementId, Entity, IntoElement, ParentElement as _, SharedString,
    Styled as _, Window, div,
};
use wirecatch_protocol::app::{AppSettingsPatch, Setting, Update};
use wirecatch_protocol::proxy::{
    MAX_PROXY_SERVERS, ProxyServer, ProxyServerDraft, ProxySettings, ProxyTestResult, ProxyUse,
};
use wirecatch_shell::api;

use super::{Typing, change, current, list_field};
use crate::backend::command;
use crate::library::Library;
use crate::main_window::proxy_dialog::{self, Users};
use crate::text;
use crate::ui::{Role, Typed as _};

const SYSTEM: &str = "system";
const DIRECT: &str = "direct";
const HOSTS_HELP: &str = "One per line. A host covers its subdomains; IP addresses, ranges (10.0.0.0/8) and <local> (names without a dot) work too.";

pub(super) fn page(library: &Entity<Library>, cx: &App) -> SettingPage {
    SettingPage::new("Network")
        .resettable(false)
        .groups([proxy(library, cx), local_network(library)])
}

fn proxy(library: &Entity<Library>, cx: &App) -> SettingGroup {
    let servers = {
        let library = library.clone();
        SettingField::render(move |_, _, cx| servers(&library, cx))
    };
    let exceptions = {
        let library = library.clone();
        SettingField::render(move |_, window, cx| exceptions(&library, window, cx))
    };
    SettingGroup::new()
        .title("Proxy")
        .item(
            SettingItem::new("Global proxy", global_proxy(library, cx))
                .description("Used by every queue without a proxy of its own. Updates and add-ons use the Main queue's proxy.")
                .keywords(["proxy", "socks", "socks5", "http", "firewall", "vpn", "office", "direct", "system"]),
        )
        .item(
            SettingItem::new("Proxy servers", servers)
                .layout(Axis::Vertical)
                .description("Saved proxies and their sign-in details, for the global proxy and any queue.")
                .keywords(["servers", "login", "password", "test connection", "copy from system"]),
        )
        .item(
            SettingItem::new("Hosts that skip the proxy", exceptions)
                .layout(Axis::Vertical)
                .description("Queues using the global list connect to these hosts directly.")
                .keywords(["exceptions", "bypass", "no_proxy", "direct", "intranet", "local"]),
        )
        .item(
            SettingItem::new("FTP links skip the proxy", ftp_bypass(library))
            .description("Only SOCKS5 proxies carry FTP. On, FTP links go direct past other proxies; off, they fail.")
            .keywords(["ftp", "ftps", "direct"]),
        )
}

fn local_network(library: &Entity<Library>) -> SettingGroup {
    let sites = {
        let library = library.clone();
        SettingField::render(move |_, window, cx| {
            list_field(
                "local-sites".into(),
                &current(&library, cx, |s| s.local_sites.clone(), Vec::new()),
                Typing::Lines,
                "wiki.example.com",
                |sites, cx| {
                    change(
                        AppSettingsPatch {
                            local_sites: Some(sites),
                            ..AppSettingsPatch::default()
                        },
                        cx,
                    );
                },
                window,
                cx,
            )
        })
    };
    SettingGroup::new().title("Local network").item(
        SettingItem::new("Local network sites", sites)
            .layout(Axis::Vertical)
            .description("Links from these sites may reach devices on your network, like a NAS or router. IP addresses and one-word names always count as local. One per line.")
            .keywords(["intranet", "nas", "router", "private", "lan", "trusted"]),
    )
}

/// The global proxy: the system's, none, or a saved server.
fn global_proxy(library: &Entity<Library>, cx: &App) -> SettingField<SharedString> {
    let mut options = vec![
        (
            SharedString::from(SYSTEM),
            SharedString::from("System proxy"),
        ),
        (SharedString::from(DIRECT), SharedString::from("No proxy")),
    ];
    options.extend(library.read(cx).proxies.iter().map(|proxy| {
        (
            SharedString::from(proxy.id.0.to_string()),
            SharedString::from(proxy_dialog::shown_name(&proxy.settings)),
        )
    }));
    let (reading, picking) = (library.clone(), library.clone());
    SettingField::dropdown(
        options,
        move |cx| match current(&reading, cx, |s| s.proxy.proxy, ProxyUse::System) {
            ProxyUse::System => SYSTEM.into(),
            ProxyUse::Direct => DIRECT.into(),
            ProxyUse::Server(id) => id.0.to_string().into(),
        },
        move |key, cx| {
            let picked = match key.as_ref() {
                SYSTEM => Some(ProxyUse::System),
                DIRECT => Some(ProxyUse::Direct),
                key => picking
                    .read(cx)
                    .proxies
                    .iter()
                    .find(|proxy| proxy.id.0.to_string() == key)
                    .map(|proxy| ProxyUse::Server(proxy.id)),
            };
            if let Some(picked) = picked {
                change_proxy(&picking, cx, |proxy| proxy.proxy = picked);
            }
        },
    )
}

/// The saved servers, with New proxy and Copy from system above them.
fn servers(library: &Entity<Library>, cx: &mut App) -> AnyElement {
    let read = library.read(cx);
    let full = read.proxies.len() >= MAX_PROXY_SERVERS;
    let rows: Vec<AnyElement> = read
        .proxies
        .iter()
        .map(|proxy| server_row(library, proxy, read, cx))
        .collect();
    let (adding, theme) = (library.clone(), cx.theme());
    v_flex()
        .w_full()
        .gap_2()
        .child(
            h_flex()
                .gap_2()
                .child(
                    Button::new("new-proxy")
                        .small()
                        .primary()
                        .icon(IconName::Plus)
                        .label("New proxy")
                        .disabled(full)
                        .on_click(move |_, window, cx| {
                            proxy_dialog::open(&adding, None, None, window, cx);
                        }),
                )
                .child(
                    Button::new("copy-system-proxy")
                        .small()
                        .outline()
                        .label("Copy from system")
                        .on_click(|_, window, cx| {
                            command(window.window_handle(), cx, |manager| async move {
                                api::copy_system_proxy(&manager).await.map(|_| ())
                            });
                        }),
                ),
        )
        .map(|list| {
            if rows.is_empty() {
                list.child(
                    div()
                        .py_2()
                        .text_role(Role::Secondary, cx)
                        .child("No proxy servers yet. Add one, or copy your system proxy."),
                )
            } else {
                list.child(
                    v_flex()
                        .rounded(theme.radius)
                        .border_1()
                        .border_color(theme.border)
                        .children(rows),
                )
            }
        })
        .into_any_element()
}

fn server_row(
    library: &Entity<Library>,
    proxy: &ProxyServer,
    read: &Library,
    cx: &App,
) -> AnyElement {
    let theme = cx.theme();
    h_flex()
        .px_3()
        .py_2p5()
        .gap_3()
        .border_b_1()
        .border_color(theme.border)
        .child(server_facts(proxy, read, cx))
        .child(server_actions(library, proxy))
        .into_any_element()
}

/// A server's name and type, address and sign-in, last test and users.
fn server_facts(proxy: &ProxyServer, read: &Library, cx: &App) -> gpui_kit::Div {
    let theme = cx.theme();
    let id = proxy.id;
    let name = proxy_dialog::shown_name(&proxy.settings);
    let login = if proxy.settings.user.is_empty() {
        "no sign-in".to_owned()
    } else if proxy.has_password {
        format!("user {}, password saved", proxy.settings.user)
    } else {
        format!("user {}, no password saved", proxy.settings.user)
    };
    let detail = if proxy.settings.name.trim().is_empty() {
        let mut login = login;
        if let Some(first) = login.get_mut(..1) {
            first.make_ascii_uppercase();
        }
        login
    } else {
        format!("{} · {login}", proxy_dialog::address(&proxy.settings))
    };
    let (tested, tone) = match &proxy.last_test {
        None => ("Not tested yet".to_owned(), theme.muted_foreground),
        Some(run) => {
            let (outcome, tone) = match run.result {
                ProxyTestResult::Works => ("Works", theme.success),
                ProxyTestResult::LoginRefused => ("Sign-in refused", theme.danger),
                ProxyTestResult::Unreachable { .. } => ("Could not be reached", theme.danger),
                ProxyTestResult::TimedOut => ("Did not answer in time", theme.warning),
            };
            {
                let when = text::date(run.at).replacen("Today", "today", 1).replacen(
                    "Yesterday",
                    "yesterday",
                    1,
                );
                (format!("{outcome} · tested {when}"), tone)
            }
        }
    };
    v_flex()
        .flex_1()
        .min_w_0()
        .gap_0p5()
        .child(
            h_flex()
                .gap_2()
                .child(div().text_role(Role::Strong, cx).child(name.clone()))
                .child(
                    div()
                        .text_role(Role::Meta, cx)
                        .child(proxy_dialog::kind_label(proxy.settings.kind)),
                ),
        )
        .child(div().text_role(Role::Meta, cx).child(detail))
        .child(
            h_flex()
                .gap_2()
                .child(
                    div()
                        .text_role(Role::Meta, cx)
                        .text_color(tone)
                        .child(tested),
                )
                .child(
                    div()
                        .text_role(Role::Meta, cx)
                        .child(Users::of(id, read).line()),
                ),
        )
}

/// Test, Edit and Delete for one server.
fn server_actions(library: &Entity<Library>, proxy: &ProxyServer) -> gpui_kit::Div {
    let id = proxy.id;
    let name = proxy_dialog::shown_name(&proxy.settings);
    let (testing, editing, deleting) = (proxy.clone(), library.clone(), library.clone());
    h_flex()
        .gap_1()
        .child(
            Button::new(ElementId::NamedInteger(
                "test-proxy".into(),
                id.0.unsigned_abs(),
            ))
            .small()
            .outline()
            .label("Test")
            .on_click(move |_, window, cx| {
                let draft = ProxyServerDraft {
                    settings: testing.settings.clone(),
                    password: Update::Keep,
                };
                command(window.window_handle(), cx, move |manager| async move {
                    api::test_proxy_server(&manager, Some(id), draft)
                        .await
                        .map(|_| ())
                });
            }),
        )
        .child(
            Button::new(ElementId::NamedInteger(
                "edit-proxy".into(),
                id.0.unsigned_abs(),
            ))
            .small()
            .ghost()
            .icon(Icon::new(Lucide::Pencil))
            .tooltip(format!("Edit {name}"))
            .on_click(move |_, window, cx| {
                proxy_dialog::open(&editing, Some(id), None, window, cx);
            }),
        )
        .child(
            Button::new(ElementId::NamedInteger(
                "delete-proxy".into(),
                id.0.unsigned_abs(),
            ))
            .small()
            .ghost()
            .icon(Icon::new(Lucide::Trash))
            .tooltip(format!("Delete {name}"))
            .on_click(move |_, window, cx| {
                proxy_dialog::ask_delete(&deleting, id, window, cx);
            }),
        )
}

/// The global exceptions, while a proxy is in use that they apply to (D73.12).
fn exceptions(library: &Entity<Library>, window: &mut Window, cx: &mut App) -> AnyElement {
    let read = library.read(cx);
    let Some(settings) = read.settings.as_ref() else {
        return div().into_any_element();
    };
    let uses_proxy = |proxy: &ProxyUse| *proxy != ProxyUse::Direct;
    let matter = uses_proxy(&settings.proxy.proxy)
        || read.queues.iter().any(|queue| {
            matches!(queue.settings.exceptions, Setting::Global)
                && queue.settings.proxy.own_ref().is_some_and(uses_proxy)
        });
    if !matter {
        return div()
            .text_role(Role::Secondary, cx)
            .child("No proxy is in use, so there is nothing to bypass.")
            .into_any_element();
    }
    let exceptions = settings.proxy.exceptions.clone();
    let (system, hosts_on, hosts) = (library.clone(), library.clone(), library.clone());
    v_flex()
        .w_full()
        .gap_3()
        .child(
            Checkbox::new("system-exceptions")
                .label("System proxy exceptions")
                .checked(exceptions.system)
                .on_click(move |on, _, cx| {
                    let on = *on;
                    change_proxy(&system, cx, |proxy| proxy.exceptions.system = on);
                }),
        )
        .child(
            Checkbox::new("global-hosts")
                .label("Global hosts")
                .checked(exceptions.use_hosts)
                .on_click(move |on, _, cx| {
                    let on = *on;
                    change_proxy(&hosts_on, cx, |proxy| proxy.exceptions.use_hosts = on);
                }),
        )
        .when(exceptions.use_hosts, |list| {
            list.child(
                v_flex()
                    .pl_6()
                    .gap_1p5()
                    .child(list_field(
                        "global-hosts-list".into(),
                        &exceptions.hosts,
                        Typing::Lines,
                        "intranet.example.com",
                        move |listed, cx| {
                            change_proxy(&hosts, cx, |proxy| proxy.exceptions.hosts = listed);
                        },
                        window,
                        cx,
                    ))
                    .child(div().text_role(Role::Meta, cx).child(HOSTS_HELP)),
            )
        })
        .into_any_element()
}

/// FTP past proxies that cannot carry it; changes only that part of the proxy settings.
fn ftp_bypass(library: &Entity<Library>) -> SettingField<bool> {
    let (reading, changing) = (library.clone(), library.clone());
    SettingField::switch(
        move |cx| current(&reading, cx, |s| s.proxy.ftp_bypass_proxy, false),
        move |on, cx| change_proxy(&changing, cx, |proxy| proxy.ftp_bypass_proxy = on),
    )
}

/// Sends the app's proxy settings with `edit` made to them.
fn change_proxy(library: &Entity<Library>, cx: &mut App, edit: impl FnOnce(&mut ProxySettings)) {
    let Some(mut proxy) = library.read(cx).settings.as_ref().map(|s| s.proxy.clone()) else {
        return;
    };
    edit(&mut proxy);
    change(
        AppSettingsPatch {
            proxy: Some(proxy),
            ..AppSettingsPatch::default()
        },
        cx,
    );
}

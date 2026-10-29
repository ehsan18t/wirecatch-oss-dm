use wirecatch_protocol::ProxyServerId;
use wirecatch_protocol::proxy::AppExceptions;

use super::*;

fn profile(kind: ProxyKind, host: &str, port: u16) -> ProxyServerSettings {
    ProxyServerSettings {
        kind,
        host: host.into(),
        port,
        ..ProxyServerSettings::default()
    }
}

fn saved(id: i64, settings: ProxyServerSettings) -> SavedProxyServer {
    SavedProxyServer {
        id: ProxyServerId(id),
        settings,
        last_test: None,
    }
}

fn manual_proxies(choice: ProxyChoice) -> ManualProxies {
    match choice {
        ProxyChoice::Manual(manual) => manual,
        other => panic!("expected manual proxies, got {other:?}"),
    }
}

/// No computer's list, so a test never reads this machine's.
fn only(hosts: &[&str]) -> Exceptions {
    Exceptions {
        system: false,
        hosts: hosts.iter().map(ToString::to_string).collect(),
    }
}

#[test]
fn a_profile_needs_a_server_name_and_port() {
    for (host, port) in [
        ("", 8080),
        ("http://proxy", 8080),
        ("proxy:8080", 8080),
        ("pro xy", 8080),
        ("user@proxy", 8080),
        ("proxy", 0),
        ("999.1.1.1", 8080),
        ("proxy.10", 8080),
    ] {
        assert!(
            validate_settings_file(profile(ProxyKind::Http, host, port), &[]).is_err(),
            "{host}:{port}"
        );
    }
    for host in ["proxy.corp", "10.0.0.5", "::1", " proxy "] {
        let kept = validate_settings_file(profile(ProxyKind::Http, host, 3128), &[]).unwrap();
        assert_eq!(kept.host, host.trim());
    }
}

#[test]
fn profile_names_are_unique_and_an_empty_one_shows_the_address() {
    let office = ProxyServerSettings {
        name: "Office".into(),
        ..profile(ProxyKind::Http, "proxy.corp", 3128)
    };
    let unnamed = profile(ProxyKind::Http, "other.corp", 8080);
    assert_eq!(unnamed.shown_name(), "other.corp:8080");
    assert_eq!(profile(ProxyKind::Http, "::1", 1).shown_name(), "[::1]:1");
    let others = [&office, &unnamed];
    let same_name = ProxyServerSettings {
        name: " office ".into(),
        ..profile(ProxyKind::Socks5, "socks", 1080)
    };
    assert!(validate_settings_file(same_name, &others).is_err());
    assert!(validate_settings_file(unnamed.clone(), &others).is_err());
    let named = ProxyServerSettings {
        name: "Home".into(),
        ..unnamed.clone()
    };
    assert!(validate_settings_file(named, &others).is_ok());
    let long = ProxyServerSettings {
        name: "n".repeat(MAX_PROXY_NAME_CHARS + 1),
        ..profile(ProxyKind::Http, "proxy", 1)
    };
    assert!(validate_settings_file(long, &[]).is_err());
}

#[test]
fn exceptions_are_cleaned_and_bounded() {
    let mut proxy = ProxySettings::default();
    proxy.exceptions.hosts = vec![
        " Files.Example ".into(),
        "files.example".into(),
        String::new(),
        "*.corp".into(),
        "<local>".into(),
    ];
    let kept = validate(proxy.clone()).unwrap();
    assert_eq!(
        kept.exceptions.hosts,
        ["files.example", "*.corp", "<local>"]
    );

    proxy.exceptions.hosts = vec!["a b".into()];
    assert!(validate(proxy.clone()).is_err());
    proxy.exceptions.hosts = (0..=MAX_PROXY_EXCEPTIONS)
        .map(|n| format!("h{n}.example"))
        .collect();
    assert!(validate(proxy).is_err());

    let own = Setting::Own(QueueExceptions {
        hosts: vec!["A.example".into(), "a.example".into()],
        ..QueueExceptions::default()
    });
    let Setting::Own(kept) = validate_queue_exceptions(own).unwrap() else {
        panic!("a queue's own exceptions stay its own");
    };
    assert_eq!(kept.hosts, ["a.example"]);
}

#[test]
fn a_queue_follows_the_apps_exceptions_or_picks_its_lists() {
    let app = ProxySettings {
        exceptions: AppExceptions {
            system: true,
            use_hosts: true,
            hosts: vec!["app.example".into()],
        },
        ..ProxySettings::default()
    };
    assert_eq!(
        exceptions(&app, &Setting::Global),
        Exceptions {
            system: true,
            hosts: vec!["app.example".into()],
        }
    );
    let own = |app_hosts, use_hosts| {
        Setting::Own(QueueExceptions {
            system: false,
            app_hosts,
            use_hosts,
            hosts: vec!["queue.example".into()],
        })
    };
    assert_eq!(
        exceptions(&app, &own(true, true)).hosts,
        ["app.example", "queue.example"]
    );
    assert_eq!(exceptions(&app, &own(false, true)).hosts, ["queue.example"]);
    assert_eq!(
        exceptions(&app, &own(false, false)).hosts,
        Vec::<String>::new()
    );
    assert!(!exceptions(&app, &own(true, true)).system);

    let mut unticked = app.clone();
    unticked.exceptions.use_hosts = false;
    assert_eq!(
        exceptions(&unticked, &Setting::Global).hosts,
        Vec::<String>::new()
    );
    assert_eq!(
        exceptions(&unticked, &own(true, true)).hosts,
        ["queue.example"]
    );
}

#[test]
fn passwords_are_kept_replaced_or_removed() {
    let was = ProxyServerSettings {
        user: "alice".into(),
        ..profile(ProxyKind::Http, "proxy", 3128)
    };
    let old = Secret::new("old");
    let keep = Update::Keep;
    assert_eq!(
        merge_password(Some(&old), &keep, Some(&was), &was).unwrap(),
        Some(old.clone())
    );
    let new = Update::Set(Some(Secret::new("new")));
    assert_eq!(
        merge_password(Some(&old), &new, Some(&was), &was).unwrap(),
        Some(Secret::new("new"))
    );
    for removed in [Update::Set(None), Update::Set(Some(Secret::new("")))] {
        assert_eq!(
            merge_password(Some(&old), &removed, Some(&was), &was).unwrap(),
            None
        );
    }
    let broken = Update::Set(Some(Secret::new("a\nb")));
    assert!(merge_password(Some(&old), &broken, Some(&was), &was).is_err());
    let nobody = profile(ProxyKind::Http, "proxy", 3128);
    assert!(merge_password(None, &new, None, &nobody).is_err());
}

#[test]
fn a_kept_password_never_goes_to_another_proxy() {
    let was = ProxyServerSettings {
        name: "Office".into(),
        user: "alice".into(),
        ..profile(ProxyKind::Http, "proxy.corp", 3128)
    };
    let saved = Secret::new("secret");
    let keep = Update::Keep;
    let mut renamed = was.clone();
    renamed.name = "Work".into();
    renamed.host = "PROXY.corp".into();
    assert_eq!(
        merge_password(Some(&saved), &keep, Some(&was), &renamed).unwrap(),
        Some(saved.clone())
    );
    for change in [
        |p: &mut ProxyServerSettings| p.host = "elsewhere.example".into(),
        |p: &mut ProxyServerSettings| p.port = 8080,
        |p: &mut ProxyServerSettings| p.kind = ProxyKind::Https,
        |p: &mut ProxyServerSettings| p.user = "bob".into(),
    ] {
        let mut now = was.clone();
        change(&mut now);
        assert_eq!(
            merge_password(Some(&saved), &keep, Some(&was), &now).unwrap(),
            None
        );
    }
    assert_eq!(
        merge_password(Some(&saved), &keep, None, &was).unwrap(),
        None
    );
}

#[test]
fn a_profile_reaches_the_engine_as_a_url_with_its_login_and_exceptions() {
    let socks = ProxyServerSettings {
        user: "al ice".into(),
        ..profile(ProxyKind::Socks5, "::1", 1080)
    };
    let profiles = [saved(2, socks)];
    let logins = Logins::from([(ProxyServerId(2), Secret::new("p@ss:word"))]);
    let manual = manual_proxies(engine_choice(
        ProxyUse::Server(ProxyServerId(2)),
        &only(&["files.example", "10.*"]),
        true,
        &profiles,
        &logins,
    ));
    assert_eq!(
        manual.all.as_deref(),
        Some("socks5h://al%20ice:p%40ss%3Aword@[::1]:1080")
    );
    assert_eq!(manual.exceptions, "files.example,10.*");
    assert!(manual.ftp_direct);
    assert_eq!(manual.http, None);
    assert_eq!(
        profile_url(&profile(ProxyKind::Https, "secure", 443), None).as_deref(),
        Some("https://secure/")
    );
}

#[test]
fn a_profile_that_is_gone_falls_back_to_the_computers_proxy() {
    assert_eq!(
        engine_choice(
            ProxyUse::Server(ProxyServerId(9)),
            &only(&[]),
            false,
            &[],
            &Logins::new()
        ),
        ProxyChoice::System
    );
    assert!(check_use(ProxyUse::Server(ProxyServerId(9)), &[]).is_err());
    assert!(check_use(ProxyUse::System, &[]).is_ok());
}

#[test]
fn the_computers_proxy_is_left_to_the_engine_unless_something_is_added() {
    let plain = Exceptions {
        system: true,
        hosts: Vec::new(),
    };
    let none = Logins::new();
    assert_eq!(
        engine_choice(ProxyUse::System, &plain, false, &[], &none),
        ProxyChoice::System
    );
    assert_eq!(
        engine_choice(ProxyUse::Direct, &plain, true, &[], &none),
        ProxyChoice::Direct
    );
    assert_ne!(
        engine_choice(ProxyUse::System, &only(&["a.example"]), false, &[], &none),
        ProxyChoice::System
    );
}

#[test]
fn yt_dlp_and_the_updater_take_the_proxy_for_https() {
    let mut manual = ManualProxies::default();
    manual.http = Some("http://web:80".into());
    manual.all = Some("socks5h://socks:1080".into());
    let choice = ProxyChoice::Manual(manual.clone());
    assert_eq!(
        ytdlp_proxy(&choice).as_deref(),
        Some("socks5h://socks:1080")
    );
    assert_eq!(
        updater_proxy(&choice),
        UpdaterProxy::Url(Url::parse("socks5h://socks:1080").unwrap())
    );
    manual.https = Some("http://secure:3128".into());
    let choice = ProxyChoice::Manual(manual);
    assert_eq!(ytdlp_proxy(&choice).as_deref(), Some("http://secure:3128"));

    assert_eq!(ytdlp_proxy(&ProxyChoice::Direct).as_deref(), Some(""));
    assert_eq!(ytdlp_proxy(&ProxyChoice::System), None);
    assert_eq!(updater_proxy(&ProxyChoice::Direct), UpdaterProxy::Direct);
    assert_eq!(updater_proxy(&ProxyChoice::System), UpdaterProxy::System);
}

#[test]
fn a_proxy_the_computer_sets_reads_as_a_profile_without_its_password() {
    let read = parse_server("http://al%20ice:secret@proxy.corp:3128").unwrap();
    assert_eq!(
        read,
        ProxyServerSettings {
            user: "al ice".into(),
            ..profile(ProxyKind::Http, "proxy.corp", 3128)
        }
    );
    assert_eq!(parse_server("proxy:8080").unwrap().kind, ProxyKind::Http);
    assert_eq!(
        parse_server("socks4://s:1080").unwrap().kind,
        ProxyKind::Socks5
    );
    assert_eq!(
        parse_server("socks5://[::1]").unwrap(),
        profile(ProxyKind::Socks5, "::1", 1080)
    );
}

#[test]
fn a_copy_of_the_computers_proxy_skips_saved_servers_and_takes_a_free_name() {
    let saved_server = ProxyServerSettings {
        user: "alice".into(),
        ..profile(ProxyKind::Http, "proxy.corp", 3128)
    };
    let saved_profiles = [saved(1, saved_server.clone())];
    let found = vec![
        saved_server,
        // The same address without the login: another proxy whose address is a saved name.
        profile(ProxyKind::Http, "proxy.corp", 3128),
        profile(ProxyKind::Socks5, "socks.corp", 1080),
    ];
    let copied = copies(found, &saved_profiles).unwrap();
    let names: Vec<_> = copied.iter().map(ProxyServerSettings::shown_name).collect();
    assert_eq!(names, ["proxy.corp:3128 (2)", "socks.corp:1080"]);

    let full: Vec<_> = (0..MAX_PROXY_SERVERS)
        .map(|n| {
            saved(
                i64::try_from(n).unwrap(),
                profile(ProxyKind::Http, &format!("p{n}"), 1),
            )
        })
        .collect();
    assert!(copies(vec![profile(ProxyKind::Http, "new", 1)], &full).is_err());
}

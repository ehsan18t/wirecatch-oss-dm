use wirecatch_test_support::http::{Config, TestServer};
use wirecatch_test_support::proxy::{LoginWall, SocksConfig, SocksServer};
use wirecatch_test_support::silent::SilentServer;

use super::*;

fn server(kind: ProxyKind, host: &str, port: u16) -> ProxyServer {
    ProxyServer {
        kind,
        host: host.into(),
        port,
        ..ProxyServer::default()
    }
}

/// A manual setting with one proxy for every protocol.
fn manual(all: ProxyServer) -> ProxySettings {
    ProxySettings {
        mode: ProxyMode::Manual,
        all,
        ..ProxySettings::default()
    }
}

fn manual_proxies(choice: ProxyChoice) -> ManualProxies {
    match choice {
        ProxyChoice::Manual(manual) => manual,
        other => panic!("expected manual proxies, got {other:?}"),
    }
}

#[test]
fn a_manual_setting_needs_a_server_and_each_server_a_name_and_port() {
    let nothing = ProxySettings {
        mode: ProxyMode::Manual,
        ..ProxySettings::default()
    };
    assert!(validate(nothing).is_err());
    for (host, port) in [
        ("http://proxy", 8080),
        ("proxy:8080", 8080),
        ("pro xy", 8080),
        ("user@proxy", 8080),
        ("proxy", 0),
    ] {
        assert!(
            validate(manual(server(ProxyKind::Http, host, port))).is_err(),
            "{host}:{port}"
        );
    }
    for host in ["proxy.corp", "10.0.0.5", "::1", " proxy "] {
        let kept = validate(manual(server(ProxyKind::Http, host, 3128))).unwrap();
        assert_eq!(kept.all.host, host.trim());
    }
    // System and no proxy need nothing entered.
    assert!(validate(ProxySettings::default()).is_ok());
}

#[test]
fn exceptions_are_cleaned_and_bounded() {
    let mut proxy = manual(server(ProxyKind::Http, "proxy", 3128));
    proxy.exceptions = vec![
        " Files.Example ".into(),
        "files.example".into(),
        String::new(),
        "*.corp".into(),
        "<local>".into(),
    ];
    let kept = validate(proxy.clone()).unwrap();
    assert_eq!(kept.exceptions, ["files.example", "*.corp", "<local>"]);

    proxy.exceptions = vec!["a b".into()];
    assert!(validate(proxy.clone()).is_err());
    proxy.exceptions = (0..=MAX_PROXY_EXCEPTIONS)
        .map(|n| format!("h{n}.example"))
        .collect();
    assert!(validate(proxy).is_err());
}

#[test]
fn passwords_are_kept_replaced_or_removed() {
    let saved = Logins::from([(ProxyRole::All, Secret::new("old"))]);
    let mut same = manual(ProxyServer {
        user: "alice".into(),
        ..server(ProxyKind::Http, "proxy", 3128)
    });
    same.socks = ProxyServer {
        user: "bob".into(),
        ..server(ProxyKind::Socks5, "socks", 1080)
    };
    let keep = merge_passwords(&saved, &ProxyPasswords::default(), &same, &same).unwrap();
    assert_eq!(keep, saved);

    let changed = merge_passwords(
        &saved,
        &ProxyPasswords {
            all: Update::Set(None),
            socks: Update::Set(Some(Secret::new("new"))),
            ..ProxyPasswords::default()
        },
        &same,
        &same,
    )
    .unwrap();
    assert_eq!(
        changed,
        Logins::from([(ProxyRole::Socks, Secret::new("new"))])
    );

    let empty = ProxyPasswords {
        all: Update::Set(Some(Secret::new(""))),
        ..ProxyPasswords::default()
    };
    assert_eq!(
        merge_passwords(&saved, &empty, &same, &same).unwrap(),
        Logins::new()
    );

    let broken = ProxyPasswords {
        all: Update::Set(Some(Secret::new("a\nb"))),
        ..ProxyPasswords::default()
    };
    assert!(merge_passwords(&saved, &broken, &same, &same).is_err());
}

#[test]
fn a_kept_password_never_goes_to_another_proxy() {
    let saved = Logins::from([(ProxyRole::All, Secret::new("secret"))]);
    let was = manual(ProxyServer {
        user: "alice".into(),
        ..server(ProxyKind::Http, "proxy.corp", 3128)
    });
    let keep = ProxyPasswords::default();
    let mut now = was.clone();
    now.all.host = " PROXY.corp ".into();
    assert_eq!(merge_passwords(&saved, &keep, &was, &now).unwrap(), saved);
    for change in [
        |p: &mut ProxySettings| p.all.host = "elsewhere.example".into(),
        |p: &mut ProxySettings| p.all.port = 8080,
        |p: &mut ProxySettings| p.all.kind = ProxyKind::Https,
        |p: &mut ProxySettings| p.all.user = "bob".into(),
    ] {
        let mut now = was.clone();
        change(&mut now);
        assert_eq!(
            merge_passwords(&saved, &keep, &was, &now).unwrap(),
            Logins::new()
        );
    }
}

#[test]
fn the_engine_gets_each_proxy_as_a_url_with_its_login() {
    let mut proxy = manual(ProxyServer {
        user: "al ice".into(),
        ..server(ProxyKind::Socks5, "::1", 1080)
    });
    proxy.exceptions = vec!["files.example".into(), "10.*".into()];
    proxy.ftp_direct = true;
    let logins = Logins::from([(ProxyRole::All, Secret::new("p@ss:word"))]);
    let manual = manual_proxies(engine_choice(&proxy, &logins));
    assert_eq!(
        manual.all.as_deref(),
        Some("socks5h://al%20ice:p%40ss%3Aword@[::1]:1080")
    );
    assert_eq!(manual.exceptions, "files.example,10.*");
    assert!(manual.ftp_direct);
    assert_eq!(manual.http, None);
}

#[test]
fn per_protocol_proxies_go_to_their_protocols_and_socks_to_every_other() {
    let proxy = ProxySettings {
        mode: ProxyMode::Manual,
        per_protocol: true,
        // The one for every protocol is kept but not used.
        all: server(ProxyKind::Http, "unused", 1),
        http: server(ProxyKind::Http, "web", 80),
        https: server(ProxyKind::Https, "secure", 443),
        // The SOCKS proxy is SOCKS5 whatever its kind says.
        socks: server(ProxyKind::Http, "socks", 1080),
        ..ProxySettings::default()
    };
    let manual = manual_proxies(engine_choice(&proxy, &Logins::new()));
    assert_eq!(manual.http.as_deref(), Some("http://web/"));
    assert_eq!(manual.https.as_deref(), Some("https://secure/"));
    assert_eq!(manual.ftp, None);
    assert_eq!(manual.all.as_deref(), Some("socks5h://socks:1080"));
}

#[test]
fn system_and_no_proxy_pass_through() {
    let direct = ProxySettings {
        mode: ProxyMode::Direct,
        ..manual(server(ProxyKind::Http, "proxy", 3128))
    };
    assert_eq!(engine_choice(&direct, &Logins::new()), ProxyChoice::Direct);
    assert_eq!(
        engine_choice(&ProxySettings::default(), &Logins::new()),
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
fn a_proxy_the_computer_sets_reads_as_a_server_without_its_password() {
    let read = parse_server("http://al%20ice:secret@proxy.corp:3128").unwrap();
    assert_eq!(
        read,
        ProxyServer {
            user: "al ice".into(),
            ..server(ProxyKind::Http, "proxy.corp", 3128)
        }
    );
    assert_eq!(parse_server("proxy:8080").unwrap().kind, ProxyKind::Http);
    assert_eq!(
        parse_server("socks4://s:1080").unwrap().kind,
        ProxyKind::Socks5
    );
    assert_eq!(
        parse_server("socks5://[::1]").unwrap(),
        server(ProxyKind::Socks5, "::1", 1080)
    );
}

#[tokio::test]
async fn the_test_says_whether_the_proxy_works() {
    let site = TestServer::start(b"hello".to_vec(), Config::default()).await;
    let url = site.url("/");
    let through = |proxy: &str| {
        let mut manual = ManualProxies::default();
        manual.all = Some(proxy.to_string());
        ProxyChoice::Manual(manual)
    };

    let socks = SocksServer::start(SocksConfig::default()).await;
    let works = test(
        through(&socks.url("socks5", "")),
        &url,
        CancellationToken::new(),
    )
    .await;
    assert_eq!(works, ProxyTestResult::Works);
    assert_ne!(socks.destinations(), Vec::<String>::new());

    let wall = LoginWall::start().await;
    let refused = test(through(&wall.url()), &url, CancellationToken::new()).await;
    assert_eq!(refused, ProxyTestResult::LoginRefused);

    let closed = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .unwrap()
        .local_addr()
        .unwrap();
    let missing = test(
        through(&format!("http://{closed}")),
        &url,
        CancellationToken::new(),
    )
    .await;
    assert!(
        matches!(missing, ProxyTestResult::Unreachable { .. }),
        "{missing:?}"
    );
}

#[tokio::test(start_paused = true)]
async fn a_proxy_that_never_answers_times_out() {
    let silent = SilentServer::start().await;
    let mut manual = ManualProxies::default();
    let address = silent.url("/");
    manual.all = Some(format!(
        "http://{}:{}",
        address.host_str().unwrap(),
        address.port().unwrap()
    ));
    let url = Url::parse("http://files.example/").unwrap();
    let result = test(ProxyChoice::Manual(manual), &url, CancellationToken::new()).await;
    assert_eq!(result, ProxyTestResult::TimedOut);
}

#[test]
fn a_server_name_no_url_can_hold_is_refused_rather_than_dropped() {
    for host in ["999.1.1.1", "proxy.10", "1.2.3.4.5"] {
        assert!(
            validate(manual(server(ProxyKind::Http, host, 8080))).is_err(),
            "{host}"
        );
    }
}

#[test]
fn a_password_needs_a_user_name() {
    let proxy = manual(server(ProxyKind::Socks5, "proxy", 1080));
    let passwords = ProxyPasswords {
        all: Update::Set(Some(Secret::new("secret"))),
        ..ProxyPasswords::default()
    };
    assert!(merge_passwords(&Logins::new(), &passwords, &proxy, &proxy).is_err());
    let with_user = manual(ProxyServer {
        user: "alice".into(),
        ..server(ProxyKind::Socks5, "proxy", 1080)
    });
    assert!(merge_passwords(&Logins::new(), &passwords, &with_user, &with_user).is_ok());
}

#[tokio::test]
async fn the_test_goes_through_every_proxy_set_whatever_the_exceptions() {
    let site = TestServer::start(b"hello".to_vec(), Config::default()).await;
    let url = site.url("/");
    let wall = LoginWall::start().await;
    let socks = SocksServer::start(SocksConfig::default()).await;

    // The exceptions name the test's own host: without them the proxy would not be tried.
    let mut excepted = ManualProxies::default();
    excepted.all = Some(wall.url());
    excepted.exceptions = "127.0.0.1".into();
    let result = test(
        ProxyChoice::Manual(excepted),
        &url,
        CancellationToken::new(),
    )
    .await;
    assert_eq!(result, ProxyTestResult::LoginRefused);

    // A proxy only for another protocol is tried too.
    let mut per_protocol = ManualProxies::default();
    per_protocol.https = Some(socks.url("socks5", ""));
    per_protocol.ftp = Some(wall.url());
    let result = test(
        ProxyChoice::Manual(per_protocol),
        &url,
        CancellationToken::new(),
    )
    .await;
    assert_eq!(result, ProxyTestResult::LoginRefused);
}

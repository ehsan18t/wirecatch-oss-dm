use super::*;

/// An environment holding only `values`.
fn env(values: &'static [(&'static str, &'static str)]) -> impl Fn(&str) -> Option<OsString> {
    move |name: &str| {
        values
            .iter()
            .find(|(n, _)| *n == name)
            .map(|(_, v)| OsString::from(v))
    }
}

/// The settings `values` give, with the system's proxy turned off.
fn from_env(values: &'static [(&'static str, &'static str)]) -> Proxies {
    Proxies::read(&env(values), || None)
}

/// The settings with no variable set and the system proxy `servers` with `exceptions`.
fn from_system(servers: &str, exceptions: &str) -> Proxies {
    let system = SystemProxy {
        servers: servers.to_string(),
        exceptions: exceptions.to_string(),
    };
    Proxies::read(&env(&[]), || Some(system))
}

/// Where the proxy for a request with `scheme` to `host` is set, or `None` when it goes directly.
fn setting(proxies: &Proxies, scheme: Scheme, host: &str) -> Option<ProxySetting> {
    proxies
        .proxy_for(scheme, host)
        .map(|proxy| proxy.setting.clone())
}

fn variable(name: &str) -> ProxySetting {
    ProxySetting::Variable(name.to_string())
}

/// The proxy the HTTP client is handed for `url`, as text.
fn client_target(proxies: &Proxies, url: &str) -> Option<String> {
    proxies
        .client_target(&Url::parse(url).unwrap())
        .map(String::from)
}

#[test]
fn each_scheme_reads_its_own_variable_then_all_proxy() {
    let set = from_env(&[
        ("https_proxy", "http://proxy:8080"),
        ("ALL_PROXY", "socks5://all:1080"),
    ]);
    assert_eq!(
        setting(&set, Scheme::Https, "h.example"),
        Some(variable("https_proxy"))
    );
    assert_eq!(
        setting(&set, Scheme::Ftp, "h.example"),
        Some(variable("ALL_PROXY"))
    );
    assert_eq!(
        setting(&set, Scheme::Http, "h.example"),
        Some(variable("ALL_PROXY"))
    );
    // The first of a pair that is set counts, even when empty.
    let empty = from_env(&[("HTTPS_PROXY", ""), ("https_proxy", "http://proxy:8080")]);
    assert_eq!(setting(&empty, Scheme::Https, "h.example"), None);
    assert_eq!(setting(&from_env(&[]), Scheme::Ftp, "h.example"), None);
}

#[test]
fn no_proxy_names_hosts_reached_directly() {
    let set = from_env(&[
        ("ALL_PROXY", "http://proxy:8080"),
        (
            "NO_PROXY",
            "localhost, .internal.example,corp.example,10.0.0.0/8,::1,192.168.1.7,fd00::/8",
        ),
        ("no_proxy", "*"),
    ]);
    for direct in [
        "localhost",
        "LOCALHOST",
        "internal.example",
        "a.internal.example",
        "corp.example",
        "files.corp.example",
        "10.1.2.3",
        "[::1]",
        "192.168.1.7",
        "[fd12::1]",
    ] {
        assert_eq!(setting(&set, Scheme::Ftp, direct), None, "{direct}");
    }
    for proxied in [
        "notcorp.example",
        "example",
        "11.0.0.1",
        "192.168.1.8",
        "[::2]",
        "[fe80::1]",
        "localhost.evil.example",
    ] {
        assert_eq!(
            setting(&set, Scheme::Ftp, proxied),
            Some(variable("ALL_PROXY")),
            "{proxied}"
        );
    }
    // `*` covers every name but no address, as in the HTTP client.
    let star = from_env(&[("HTTPS_PROXY", "http://proxy:8080"), ("no_proxy", "*")]);
    assert_eq!(setting(&star, Scheme::Https, "anything.example"), None);
    assert_eq!(
        setting(&star, Scheme::Https, "8.8.8.8"),
        Some(variable("HTTPS_PROXY"))
    );
    // An address entry never covers a name that ends like it.
    let address = from_env(&[("HTTPS_PROXY", "http://p:1"), ("NO_PROXY", "1.2.3.4")]);
    assert_eq!(
        setting(&address, Scheme::Https, "x.1.2.3.4"),
        Some(variable("HTTPS_PROXY"))
    );
}

/// A proxy value the client cannot read still counts as a proxy the user
/// set: an unrelated `NO_PROXY` must not make FTP or QUIC go around it.
#[test]
fn an_unreadable_proxy_is_not_excused_by_no_proxy() {
    for proxy in ["not a url at all", "http://[bad"] {
        let set = move |name: &str| match name {
            "ALL_PROXY" => Some(OsString::from(proxy)),
            "NO_PROXY" => Some(OsString::from("other.example")),
            _ => None,
        };
        let proxies = Proxies::read(&set, || None);
        for scheme in [Scheme::Ftp, Scheme::Https] {
            assert_eq!(
                setting(&proxies, scheme, "h.example"),
                Some(variable("ALL_PROXY")),
                "{proxy}"
            );
        }
        // The client cannot use it, so it goes directly, as it always did.
        assert_eq!(client_target(&proxies, "https://h.example/"), None);
    }
}

#[test]
fn proxy_host_names_come_from_the_settings() {
    let hosts = from_env(&[
        ("HTTPS_PROXY", "http://user:secret@Proxy.Corp.example:8080"),
        ("http_proxy", "proxy.corp.example:3128"),
        ("ALL_PROXY", "socks5://socks.lan:1080"),
        ("all_proxy", "http://10.0.0.5:8080"),
        ("HTTP_PROXY", "  "),
        ("https_proxy", "http://[::1]:8080"),
        ("FTP_PROXY", "http://ftp-only.lan:21"),
    ])
    .host_names();
    // `http_proxy` is shadowed by the blank `HTTP_PROXY`, so the client never uses it.
    assert_eq!(hosts, ["proxy.corp.example", "socks.lan"]);
    assert_eq!(
        from_env(&[("NO_PROXY", "localhost")]).host_names(),
        [] as [String; 0]
    );
    assert_eq!(
        from_system("http=web.corp:80;ftp=ftp.corp:21", "").host_names(),
        ["web.corp"]
    );
}

/// The system's proxy counts only while no proxy variable is set, for every
/// protocol or per protocol, and its exceptions apply unless `NO_PROXY` is set.
#[test]
fn the_system_proxy_counts_when_no_variable_sets_one() {
    let every = from_system("proxy.corp:8080", "");
    for scheme in [Scheme::Http, Scheme::Https, Scheme::Ftp] {
        assert_eq!(
            setting(&every, scheme, "files.example"),
            Some(ProxySetting::System)
        );
    }
    assert_eq!(
        client_target(&every, "https://files.example/a.iso").as_deref(),
        Some("http://proxy.corp:8080/")
    );

    let per_protocol = from_system("http=web.corp:80; https=secure.corp:443 ;", "");
    assert_eq!(
        client_target(&per_protocol, "http://files.example/").as_deref(),
        Some("http://web.corp/")
    );
    assert_eq!(
        client_target(&per_protocol, "https://files.example/").as_deref(),
        Some("http://secure.corp:443/")
    );
    assert_eq!(setting(&per_protocol, Scheme::Ftp, "files.example"), None);

    // A SOCKS entry serves every protocol without its own, as in the system,
    // so FTP is refused rather than going around it.
    let socks = from_system("https=secure.corp:443;socks=s.corp:1080", "");
    assert_eq!(
        setting(&socks, Scheme::Ftp, "files.example"),
        Some(ProxySetting::System)
    );
    assert_eq!(
        client_target(&socks, "http://files.example/").as_deref(),
        Some("socks4://s.corp:1080")
    );
    assert_eq!(
        client_target(&socks, "https://files.example/").as_deref(),
        Some("http://secure.corp:443/")
    );

    // A variable takes the place of the system's setting, even for another protocol.
    let both = Proxies::read(&env(&[("FTP_PROXY", "http://ftp-proxy:21")]), || {
        Some(SystemProxy {
            servers: "proxy.corp:8080".into(),
            exceptions: String::new(),
        })
    });
    assert_eq!(setting(&both, Scheme::Https, "files.example"), None);
    assert_eq!(
        setting(&both, Scheme::Ftp, "files.example"),
        Some(variable("FTP_PROXY"))
    );

    // NO_PROXY applies to the system's proxy too, in place of its exceptions.
    let no_proxy = Proxies::read(&env(&[("NO_PROXY", "files.example")]), || {
        Some(SystemProxy {
            servers: "proxy.corp:8080".into(),
            exceptions: "<local>".into(),
        })
    });
    assert_eq!(setting(&no_proxy, Scheme::Https, "files.example"), None);
    assert_eq!(
        setting(&no_proxy, Scheme::Https, "intranet"),
        Some(ProxySetting::System)
    );

    assert_eq!(
        from_system("", "").proxy_for(Scheme::Https, "a.example"),
        None
    );
}

#[test]
fn the_system_exceptions_are_read_as_no_proxy() {
    let proxies = from_system(
        "proxy.corp:8080",
        "*.corp.example; 10.*;192.168.1.*;<local>;files.example;*mid*;300.*",
    );
    for direct in [
        "a.corp.example",
        "corp.example",
        "10.20.30.40",
        "192.168.1.9",
        "intranet",
        "files.example",
        "dl.files.example",
    ] {
        assert_eq!(setting(&proxies, Scheme::Https, direct), None, "{direct}");
        assert_eq!(client_target(&proxies, &format!("https://{direct}/")), None);
    }
    for proxied in [
        "corp.example.net",
        "11.0.0.1",
        "192.168.2.1",
        "a.mid.example",
        "[::1]",
    ] {
        assert_eq!(
            setting(&proxies, Scheme::Https, proxied),
            Some(ProxySetting::System),
            "{proxied}"
        );
    }
    assert_eq!(ipv4_wildcard("10.*.*.*").as_deref(), Some("10.0.0.0/8"));
    assert_eq!(ipv4_wildcard("*"), None);
    assert_eq!(ipv4_wildcard("10.*.3.*"), None);
}

/// In a CGI program `HTTP_PROXY` may come from a request's `Proxy:` header,
/// so, as in the HTTP client, no proxy is read at all.
#[test]
fn a_cgi_program_reads_no_proxy() {
    let cgi = Proxies::read(
        &env(&[
            ("REQUEST_METHOD", "GET"),
            ("HTTP_PROXY", "http://attacker:80"),
        ]),
        || {
            Some(SystemProxy {
                servers: "proxy.corp:8080".into(),
                exceptions: String::new(),
            })
        },
    );
    assert_eq!(cgi, Proxies::default());
}

/// The client is handed the first proxy it can read, the scheme's own first:
/// what it did when it read the environment itself.
#[test]
fn the_client_takes_the_proxy_it_can_read() {
    let set = from_env(&[
        ("HTTP_PROXY", "not a url at all"),
        ("HTTPS_PROXY", "secure.corp:3128"),
        ("ALL_PROXY", "http://user:p%40ss@all.corp:8080"),
        ("NO_PROXY", "direct.example"),
    ]);
    assert_eq!(
        client_target(&set, "https://files.example/").as_deref(),
        Some("http://secure.corp:3128/")
    );
    // Its credentials go with it.
    assert_eq!(
        client_target(&set, "http://files.example/").as_deref(),
        Some("http://user:p%40ss@all.corp:8080/")
    );
    assert_eq!(client_target(&set, "https://direct.example/"), None);
    assert_eq!(client_target(&set, "ftp://files.example/"), None);
    // The exceptions apply to the proxy taken in place of the unreadable one.
    assert_eq!(client_target(&set, "http://direct.example/"), None);
    assert_eq!(setting(&set, Scheme::Http, "direct.example"), None);
}

/// For any environment, the client is handed exactly the proxy the client's
/// own matcher picked when it read the environment itself.
#[test]
fn the_client_takes_what_its_own_matcher_took() {
    let environments: &[&[(&str, &str)]] = &[
        &[],
        &[("ALL_PROXY", "http://all:1")],
        &[("HTTP_PROXY", "http://h:2"), ("ALL_PROXY", "all:1")],
        &[
            ("HTTP_PROXY", "not a url"),
            ("ALL_PROXY", "http://all:1"),
            ("NO_PROXY", "direct.example"),
        ],
        &[
            ("HTTPS_PROXY", ""),
            ("https_proxy", "http://lower:3"),
            ("ALL_PROXY", "http://all:1"),
        ],
        &[
            ("https_proxy", "http://u:p%40ss@lower:3"),
            ("NO_PROXY", ".corp.example,10.0.0.0/8"),
        ],
        &[("ALL_PROXY", "socks5://s:1080"), ("no_proxy", "*")],
        &[("HTTPS_PROXY", "http://[bad"), ("HTTP_PROXY", "plain:8080")],
    ];
    let destinations = [
        "http://files.example/",
        "https://files.example:8443/",
        "https://direct.example/",
        "http://a.corp.example/",
        "https://10.1.2.3/",
        "http://[::1]:8080/",
    ];
    for &values in environments {
        let get = |name: &str| first_value(values, name);
        let matcher = Matcher::builder()
            .http(
                get("HTTP_PROXY")
                    .or_else(|| get("http_proxy"))
                    .unwrap_or_default(),
            )
            .https(
                get("HTTPS_PROXY")
                    .or_else(|| get("https_proxy"))
                    .unwrap_or_default(),
            )
            .all(
                get("ALL_PROXY")
                    .or_else(|| get("all_proxy"))
                    .unwrap_or_default(),
            )
            .no(get("NO_PROXY")
                .or_else(|| get("no_proxy"))
                .unwrap_or_default())
            .build();
        let lookup = move |name: &str| first_value(values, name).map(OsString::from);
        let proxies = Proxies::read(&lookup, || None);
        for destination in destinations {
            let expected = matcher
                .intercept(&destination.parse().unwrap())
                .map(|intercept| intercept.uri().authority().unwrap().to_string());
            let taken = proxies
                .client_target(&Url::parse(destination).unwrap())
                .map(|url| {
                    format!(
                        "{}:{}",
                        url.host_str().unwrap(),
                        url.port_or_known_default().unwrap()
                    )
                });
            assert_eq!(taken, expected, "{values:?} {destination}");
        }
    }
}

/// The value of `name` in `values`, if set.
fn first_value(values: &[(&str, &str)], name: &str) -> Option<String> {
    values
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, v)| (*v).to_string())
}

/// A value that is not Unicode counts as not set, as in the HTTP client: the
/// lower-case variable is read in its place.
#[test]
fn a_value_that_is_not_text_counts_as_unset() {
    #[cfg(windows)]
    let garbled = {
        use std::os::windows::ffi::OsStringExt;
        OsString::from_wide(&[0xD800])
    };
    #[cfg(unix)]
    let garbled = {
        use std::os::unix::ffi::OsStringExt;
        OsString::from_vec(vec![0xFF])
    };
    let read = move |name: &str| match name {
        "HTTPS_PROXY" => Some(garbled.clone()),
        "https_proxy" => Some(OsString::from("http://lower:3")),
        _ => None,
    };
    let proxies = Proxies::read(&read, || None);
    assert_eq!(
        setting(&proxies, Scheme::Https, "files.example"),
        Some(variable("https_proxy"))
    );
}

#[test]
fn debug_output_hides_proxy_credentials() {
    let proxies = from_env(&[("ALL_PROXY", "http://user:secret@proxy.corp:8080")]);
    let shown = format!("{proxies:?}");
    assert!(
        !shown.contains("secret") && !shown.contains("user"),
        "{shown}"
    );
    assert!(shown.contains("***@proxy.corp:8080"), "{shown}");
}

#[test]
fn a_setting_says_how_to_go_around_it() {
    let named = ProxySetting::Variable("ALL_PROXY".into());
    assert_eq!(named.to_string(), "the ALL_PROXY environment variable");
    assert_eq!(named.remedy(), "remove ALL_PROXY from the environment");
    assert_eq!(
        ProxySetting::System.to_string(),
        "the system's proxy setting"
    );
    assert!(ProxySetting::System.remedy().contains("exceptions"));
}

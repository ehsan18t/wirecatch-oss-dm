//! Proxy servers (D73): saved with their passwords kept in the app, chosen by the app and by
//! each queue with the hosts they reach directly, never interrupting a running download, kept
//! out of settings exports without their passwords, and tested.

use std::sync::Arc;

use wirecatch_engine::units::KIB;
use wirecatch_protocol::app::{AppSettingsPatch, DownloadStatus, QueueSettings, Setting, Update};
use wirecatch_protocol::proxy::{
    ProxyKind, ProxyServerDraft, ProxyServerSettings, ProxySettings, ProxyTestResult, ProxyUse,
    QueueExceptions,
};
use wirecatch_protocol::{ProxyServerId, Secret};
use wirecatch_test_support::proxy::{SocksConfig, SocksServer};

use crate::common::{Harness, TestResult, fast_server, is_running, throttled_server, wait_until};

/// A profile for `socks`, logged in as `user` when one is given.
fn socks_profile(name: &str, socks: &SocksServer, user: &str) -> ProxyServerSettings {
    ProxyServerSettings {
        name: name.into(),
        kind: ProxyKind::Socks5,
        host: "127.0.0.1".into(),
        port: socks.port,
        user: user.into(),
    }
}

fn draft(settings: ProxyServerSettings, password: Option<&str>) -> ProxyServerDraft {
    ProxyServerDraft {
        settings,
        password: password.map_or(Update::Keep, |p| Update::Set(Some(Secret::new(p)))),
    }
}

async fn socks_with_login() -> SocksServer {
    SocksServer::start(SocksConfig {
        login: Some(("alice".into(), "secret".into())),
    })
    .await
}

fn use_for_the_app(h: &Harness, proxy: ProxyUse) -> TestResult {
    let mut settings = h.manager.get_settings().proxy;
    settings.proxy = proxy;
    h.manager.change_settings(AppSettingsPatch {
        proxy: Some(settings),
        ..AppSettingsPatch::default()
    })?;
    Ok(())
}

fn profile(id: ProxyServerId) -> ProxyUse {
    ProxyUse::Server(id)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_saved_profile_carries_downloads_and_its_password_stays_in_the_app() -> TestResult {
    let server = fast_server(64 * KIB).await?;
    let socks = socks_with_login().await;
    let mut h = Harness::new().await?;
    let id = h.manager.create_proxy_server(draft(
        socks_profile("Office", &socks, "alice"),
        Some("secret"),
    ))?;
    use_for_the_app(&h, profile(id))?;

    let profiles = h.manager.list_proxy_servers();
    assert!(profiles[0].has_password);
    let shown = serde_json::to_string(&(profiles, h.manager.get_settings()))?;
    assert!(
        !shown.contains("secret"),
        "the password reached a window: {shown}"
    );

    let row = h.add(server.url("file.bin")).await?;
    h.wait_status(row.id, DownloadStatus::Completed).await;
    assert!(!socks.destinations().is_empty(), "went through the proxy");

    // After a restart the profile and its password are read from the database.
    h.restart().await?;
    assert!(h.manager.list_proxy_servers()[0].has_password);
    let before = socks.destinations().len();
    let again = h.add(server.url("again.bin")).await?;
    h.wait_status(again.id, DownloadStatus::Completed).await;
    assert!(socks.destinations().len() > before);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_queue_uses_its_own_proxy_and_its_exceptions() -> TestResult {
    let server = fast_server(16 * KIB).await?;
    let socks = SocksServer::start(SocksConfig::default()).await;
    let h = Harness::new().await?;
    let id = h
        .manager
        .create_proxy_server(draft(socks_profile("Office", &socks, ""), None))?;
    use_for_the_app(&h, ProxyUse::Direct)?;
    let proxied = h
        .manager
        .create_queue(QueueSettings {
            proxy: Setting::Own(profile(id)),
            ..QueueSettings::new("Proxied")
        })?
        .id;
    let excepted = h
        .manager
        .create_queue(QueueSettings {
            proxy: Setting::Own(profile(id)),
            exceptions: Setting::Own(QueueExceptions {
                use_hosts: true,
                hosts: vec!["127.0.0.1".into()],
                ..QueueExceptions::default()
            }),
            ..QueueSettings::new("Excepted")
        })?
        .id;

    let main = h.add(server.url("main.bin")).await?;
    h.wait_status(main.id, DownloadStatus::Completed).await;
    let skipped = h
        .add_with(server.url("skip.bin"), |r| r.queue = Some(excepted))
        .await?;
    h.wait_status(skipped.id, DownloadStatus::Completed).await;
    assert_eq!(socks.destinations(), Vec::<String>::new());

    let through = h
        .add_with(server.url("through.bin"), |r| r.queue = Some(proxied))
        .await?;
    h.wait_status(through.id, DownloadStatus::Completed).await;
    assert!(!socks.destinations().is_empty(), "the queue's own proxy");
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_proxy_that_refuses_the_login_fails_the_download_with_the_way_to_fix_it() -> TestResult {
    let server = fast_server(16 * KIB).await?;
    let socks = socks_with_login().await;
    let h = Harness::new().await?;
    let id = h.manager.create_proxy_server(draft(
        socks_profile("Office", &socks, "alice"),
        Some("wrong"),
    ))?;
    use_for_the_app(&h, profile(id))?;
    let row = h.add(server.url("file.bin")).await?;
    let failed = h.wait_status(row.id, DownloadStatus::Failed).await;
    let error = failed.error.unwrap();
    assert_eq!(
        error.fix,
        Some(wirecatch_protocol::app::FixAction::ChangeQueue)
    );
    assert!(
        error.message.contains("user name and password"),
        "{}",
        error.message
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_download_keeps_the_proxy_it_went_through_and_names_one_it_could_not_reach() -> TestResult
{
    let server = fast_server(16 * KIB).await?;
    let socks = SocksServer::start(SocksConfig::default()).await;
    let h = Harness::new().await?;
    let id = h
        .manager
        .create_proxy_server(draft(socks_profile("Office", &socks, ""), None))?;
    use_for_the_app(&h, profile(id))?;
    let row = h.add(server.url("file.bin")).await?;
    let done = h.wait_status(row.id, DownloadStatus::Completed).await;
    let through = format!("Office (127.0.0.1:{})", socks.port);
    assert_eq!(done.proxy.as_deref(), Some(through.as_str()));

    // A port nothing listens on: the proxy cannot be reached.
    let gone = std::net::TcpListener::bind("127.0.0.1:0")?
        .local_addr()?
        .port();
    let mut closed = socks_profile("Office", &socks, "");
    closed.port = gone;
    h.manager.update_proxy_server(id, draft(closed, None))?;
    let row = h.add(server.url("again.bin")).await?;
    let failed = h.wait_status(row.id, DownloadStatus::Failed).await;
    let error = failed.error.unwrap();
    assert_eq!(
        error.fix,
        Some(wirecatch_protocol::app::FixAction::ChangeQueue)
    );
    assert!(
        error.message.contains("Could not reach the proxy Office"),
        "{}",
        error.message
    );

    use_for_the_app(&h, ProxyUse::Direct)?;
    let direct = h.add(server.url("direct.bin")).await?;
    let done = h.wait_status(direct.id, DownloadStatus::Completed).await;
    assert_eq!(done.proxy.as_deref(), Some("No proxy"));
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_running_download_keeps_its_proxy_when_the_profile_changes() -> TestResult {
    let slow = throttled_server(256 * KIB, 32 * KIB).await?;
    let first = SocksServer::start(SocksConfig::default()).await;
    let h = Harness::new().await?;
    let id = h
        .manager
        .create_proxy_server(draft(socks_profile("Office", &first, ""), None))?;
    use_for_the_app(&h, profile(id))?;
    let row = h.add(slow.url("slow.bin")).await?;
    h.wait_for(row.id, "some progress", |r| r.done > 0).await;

    let second = SocksServer::start(SocksConfig::default()).await;
    h.manager
        .update_proxy_server(id, draft(socks_profile("Office", &second, ""), None))?;
    assert!(is_running(&h.row(row.id)));
    let done = h.wait_status(row.id, DownloadStatus::Completed).await;
    assert_eq!(done.done, 256 * KIB);
    assert_eq!(second.destinations(), Vec::<String>::new());
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn deleting_a_profile_gives_what_used_it_the_replacement() -> TestResult {
    let socks = socks_with_login().await;
    let h = Harness::new().await?;
    let office = h.manager.create_proxy_server(draft(
        socks_profile("Office", &socks, "alice"),
        Some("secret"),
    ))?;
    let home = h
        .manager
        .create_proxy_server(draft(socks_profile("Home", &socks, ""), None))?;
    use_for_the_app(&h, profile(office))?;
    let queue = h
        .manager
        .create_queue(QueueSettings {
            proxy: Setting::Own(profile(office)),
            ..QueueSettings::new("Office work")
        })?
        .id;

    assert!(
        h.manager
            .delete_proxy_server(office, profile(office))
            .is_err()
    );
    assert!(
        h.manager
            .delete_proxy_server(office, profile(ProxyServerId(99)))
            .is_err()
    );
    h.manager.delete_proxy_server(office, profile(home))?;

    assert_eq!(h.manager.get_settings().proxy.proxy, profile(home));
    let queues = h.manager.list_queues();
    let moved = queues.iter().find(|q| q.id == queue).unwrap();
    assert_eq!(moved.settings.proxy, Setting::Own(profile(home)));
    let ids: Vec<_> = h
        .manager
        .list_proxy_servers()
        .iter()
        .map(|p| p.id)
        .collect();
    assert_eq!(ids, [home]);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_export_has_no_password_and_an_import_of_another_server_forgets_it() -> TestResult {
    let socks = socks_with_login().await;
    let here = Harness::new().await?;
    let id = here.manager.create_proxy_server(draft(
        socks_profile("Office", &socks, "alice"),
        Some("secret"),
    ))?;
    let file = here.root.path().join("profile.json");
    here.manager.export_settings(&file).await?;
    let text = std::fs::read_to_string(&file)?;
    assert!(!text.contains("secret"), "{text}");

    // The same profile comes back: the saved password still belongs to it.
    here.manager.import_settings(&file).await?;
    assert!(here.manager.list_proxy_servers()[0].has_password);

    // The same name for another server: the password was for the old one.
    let other = SocksServer::start(SocksConfig::default()).await;
    let elsewhere = Harness::new().await?;
    elsewhere
        .manager
        .create_proxy_server(draft(socks_profile("Office", &other, "alice"), None))?;
    let other_file = elsewhere.root.path().join("other.json");
    elsewhere.manager.export_settings(&other_file).await?;
    here.manager.import_settings(&other_file).await?;
    let profiles = here.manager.list_proxy_servers();
    assert_eq!(profiles.len(), 1);
    assert_eq!(
        (profiles[0].id, profiles[0].settings.port),
        (id, other.port)
    );
    assert!(!profiles[0].has_password);
    // Another server is tested again, as if it had been edited.
    wait_until("the imported profile's test", || {
        here.manager.list_proxy_servers()[0].last_test.clone()
    })
    .await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_test_uses_the_draft_and_the_saved_password_and_is_kept_when_saved() -> TestResult {
    let site = fast_server(KIB).await?;
    let url = site.url("ping");
    let tweak = Arc::new(move |config: wirecatch_core::ManagerConfig| {
        config.with_proxy_test_url(url.clone())
    });
    let h = Harness::configured(tweak).await?;
    let socks = socks_with_login().await;
    let settings = socks_profile("Office", &socks, "alice");

    assert_eq!(
        h.manager
            .test_proxy_server(None, draft(settings.clone(), Some("secret")))
            .await?,
        ProxyTestResult::Works
    );
    assert_eq!(
        h.manager
            .test_proxy_server(None, draft(settings.clone(), None))
            .await?,
        ProxyTestResult::LoginRefused
    );
    assert!(
        h.manager.list_proxy_servers().is_empty(),
        "testing saves nothing"
    );

    // A new profile is tested once in the background.
    let id = h
        .manager
        .create_proxy_server(draft(settings.clone(), Some("secret")))?;
    let tested = wait_until("the background test", || {
        h.manager.list_proxy_servers()[0].last_test.clone()
    })
    .await;
    assert_eq!(tested.result, ProxyTestResult::Works);

    // A saved profile's draft keeps its password, and its result is kept as its last test.
    assert_eq!(
        h.manager
            .test_proxy_server(Some(id), draft(settings, None))
            .await?,
        ProxyTestResult::Works
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_profile_or_proxy_that_cannot_work_is_refused_and_nothing_changes() -> TestResult {
    let h = Harness::new().await?;
    let mut bad = socks_profile(
        "Office",
        &SocksServer::start(SocksConfig::default()).await,
        "",
    );
    bad.host = "http://proxy.example".into();
    assert!(h.manager.create_proxy_server(draft(bad, None)).is_err());
    assert_eq!(h.manager.list_proxy_servers(), Vec::new());

    let missing = ProxySettings {
        proxy: profile(ProxyServerId(7)),
        ..ProxySettings::default()
    };
    let refused = h.manager.change_settings(AppSettingsPatch {
        proxy: Some(missing),
        ..AppSettingsPatch::default()
    });
    assert!(refused.is_err());
    assert_eq!(h.manager.get_settings().proxy, ProxySettings::default());
    let queue = QueueSettings {
        proxy: Setting::Own(profile(ProxyServerId(7))),
        ..QueueSettings::new("Nowhere")
    };
    assert!(h.manager.create_queue(queue).is_err());
    Ok(())
}

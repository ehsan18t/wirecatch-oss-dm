//! The app's proxy setting (D71): saved with its passwords kept in the app, followed by
//! downloads, applied to running ones when the person asks, kept out of settings exports,
//! and tested before it is saved.

use std::sync::Arc;

use wirecatch_engine::units::KIB;
use wirecatch_protocol::Secret;
use wirecatch_protocol::app::{DownloadStatus, Update};
use wirecatch_protocol::proxy::{
    ProxyChange, ProxyKind, ProxyMode, ProxyPasswords, ProxyServer, ProxySettings, ProxyTest,
    ProxyTestResult,
};
use wirecatch_test_support::proxy::{SocksConfig, SocksServer};

use crate::common::{Harness, TestResult, fast_server, is_running, throttled_server};

/// A manual setting with `socks` as the one proxy for every protocol, logged in as `alice`.
fn through(socks: &SocksServer) -> ProxySettings {
    ProxySettings {
        mode: ProxyMode::Manual,
        all: ProxyServer {
            kind: ProxyKind::Socks5,
            host: "127.0.0.1".into(),
            port: socks.port,
            user: "alice".into(),
            has_password: false,
        },
        ..ProxySettings::default()
    }
}

fn with_password(password: &str) -> ProxyPasswords {
    ProxyPasswords {
        all: Update::Set(Some(Secret::new(password))),
        ..ProxyPasswords::default()
    }
}

async fn socks_with_login() -> SocksServer {
    SocksServer::start(SocksConfig {
        login: Some(("alice".into(), "secret".into())),
    })
    .await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_saved_proxy_carries_downloads_and_its_password_stays_in_the_app() -> TestResult {
    let server = fast_server(64 * KIB).await?;
    let socks = socks_with_login().await;
    let mut h = Harness::new().await?;
    h.manager.set_proxy(ProxyChange {
        settings: through(&socks),
        passwords: with_password("secret"),
        restart_running: false,
    })?;

    let settings = h.manager.get_settings();
    assert!(settings.proxy.all.has_password);
    let shown = serde_json::to_string(&settings)?;
    assert!(
        !shown.contains("secret"),
        "the password reached a window: {shown}"
    );

    let row = h.add(server.url("file.bin")).await?;
    h.wait_status(row.id, DownloadStatus::Completed).await;
    assert!(!socks.destinations().is_empty(), "went through the proxy");

    // After a restart the password is still there, read from the database.
    h.restart().await?;
    assert!(h.manager.get_settings().proxy.all.has_password);
    let before = socks.destinations().len();
    let again = h.add(server.url("again.bin")).await?;
    h.wait_status(again.id, DownloadStatus::Completed).await;
    assert!(socks.destinations().len() > before);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_proxy_that_refuses_the_login_fails_the_download_with_the_way_to_fix_it() -> TestResult {
    let server = fast_server(16 * KIB).await?;
    let socks = socks_with_login().await;
    let h = Harness::new().await?;
    h.manager.set_proxy(ProxyChange {
        settings: through(&socks),
        passwords: with_password("wrong"),
        restart_running: false,
    })?;
    let row = h.add(server.url("file.bin")).await?;
    let failed = h.wait_status(row.id, DownloadStatus::Failed).await;
    let error = failed.error.unwrap();
    assert_eq!(
        error.fix,
        Some(wirecatch_protocol::app::FixAction::ProxySettings)
    );
    assert!(
        error.message.contains("user name and password"),
        "{}",
        error.message
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn running_downloads_restart_on_the_new_proxy_only_when_asked() -> TestResult {
    let slow = throttled_server(256 * KIB, 32 * KIB).await?;
    let h = Harness::new().await?;
    let row = h.add(slow.url("slow.bin")).await?;
    h.wait_for(row.id, "some progress", |r| r.done > 0).await;

    let left_alone = SocksServer::start(SocksConfig::default()).await;
    let mut settings = through(&left_alone);
    settings.all.user.clear();
    h.manager.set_proxy(ProxyChange {
        settings,
        passwords: ProxyPasswords::default(),
        restart_running: false,
    })?;
    assert!(is_running(&h.row(row.id)));
    assert_eq!(left_alone.destinations(), Vec::<String>::new());

    let restarted = SocksServer::start(SocksConfig::default()).await;
    let mut settings = through(&restarted);
    settings.all.user.clear();
    h.manager.set_proxy(ProxyChange {
        settings,
        passwords: ProxyPasswords::default(),
        restart_running: true,
    })?;
    let done = h.wait_status(row.id, DownloadStatus::Completed).await;
    assert_eq!(done.done, 256 * KIB);
    assert!(
        !restarted.destinations().is_empty(),
        "continued through the new proxy"
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_export_has_no_password_and_an_import_with_another_proxy_forgets_it() -> TestResult {
    let socks = socks_with_login().await;
    let here = Harness::new().await?;
    here.manager.set_proxy(ProxyChange {
        settings: through(&socks),
        passwords: with_password("secret"),
        restart_running: false,
    })?;
    let file = here.root.path().join("profile.json");
    here.manager.export_settings(&file).await?;
    let text = std::fs::read_to_string(&file)?;
    assert!(!text.contains("secret"), "{text}");
    let profile: serde_json::Value = serde_json::from_str(&text)?;
    assert_eq!(
        profile.pointer("/settings/proxy/all/has_password"),
        Some(&serde_json::Value::Bool(false))
    );

    // The same proxy comes back: the saved password still belongs to it.
    here.manager.import_settings(&file).await?;
    assert!(here.manager.get_settings().proxy.all.has_password);

    // Another proxy: the password was for the old one.
    let other = SocksServer::start(SocksConfig::default()).await;
    let elsewhere = Harness::new().await?;
    elsewhere.manager.set_proxy(ProxyChange {
        settings: through(&other),
        passwords: ProxyPasswords::default(),
        restart_running: false,
    })?;
    let other_file = elsewhere.root.path().join("other.json");
    elsewhere.manager.export_settings(&other_file).await?;
    here.manager.import_settings(&other_file).await?;
    let proxy = here.manager.get_settings().proxy;
    assert_eq!(proxy.all.port, other.port);
    assert!(!proxy.all.has_password);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_test_uses_the_unsaved_setting_and_the_saved_password() -> TestResult {
    let site = fast_server(KIB).await?;
    let url = site.url("ping");
    let tweak = Arc::new(move |config: wirecatch_core::ManagerConfig| {
        config.with_proxy_test_url(url.clone())
    });
    let h = Harness::configured(tweak).await?;
    let socks = socks_with_login().await;

    let unsaved = |passwords| ProxyTest {
        settings: through(&socks),
        passwords,
    };
    assert_eq!(
        h.manager
            .test_proxy(unsaved(with_password("secret")))
            .await?,
        ProxyTestResult::Works
    );
    assert_eq!(
        h.manager
            .test_proxy(unsaved(ProxyPasswords::default()))
            .await?,
        ProxyTestResult::LoginRefused
    );
    // Nothing was saved by testing.
    assert_eq!(h.manager.get_settings().proxy, ProxySettings::default());

    h.manager.set_proxy(ProxyChange {
        settings: through(&socks),
        passwords: with_password("secret"),
        restart_running: false,
    })?;
    assert_eq!(
        h.manager
            .test_proxy(unsaved(ProxyPasswords::default()))
            .await?,
        ProxyTestResult::Works
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_proxy_that_cannot_work_is_refused_and_nothing_changes() -> TestResult {
    let h = Harness::new().await?;
    let mut settings = ProxySettings {
        mode: ProxyMode::Manual,
        ..ProxySettings::default()
    };
    settings.all.host = "http://proxy.example".into();
    settings.all.port = 8080;
    let refused = h.manager.set_proxy(ProxyChange {
        settings,
        passwords: ProxyPasswords::default(),
        restart_running: false,
    });
    assert!(refused.is_err());
    assert_eq!(h.manager.get_settings().proxy, ProxySettings::default());
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn replacing_every_setting_leaves_the_proxy_to_its_own_change() -> TestResult {
    let socks = socks_with_login().await;
    let h = Harness::new().await?;
    h.manager.set_proxy(ProxyChange {
        settings: through(&socks),
        passwords: with_password("secret"),
        restart_running: false,
    })?;
    let mut settings = h.manager.get_settings();
    settings.proxy.all.host = "elsewhere.example".into();
    settings.proxy.mode = ProxyMode::Direct;
    h.manager.update_settings(settings)?;
    let proxy = h.manager.get_settings().proxy;
    assert_eq!(proxy, through_saved(&socks));
    Ok(())
}

/// [`through`] as saved with its password.
fn through_saved(socks: &SocksServer) -> ProxySettings {
    let mut saved = through(socks);
    saved.all.has_password = true;
    saved
}

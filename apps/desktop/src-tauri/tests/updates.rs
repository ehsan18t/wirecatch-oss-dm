//! The updater (D19) against a local update server: a `latest.json` announcing version 9.9.9
//! and an artifact signed with a throwaway key (`tests/fixtures/updater`, made with
//! `tauri signer generate` and `tauri signer sign`; the private key was not kept). The app runs
//! on Tauri's mock runtime, at version 0.1.0.

use std::sync::Arc;

use axum::Router;
use axum::routing::get;
use tauri::Manager as _;
use tauri::test::{mock_builder, mock_context, noop_assets};
use wirecatch_desktop_lib::updates::{PendingUpdate, check_and_download};

const ARTIFACT: &[u8] = include_bytes!("fixtures/updater/update.bin");
const SIGNATURE: &str = include_str!("fixtures/updater/update.bin.sig");
const PUBLIC_KEY: &str = include_str!("fixtures/updater/throwaway.key.pub");

fn platform() -> String {
    let os = if cfg!(windows) {
        "windows"
    } else if cfg!(target_os = "macos") {
        "darwin"
    } else {
        "linux"
    };
    format!("{os}-{}", std::env::consts::ARCH)
}

/// Serves `latest.json` for `version`, pointing at the signed artifact or a tampered copy.
async fn update_server(version: &str, tampered: bool) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let artifact = if tampered {
        "tampered.bin"
    } else {
        "update.bin"
    };
    let manifest = serde_json::json!({
        "version": version,
        "notes": "Test update",
        "pub_date": "2026-10-11T00:00:00Z",
        "platforms": {
            platform(): { "signature": SIGNATURE.trim(), "url": format!("{base}/{artifact}") }
        }
    })
    .to_string();
    let manifest = Arc::new(manifest);
    let app = Router::new()
        .route(
            "/latest.json",
            get(move || {
                let manifest = manifest.clone();
                async move { (*manifest).clone() }
            }),
        )
        .route("/update.bin", get(|| async { ARTIFACT.to_vec() }))
        .route(
            "/tampered.bin",
            get(|| async {
                let mut bytes = ARTIFACT.to_vec();
                bytes[0] ^= 1;
                bytes
            }),
        );
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    base
}

fn app_checking(base: &str) -> tauri::App<tauri::test::MockRuntime> {
    let mut context = mock_context(noop_assets());
    context.config_mut().plugins.0.insert(
        "updater".into(),
        serde_json::json!({
            "pubkey": PUBLIC_KEY.trim(),
            "endpoints": [format!("{base}/latest.json")],
            "dangerousInsecureTransportProtocol": true
        }),
    );
    mock_builder()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(PendingUpdate::default())
        .build(context)
        .unwrap()
}

#[test]
fn a_signed_newer_version_is_downloaded_and_kept_for_the_quit() {
    tauri::async_runtime::block_on(async {
        let base = update_server("9.9.9", false).await;
        let app = app_checking(&base);
        let found = check_and_download(app.handle()).await;
        assert_eq!(found, Ok(Some("9.9.9".to_string())));
        assert_eq!(
            app.state::<PendingUpdate>().version().as_deref(),
            Some("9.9.9")
        );
    });
}

#[test]
fn an_artifact_that_does_not_match_its_signature_is_refused() {
    tauri::async_runtime::block_on(async {
        let base = update_server("9.9.9", true).await;
        let app = app_checking(&base);
        let found = check_and_download(app.handle()).await;
        assert!(
            found.is_err(),
            "a tampered update must be refused: {found:?}"
        );
        assert!(app.state::<PendingUpdate>().version().is_none());
    });
}

#[test]
fn no_newer_version_means_nothing_to_do() {
    tauri::async_runtime::block_on(async {
        let base = update_server("0.1.0", false).await;
        let app = app_checking(&base);
        assert_eq!(check_and_download(app.handle()).await, Ok(None));
        assert!(app.state::<PendingUpdate>().version().is_none());
    });
}

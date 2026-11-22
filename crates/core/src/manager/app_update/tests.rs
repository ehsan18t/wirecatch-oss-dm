use super::*;
use crate::ManagerConfig;
use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use ed25519_dalek::{Signer as _, SigningKey};
use sha2::{Digest as _, Sha256};
use wirecatch_settings::folders::KnownFolders;
use wirecatch_test_support::files::FileServer;

const LIST: &str = "/releases";
const INSTALLER: &[u8] = b"a newer installer";

/// What a published release says differently from what is usual.
#[derive(Default)]
struct Odd {
    manifest_version: Option<&'static str>,
    asset_size: Option<usize>,
    page: Option<&'static str>,
}

struct Harness {
    manager: Manager,
    server: FileServer,
    _dir: tempfile::TempDir,
}

fn key() -> SigningKey {
    SigningKey::from_bytes(&[7; 32])
}

impl Harness {
    async fn new() -> Self {
        let server = FileServer::start().await;
        let releases = AppReleases::signed(
            server.url(LIST),
            URL_SAFE_NO_PAD.encode(key().verifying_key().as_bytes()),
        );
        Self::open(server, releases).await
    }

    async fn open(server: FileServer, releases: AppReleases) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let config = ManagerConfig::for_tests(
            dir.path().join("data"),
            KnownFolders::under_home(dir.path().join("home")),
        )
        .with_app_releases(releases);
        let manager = Manager::open(config).await.unwrap();
        Self {
            manager,
            server,
            _dir: dir,
        }
    }

    async fn fetch(&self, download: &UpdateDownload) -> Result<PathBuf> {
        self.manager
            .fetch_app_update(
                download,
                Arc::new(Progress::default()),
                CancellationToken::new(),
            )
            .await
    }

    /// Publishes `tag`, its installer and its manifest signed by `signer`.
    fn publish(&self, tag: &str, prerelease: bool, signer: &SigningKey, installer: &[u8]) {
        self.publish_odd(tag, prerelease, signer, installer, &Odd::default());
    }

    fn publish_odd(
        &self,
        tag: &str,
        prerelease: bool,
        signer: &SigningKey,
        installer: &[u8],
        odd: &Odd,
    ) {
        let version = tag.trim_start_matches('v');
        let (system, arch) = (System::current().unwrap(), Arch::current().unwrap());
        let file = UpdateFile {
            name: format!("Wirecatch-{version}-setup.exe"),
            system,
            arch,
            kind: FileKind::Setup,
            size: INSTALLER.len() as u64,
            sha256: format!("{:x}", Sha256::digest(INSTALLER)),
        };
        let manifest = serde_json::to_vec(&Manifest {
            version: odd.manifest_version.unwrap_or(version).to_owned(),
            files: vec![file.clone()],
        })
        .unwrap();
        let signature = URL_SAFE_NO_PAD.encode(signer.sign(&manifest).to_bytes());
        let path = |name: &str| format!("/{tag}/{name}");
        self.server.put(&path(&file.name), installer.to_vec());
        self.server.put(&path(MANIFEST_FILE), manifest);
        self.server.put(&path(SIGNATURE_FILE), signature);
        let asset = |name: &str, size: usize| {
            serde_json::json!({
                "name": name,
                "browser_download_url": self.server.url(&path(name)).to_string(),
                "size": size,
            })
        };
        let list = serde_json::json!([{
            "tag_name": tag,
            "draft": false,
            "prerelease": prerelease,
            "html_url": odd.page.map_or_else(|| format!("https://example.com/{tag}"), str::to_owned),
            "assets": [
                asset(&file.name, odd.asset_size.unwrap_or(INSTALLER.len())),
                asset(MANIFEST_FILE, 1),
                asset(SIGNATURE_FILE, 1),
            ],
        }]);
        self.server.put(LIST, serde_json::to_vec(&list).unwrap());
    }
}

fn put_github_release(server: &FileServer, assets: &[serde_json::Value]) {
    let list = serde_json::json!([{
        "tag_name": "v0.2.0",
        "html_url": "https://another.example/release",
        "assets": assets,
    }]);
    server.put(LIST, serde_json::to_vec(&list).unwrap());
}

#[tokio::test]
async fn github_mode_installs_by_digest_without_a_manifest_or_external_asset_url() {
    let server = FileServer::start().await;
    let repository = server.url("/owner/repo");
    let releases = AppReleases {
        list: server.url(LIST),
        verification: ReleaseVerification::Github(repository.clone()),
    };
    let h = Harness::open(server, releases).await;
    let kind = if cfg!(windows) {
        FileKind::Setup
    } else {
        FileKind::AppImage
    };
    let version = Version::parse("0.2.0").unwrap();
    let name = github::installer_name(
        &version,
        System::current().unwrap(),
        Arch::current().unwrap(),
        kind,
    )
    .unwrap();
    let asset = serde_json::json!({
        "name": name,
        "state": "uploaded",
        "size": INSTALLER.len(),
        "digest": format!("sha256:{:x}", Sha256::digest(INSTALLER)),
        "browser_download_url": "http://127.0.0.1:1/untrusted",
    });
    let url = repository
        .join(&format!("/owner/repo/releases/download/v0.2.0/{name}"))
        .unwrap();
    h.server.put(url.path(), INSTALLER);
    put_github_release(&h.server, std::slice::from_ref(&asset));
    let update = h
        .manager
        .check_app_update("0.1.0", Some(kind))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(update.page, h.server.url("/owner/repo/releases/tag/v0.2.0"));
    let download = update.download.unwrap();
    assert_eq!(download.url, url);
    let path = h.fetch(&download).await.unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), INSTALLER);

    // A retry must recheck the bytes even though the replacement is the same size.
    std::fs::remove_file(&path).unwrap();
    h.server.put(url.path(), vec![b'x'; INSTALLER.len()]);
    assert!(h.fetch(&download).await.is_err());
    assert!(!path.exists());

    for digest in [
        serde_json::Value::Null,
        serde_json::json!("sha512:abc"),
        serde_json::json!("sha256:abc"),
    ] {
        let mut invalid = asset.clone();
        invalid["digest"] = digest;
        put_github_release(&h.server, &[invalid]);
        assert!(refusal(&h, kind).await.contains("no verifiable installer"));
    }
    put_github_release(&h.server, &[asset.clone(), asset]);
    assert!(refusal(&h, kind).await.contains("no verifiable installer"));

    put_github_release(&h.server, &[]);
    let update = h
        .manager
        .check_app_update("0.1.0", None)
        .await
        .unwrap()
        .unwrap();
    assert!(update.download.is_none());
    assert_eq!(update.page, h.server.url("/owner/repo/releases/tag/v0.2.0"));
}

#[tokio::test]
async fn signed_mode_rejects_a_newer_release_without_a_manifest() {
    let h = Harness::new().await;
    put_github_release(&h.server, &[]);
    assert!(
        refusal(&h, FileKind::Setup)
            .await
            .contains("it has no update.json")
    );
}

#[tokio::test]
async fn a_signed_newer_release_is_offered_and_its_installer_checked() {
    let h = Harness::new().await;
    h.publish("v0.2.0", false, &key(), INSTALLER);
    let update = h
        .manager
        .check_app_update("0.1.0", Some(FileKind::Setup))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(update.version, "0.2.0");
    let download = update.download.unwrap();
    let path = h
        .manager
        .fetch_app_update(
            &download,
            Arc::new(Progress::default()),
            CancellationToken::new(),
        )
        .await
        .unwrap();
    assert_eq!(std::fs::read(path).unwrap(), INSTALLER);
    assert_eq!(
        h.manager
            .check_app_update("0.2.0", Some(FileKind::Setup))
            .await
            .unwrap(),
        None
    );
}

#[tokio::test]
async fn a_release_signed_with_another_key_is_not_offered() {
    let h = Harness::new().await;
    h.publish(
        "v0.2.0",
        false,
        &SigningKey::from_bytes(&[8; 32]),
        INSTALLER,
    );
    let refused = h
        .manager
        .check_app_update("0.1.0", Some(FileKind::Setup))
        .await;
    assert!(
        matches!(refused, Err(CoreError::UpdateCheckFailed(_))),
        "{refused:?}"
    );
}

#[tokio::test]
async fn a_pre_release_waits_for_the_setting() {
    let h = Harness::new().await;
    h.publish("v0.2.0-beta.1", true, &key(), INSTALLER);
    assert_eq!(
        h.manager.check_app_update("0.1.0", None).await.unwrap(),
        None
    );
    let mut settings = h.manager.get_settings();
    settings.prerelease_updates = true;
    h.manager
        .change_settings(wirecatch_test_support::settings::whole_patch(&settings))
        .unwrap();
    let update = h
        .manager
        .check_app_update("0.1.0", None)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(update.version, "0.2.0-beta.1");
    assert_eq!(update.download, None, "no installer was asked for");
}

async fn refusal(h: &Harness, kind: FileKind) -> String {
    let found = h.manager.check_app_update("0.1.0", Some(kind)).await;
    let Err(CoreError::UpdateCheckFailed(message)) = found else {
        panic!("expected a refusal, got {found:?}");
    };
    message
}

#[tokio::test]
async fn a_manifest_naming_another_version_is_refused() {
    let h = Harness::new().await;
    let odd = Odd {
        manifest_version: Some("0.3.0"),
        ..Odd::default()
    };
    h.publish_odd("v0.2.0", false, &key(), INSTALLER, &odd);
    let message = refusal(&h, FileKind::Setup).await;
    assert!(message.contains("names version 0.3.0"), "{message}");
}

#[tokio::test]
async fn an_asset_of_another_size_than_its_manifest_is_refused() {
    let h = Harness::new().await;
    let odd = Odd {
        asset_size: Some(INSTALLER.len() + 1),
        ..Odd::default()
    };
    h.publish_odd("v0.2.0", false, &key(), INSTALLER, &odd);
    let message = refusal(&h, FileKind::Setup).await;
    assert!(message.contains("no verifiable installer"), "{message}");
}

#[tokio::test]
async fn a_release_with_no_installer_for_this_install_is_refused() {
    let h = Harness::new().await;
    h.publish("v0.2.0", false, &key(), INSTALLER);
    let message = refusal(&h, FileKind::AppImage).await;
    assert!(message.contains("no verifiable installer"), "{message}");
}

#[tokio::test]
async fn a_release_page_that_is_not_https_is_refused() {
    let h = Harness::new().await;
    let odd = Odd {
        page: Some("file:///C:/Windows/System32/calc.exe"),
        ..Odd::default()
    };
    h.publish_odd("v0.2.0", false, &key(), INSTALLER, &odd);
    let message = refusal(&h, FileKind::Setup).await;
    assert!(message.contains("page"), "{message}");
}

#[tokio::test]
async fn trying_again_uses_the_checked_installer_already_downloaded() {
    let h = Harness::new().await;
    h.publish("v0.2.0", false, &key(), INSTALLER);
    let update = h
        .manager
        .check_app_update("0.1.0", Some(FileKind::Setup))
        .await
        .unwrap()
        .unwrap();
    let download = update.download.unwrap();
    let fetch = || {
        h.manager.fetch_app_update(
            &download,
            Arc::new(Progress::default()),
            CancellationToken::new(),
        )
    };
    let first = fetch().await.unwrap();
    let sent = h.server.requests(download.url.path());
    assert_eq!(fetch().await.unwrap(), first);
    assert_eq!(h.server.requests(download.url.path()), sent);
}

#[tokio::test]
async fn installers_from_an_earlier_run_are_removed_when_the_app_starts() {
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().join("data");
    let updates = data.join(UPDATES_FOLDER);
    std::fs::create_dir_all(&updates).unwrap();
    std::fs::write(updates.join("Wirecatch-0.1.0-setup.exe"), INSTALLER).unwrap();
    let config = ManagerConfig::for_tests(data, KnownFolders::under_home(dir.path().join("home")));
    let _manager = Manager::open(config).await.unwrap();
    assert!(!updates.exists());
}

#[tokio::test]
async fn an_installer_of_the_wrong_size_is_refused() {
    let h = Harness::new().await;
    h.publish("v0.2.0", false, &key(), b"a different installer");
    let update = h
        .manager
        .check_app_update("0.1.0", Some(FileKind::Setup))
        .await
        .unwrap()
        .unwrap();
    let failed = h
        .manager
        .fetch_app_update(
            &update.download.unwrap(),
            Arc::new(Progress::default()),
            CancellationToken::new(),
        )
        .await;
    let Err(CoreError::UpdateCheckFailed(message)) = failed else {
        panic!("expected a refusal, got {failed:?}");
    };
    assert!(message.contains("not the size"), "{message}");
}

#[tokio::test]
async fn an_installer_that_does_not_match_its_manifest_is_deleted() {
    let h = Harness::new().await;
    h.publish("v0.2.0", false, &key(), &vec![b'x'; INSTALLER.len()]);
    let update = h
        .manager
        .check_app_update("0.1.0", Some(FileKind::Setup))
        .await
        .unwrap()
        .unwrap();
    let download = update.download.unwrap();
    let failed = h
        .manager
        .fetch_app_update(
            &download,
            Arc::new(Progress::default()),
            CancellationToken::new(),
        )
        .await;
    let Err(CoreError::UpdateCheckFailed(message)) = failed else {
        panic!("expected a refusal, got {failed:?}");
    };
    assert_eq!(
        message,
        "The downloaded installer does not match this release, so it was not run. Try again."
    );
    let kept = h
        .manager
        .inner
        .data_dir
        .join(UPDATES_FOLDER)
        .join(&download.file.name);
    assert!(!kept.exists());
}

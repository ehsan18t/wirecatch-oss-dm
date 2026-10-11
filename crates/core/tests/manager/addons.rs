//! The media add-on: install from a local server, checksum refusal, safe
//! unpacking, progress, idempotence, updates and removal.

use wirecatch_protocol::app::{AddonState, UiEvent};

use crate::media_support::{
    FileServer, Releases, addon_dir, exe, harness, install, next_event, path_exists, publish,
    sha256, tar_xz, wait_addon, ytdlp_asset,
};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn install_downloads_checks_and_unpacks_only_the_expected_files() {
    let server = FileServer::start().await;
    let sources = publish(&server, &Releases::fake(), "r1");
    let h = harness(sources).await;
    assert_eq!(h.manager.list_addons()[0].state, AddonState::NotInstalled);
    let mut ui = h.manager.subscribe();
    h.manager.install_addon("media").await.unwrap();
    // Progress is reported while it downloads.
    let installing = wait_addon(&mut ui, "installing", |a| {
        matches!(a.state, AddonState::Installing { .. })
    })
    .await;
    let AddonState::Installing { total, .. } = installing.state else {
        unreachable!()
    };
    assert!(total.is_some_and(|t| t > 0));
    let done = wait_addon(&mut ui, "installed", |a| {
        matches!(a.state, AddonState::Installed | AddonState::Failed { .. })
    })
    .await;
    assert_eq!(done.state, AddonState::Installed, "{done:?}");
    assert_eq!(
        done.installed_version.as_deref(),
        Some("yt-dlp 2026.08.19, ffmpeg 9.0.2, Deno 2.9.7")
    );
    assert_eq!(done.size, None, "nothing left to download");

    let dir = addon_dir(&h);
    for file in [
        dir.join("yt-dlp").join(exe("yt-dlp")),
        dir.join("ffmpeg").join("bin").join(exe("ffmpeg")),
        dir.join("ffmpeg").join("bin").join("avcodec-63.dll"),
        dir.join("deno").join(exe("deno")),
        dir.join("installed.json"),
    ] {
        assert!(file.is_file(), "{} is missing", file.display());
    }
    // Entries that climb out, unlisted files and links are never written.
    for never in [
        dir.join("escape.txt"),
        dir.parent().unwrap().join("escape.txt"),
        dir.join("evil.txt"),
        dir.parent().unwrap().parent().unwrap().join("evil.txt"),
        dir.join("ffmpeg").join("bin").join("extra.exe"),
        dir.join("ffmpeg").join("bin").join("link"),
        dir.join("deno").join("link"),
    ] {
        assert!(!path_exists(&never), "{} was written", never.display());
    }
    // The installed programs run.
    let deno = std::process::Command::new(dir.join("deno").join(exe("deno")))
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&deno.stdout).contains("deno 2.9.7"));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode =
            |p: std::path::PathBuf| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(dir.join("yt-dlp").join("yt-dlp")), 0o755);
        assert_eq!(
            mode(dir.join("ffmpeg").join("bin").join("avcodec-63.dll")),
            0o644
        );
    }
    // The downloads and staging folders are cleaned up.
    let work: Vec<_> = std::fs::read_dir(dir.join(".work"))
        .map(|d| d.filter_map(Result::ok).map(|e| e.file_name()).collect())
        .unwrap_or_default();
    assert!(work.is_empty(), "left in .work: {work:?}");

    // Installing again does nothing: no request, no change.
    let before = server.requests("r1/yt-dlp").len();
    h.manager.install_addon("media").await.unwrap();
    assert_eq!(h.manager.list_addons()[0].state, AddonState::Installed);
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    assert_eq!(server.requests("r1/yt-dlp").len(), before);

    // It survives a restart.
    let mut h = h;
    h.restart().await;
    assert_eq!(h.manager.list_addons()[0].state, AddonState::Installed);

    assert!(h.manager.install_addon("something-else").await.is_err());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_release_that_does_not_match_its_checksum_is_never_unpacked() {
    let server = FileServer::start().await;
    let mut sources = publish(&server, &Releases::fake(), "bad");
    sources.ffmpeg.sha256 = sha256(b"something else");
    let h = harness(sources).await;
    let info = install(&h).await;
    let AddonState::Failed { message } = info.state else {
        panic!("installed anyway: {info:?}");
    };
    assert!(
        message.contains("did not match its published checksum"),
        "{message}"
    );
    assert!(!message.contains('\u{2014}'));
    let dir = addon_dir(&h);
    // Nothing is put in place, not even the parts that matched.
    for part in ["yt-dlp", "ffmpeg", "deno"] {
        assert!(!dir.join(part).exists(), "{part} was installed");
    }
    assert!(!dir.join("installed.json").exists());
    // The bad download is deleted, so trying again downloads it again.
    let leftovers: Vec<String> = std::fs::read_dir(dir.join(".work"))
        .unwrap()
        .filter_map(Result::ok)
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with("ffmpeg"))
        .collect();
    assert!(leftovers.is_empty(), "{leftovers:?}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_link_where_a_program_should_be_is_refused() {
    let server = FileServer::start().await;
    let mut releases = Releases::fake();
    releases.deno = tar_xz(&[(&format!("deno/{}", exe("deno")), None)]);
    let sources = publish(&server, &releases, "link");
    let h = harness(sources).await;
    let info = install(&h).await;
    let AddonState::Failed { message } = info.state else {
        panic!("installed anyway: {info:?}");
    };
    assert!(message.contains("not a plain file"), "{message}");
    assert!(!addon_dir(&h).join("deno").exists());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn remove_deletes_the_add_on() {
    let server = FileServer::start().await;
    let h = harness(publish(&server, &Releases::fake(), "rm")).await;
    assert_eq!(install(&h).await.state, AddonState::Installed);
    let mut ui = h.manager.subscribe();
    h.manager.remove_addon("media").await.unwrap();
    let info = next_event(&mut ui, "AddonsChanged", |e| match e {
        UiEvent::AddonsChanged(list) => Some(list[0].clone()),
        _ => None,
    })
    .await;
    assert_eq!(info.state, AddonState::NotInstalled);
    assert!(!addon_dir(&h).exists());
    // Removing what is not there is fine.
    h.manager.remove_addon("media").await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_newer_yt_dlp_is_installed_quietly_after_its_checksum_matches() {
    let server = FileServer::start().await;
    let mut sources = publish(&server, &Releases::fake(), "up");
    sources.ytdlp_latest_release = Some(server.url("api/latest"));
    let h = harness(sources).await;
    assert_eq!(install(&h).await.state, AddonState::Installed);
    let ffmpeg_requests = server.requests("up/ffmpeg.zip").len();

    let new_ytdlp = [crate::media_support::fake_tool(), b"v2".to_vec()].concat();
    server.put("up2/yt-dlp", new_ytdlp.clone());
    let release = |sums: String| {
        server.put("up2/SHA2-256SUMS", sums);
        serde_json::json!({
            "tag_name": "2026.09.30",
            "assets": [
                {"name": ytdlp_asset(), "browser_download_url": server.url("up2/yt-dlp"), "size": new_ytdlp.len()},
                {"name": "SHA2-256SUMS", "browser_download_url": server.url("up2/SHA2-256SUMS"), "size": 100},
            ]
        })
        .to_string()
    };

    // A checksum list that does not match: the update is refused, the old
    // yt-dlp stays.
    server.put(
        "api/latest",
        release(format!("{}  {}\n", sha256(b"other"), ytdlp_asset())),
    );
    h.manager.check_addon_updates().await.unwrap();
    crate::common::wait_until("the refused update ends", || {
        let info = h.manager.list_addons().remove(0);
        (!matches!(info.state, AddonState::Installing { .. })).then_some(())
    })
    .await;
    let info = h.manager.list_addons().remove(0);
    assert!(info.installed_version.unwrap().contains("2026.08.19"));
    assert_eq!(info.state, AddonState::UpdateAvailable);
    let installed = std::fs::read(addon_dir(&h).join("yt-dlp").join(exe("yt-dlp"))).unwrap();
    assert_ne!(installed, new_ytdlp);

    // The right checksum: installed without asking.
    server.put(
        "api/latest",
        release(format!(
            "0000000000000000000000000000000000000000000000000000000000000000  yt-dlp_other\n{}  {}\n",
            sha256(&new_ytdlp),
            ytdlp_asset()
        )),
    );
    h.manager.check_addon_updates().await.unwrap();
    let info = crate::common::wait_until("yt-dlp updated", || {
        let info = h.manager.list_addons().remove(0);
        (info.state == AddonState::Installed
            && info
                .installed_version
                .as_deref()
                .is_some_and(|v| v.contains("2026.09.30")))
        .then_some(info)
    })
    .await;
    assert!(info.installed_version.unwrap().contains("ffmpeg 9.0.2"));
    let installed = std::fs::read(addon_dir(&h).join("yt-dlp").join(exe("yt-dlp"))).unwrap();
    assert_eq!(installed, new_ytdlp);
    // ffmpeg and Deno were not downloaded again.
    assert_eq!(server.requests("up/ffmpeg.zip").len(), ffmpeg_requests);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_new_pinned_ffmpeg_replaces_only_ffmpeg() {
    let server = FileServer::start().await;
    let releases = Releases::fake();
    let sources = publish(&server, &releases, "pin");
    let mut h = harness(sources.clone()).await;
    assert_eq!(install(&h).await.state, AddonState::Installed);
    let others = (
        server.requests("pin/yt-dlp").len(),
        server.requests("pin/deno.tar.xz").len(),
    );

    // The app is updated with a new ffmpeg pin.
    let mut newer = sources;
    let new_zip = crate::media_support::zip(&[
        (
            &format!("ff/bin/{}", exe("ffmpeg")),
            Some(b"new ffmpeg".as_slice()),
        ),
        ("ff/bin/avcodec-63.dll", Some(b"new library".as_slice())),
    ]);
    server.put("pin2/ffmpeg.zip", new_zip.clone());
    newer.ffmpeg.version = "n9.1".into();
    newer.ffmpeg.url = server.url("pin2/ffmpeg.zip");
    newer.ffmpeg.sha256 = sha256(&new_zip);
    newer.ffmpeg.size = new_zip.len() as u64;
    let newer = std::sync::Arc::new(newer);
    *h.tweak_mut() = Some(std::sync::Arc::new(
        move |c: wirecatch_core::ManagerConfig| c.with_addon_sources((*newer).clone()),
    ));
    h.restart().await;
    let info = h.manager.list_addons().remove(0);
    assert_eq!(info.state, AddonState::UpdateAvailable);
    assert_eq!(info.size, Some(new_zip.len() as u64));
    h.manager.update_addon("media").await.unwrap();
    crate::common::wait_until("ffmpeg updated", || {
        let info = h.manager.list_addons().remove(0);
        (info.state == AddonState::Installed).then_some(())
    })
    .await;
    let dir = addon_dir(&h);
    assert_eq!(
        std::fs::read(dir.join("ffmpeg").join("bin").join(exe("ffmpeg"))).unwrap(),
        b"new ffmpeg"
    );
    assert_eq!(
        (
            server.requests("pin/yt-dlp").len(),
            server.requests("pin/deno.tar.xz").len()
        ),
        others
    );
}

/// D6: a settings profile lists the installed add-ons, and importing it on
/// another machine installs them there.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn importing_settings_reinstalls_the_add_on() {
    let server = FileServer::start().await;
    let sources = publish(&server, &Releases::fake(), "prof");
    let first = harness(sources.clone()).await;
    assert_eq!(install(&first).await.state, AddonState::Installed);
    let file = first.root.path().join("profile.json");
    first.manager.export_settings(&file).await.unwrap();
    let profile: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
    assert_eq!(profile["addons"], serde_json::json!(["media"]));

    let second = harness(sources).await;
    let mut ui = second.manager.subscribe();
    second.manager.import_settings(&file).await.unwrap();
    let info = wait_addon(&mut ui, "installed on import", |a| {
        a.state == AddonState::Installed
    })
    .await;
    assert_eq!(info.id, "media");
    assert!(addon_dir(&second).join("installed.json").is_file());
}

/// An antivirus may take a program away; installing again must look at the
/// disk rather than trust what was found at startup.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_program_taken_away_is_reinstalled() {
    let server = FileServer::start().await;
    let h = harness(publish(&server, &Releases::fake(), "gone")).await;
    assert_eq!(install(&h).await.state, AddonState::Installed);
    let ytdlp = addon_dir(&h).join("yt-dlp").join(exe("yt-dlp"));
    std::fs::remove_file(&ytdlp).unwrap();
    let mut ui = h.manager.subscribe();
    h.manager.install_addon("media").await.unwrap();
    let info = wait_addon(&mut ui, "repaired", |a| a.state == AddonState::Installed).await;
    assert_eq!(info.id, "media");
    assert!(ytdlp.is_file(), "yt-dlp was not put back");
}

/// When a program cannot be started, the add-on is looked at again, so the UI
/// offers the repair instead of showing it as installed.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_program_that_cannot_start_shows_the_add_on_as_damaged() {
    let server = FileServer::start().await;
    let h = harness(publish(&server, &Releases::fake(), "nostart")).await;
    assert_eq!(install(&h).await.state, AddonState::Installed);
    std::fs::remove_file(addon_dir(&h).join("yt-dlp").join(exe("yt-dlp"))).unwrap();
    h.manager
        .add_capture(crate::media_support::media_capture(
            &server.url("watch/1"),
            wirecatch_protocol::extension::MediaKind::Page,
            None,
        ))
        .await
        .unwrap();
    let message = crate::common::wait_until("the add-on shown as damaged", || {
        match h.manager.list_addons().remove(0).state {
            AddonState::Failed { message } => Some(message),
            _ => None,
        }
    })
    .await;
    assert!(message.contains("damaged"), "{message}");
}

/// A removal that fails half way leaves the add-on shown as it is on disk.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_failed_removal_leaves_a_consistent_state() {
    let server = FileServer::start().await;
    let mut h = harness(publish(&server, &Releases::fake(), "stuck")).await;
    assert_eq!(install(&h).await.state, AddonState::Installed);
    let dir = addon_dir(&h);
    // Part of it is already gone, and another part cannot be deleted.
    std::fs::remove_file(dir.join("yt-dlp").join(exe("yt-dlp"))).unwrap();
    let lock = Lock::hold(&dir.join("deno"), &exe("deno"));
    let removed = h.manager.remove_addon("media").await;
    assert!(removed.is_err(), "the removal should have failed");
    let shown = h.manager.list_addons().remove(0).state;
    drop(lock);
    h.restart().await;
    assert_eq!(shown, h.manager.list_addons().remove(0).state);
    assert!(
        matches!(shown, AddonState::Failed { .. } | AddonState::NotInstalled),
        "{shown:?}"
    );
}

/// Keeps a file from being deleted until dropped.
struct Lock {
    #[cfg(windows)]
    _file: std::fs::File,
    #[cfg(unix)]
    dir: std::path::PathBuf,
}

impl Lock {
    fn hold(dir: &std::path::Path, name: &str) -> Self {
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            let file = std::fs::OpenOptions::new()
                .read(true)
                .share_mode(0)
                .open(dir.join(name))
                .unwrap();
            Lock { _file: file }
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = name;
            std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o555)).unwrap();
            Lock {
                dir: dir.to_path_buf(),
            }
        }
    }
}

#[cfg(unix)]
impl Drop for Lock {
    fn drop(&mut self) {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&self.dir, std::fs::Permissions::from_mode(0o755));
    }
}

/// Removing the add-on while a video is downloaded with it is refused.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_add_on_is_not_removed_while_in_use() {
    let server = FileServer::start().await;
    let h = harness(publish(&server, &Releases::fake(), "busy")).await;
    assert_eq!(install(&h).await.state, AddonState::Installed);
    server.put("fmt/v.mp4", crate::common::test_data(400 * 1024));
    server.throttle("fmt/v.mp4", Some(50 * 1024));
    let info = serde_json::json!({"title": "Busy", "formats": [
        {"format_id": "1", "ext": "mp4", "vcodec": "avc1", "acodec": "mp4a", "height": 360,
         "protocol": "https", "url": server.url("fmt/v.mp4")}
    ]});
    crate::media_support::tell_tool(&h, "info.json", &info.to_string());
    let mut ui = h.manager.subscribe();
    h.manager
        .add_capture(crate::media_support::media_capture(
            &server.url("watch/9"),
            wirecatch_protocol::extension::MediaKind::Page,
            None,
        ))
        .await
        .unwrap();
    let choice = next_event(&mut ui, "a choice", |e| match e {
        UiEvent::MediaChoiceRequested(c) => Some(c.clone()),
        _ => None,
    })
    .await;
    h.manager
        .choose_media(
            choice.id,
            wirecatch_protocol::app::MediaSelection {
                option_id: "1".into(),
                folder: None,
                file_name: None,
                queue: None,
            },
        )
        .await
        .unwrap();
    let id = h.manager.list_downloads()[0].id;
    h.wait_for(id, "downloading", |r| r.done > 0).await;
    let refused = h.manager.remove_addon("media").await;
    assert!(
        matches!(refused, Err(wirecatch_core::CoreError::WrongState(_))),
        "{refused:?}"
    );
    assert!(addon_dir(&h).join("installed.json").is_file());
    assert_eq!(h.manager.list_addons()[0].state, AddonState::Installed);
}

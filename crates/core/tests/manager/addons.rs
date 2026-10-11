//! The media add-on: install from a local server, checksum refusal, safe
//! unpacking, progress, idempotence, updates and removal.

use wirecatch_protocol::app::{AddonState, UiEvent};

use crate::media_support::{
    FileServer, Releases, addon_dir, exe, harness, install, next_event, path_exists, publish,
    sha256, sign, tar_xz, wait_addon, ytdlp_asset,
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
async fn stopping_a_first_install_leaves_nothing_behind() {
    let server = FileServer::start().await;
    let h = harness(publish(&server, &Releases::fake(), "cx")).await;
    // Slow enough to still be running when it is stopped.
    server.throttle("cx/yt-dlp", Some(32 * 1024));
    let mut ui = h.manager.subscribe();
    h.manager.install_addon("media").await.unwrap();
    wait_addon(&mut ui, "installing", |a| {
        matches!(a.state, AddonState::Installing { .. })
    })
    .await;
    h.manager.cancel_addon_operation("media").await.unwrap();
    assert_eq!(h.manager.list_addons()[0].state, AddonState::NotInstalled);
    assert!(!path_exists(addon_dir(&h)), "the partial files are gone");
    // Nothing in progress: stopping again is fine.
    h.manager.cancel_addon_operation("media").await.unwrap();
    assert!(h.manager.cancel_addon_operation("flash").await.is_err());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn stopping_an_update_keeps_the_installed_version_working() {
    let (server, h, new_ytdlp) = updatable("cu").await;
    let before = installed_ytdlp(&h);
    serve_release(
        &server,
        "2026.09.30",
        &new_ytdlp,
        new_ytdlp.len() as u64,
        &sha256(&new_ytdlp),
        None,
        OLD,
    );
    server.throttle(
        &format!("rel/2026.09.30/{}", ytdlp_asset()),
        Some(16 * 1024),
    );
    let mut ui = h.manager.subscribe();
    // A quiet update starts (auto_update_addons is on by default).
    h.manager.check_addon_updates().await.unwrap();
    wait_addon(&mut ui, "updating", |a| {
        matches!(a.state, AddonState::Installing { .. })
    })
    .await;
    h.manager.cancel_addon_operation("media").await.unwrap();

    let info = h.manager.list_addons().remove(0);
    assert!(
        matches!(
            info.state,
            AddonState::Installed | AddonState::UpdateAvailable
        ),
        "{:?}",
        info.state
    );
    assert!(info.installed_version.unwrap().contains("2026.08.19"));
    assert_eq!(installed_ytdlp(&h), before, "the old yt-dlp is untouched");
    assert!(
        !path_exists(addon_dir(&h).join(".work")),
        "staging discarded"
    );
    // Still runnable: the installed Deno answers.
    let deno = std::process::Command::new(addon_dir(&h).join("deno").join(exe("deno")))
        .arg("--version")
        .output()
        .unwrap();
    assert!(deno.status.success());
}

/// Publishes yt-dlp release `tag` the way GitHub describes it: the program at
/// `rel/<tag>/<asset>` (or at `program_url`), announced as `listed_size`
/// bytes, a checksum list giving it `listed_hash`, and the description at
/// `api/latest`, published at `published`.
fn serve_release(
    server: &FileServer,
    tag: &str,
    program: &[u8],
    listed_size: u64,
    listed_hash: &str,
    program_url: Option<String>,
    published: &str,
) {
    serve_signed(
        server,
        tag,
        program,
        listed_size,
        listed_hash,
        program_url,
        published,
        Sig::Good,
    );
}

/// How the checksum list of a served release is signed.
#[derive(Clone, Copy, Debug)]
enum Sig {
    /// By the key the test sources trust.
    Good,
    /// By the trusted key, but over a different list.
    OtherData,
    /// By a key that is not trusted.
    WrongKey,
    /// Not at all: the release has no signature file.
    Missing,
    /// A signature file that is not a signature.
    Garbage,
}

#[allow(clippy::too_many_arguments)]
fn serve_signed(
    server: &FileServer,
    tag: &str,
    program: &[u8],
    listed_size: u64,
    listed_hash: &str,
    program_url: Option<String>,
    published: &str,
    sig: Sig,
) {
    let release = put_release(
        server,
        tag,
        program,
        listed_size,
        listed_hash,
        program_url,
        published,
        sig,
    );
    server.put("api/releases", serde_json::json!([release]).to_string());
}

/// Serves several releases `(tag, program, published)`, each signed and
/// listed correctly, as yt-dlp's list of releases in the order given.
fn serve_releases(server: &FileServer, releases: &[(&str, &[u8], &str)]) {
    let list: Vec<serde_json::Value> = releases
        .iter()
        .map(|(tag, program, published)| {
            put_release(
                server,
                tag,
                program,
                program.len() as u64,
                &sha256(program),
                None,
                published,
                Sig::Good,
            )
        })
        .collect();
    server.put("api/releases", serde_json::Value::from(list).to_string());
}

/// Puts one release's files on the server and returns its description.
#[allow(clippy::too_many_arguments)]
fn put_release(
    server: &FileServer,
    tag: &str,
    program: &[u8],
    listed_size: u64,
    listed_hash: &str,
    program_url: Option<String>,
    published: &str,
    sig: Sig,
) -> serde_json::Value {
    let asset = ytdlp_asset();
    server.put(&format!("rel/{tag}/{asset}"), program.to_vec());
    let list = format!(
        "0000000000000000000000000000000000000000000000000000000000000000  yt-dlp_other\n{listed_hash}  {asset}\n"
    );
    let signature = match sig {
        Sig::Good => Some(sign(list.as_bytes(), true)),
        Sig::OtherData => Some(sign(b"another list", true)),
        Sig::WrongKey => Some(sign(list.as_bytes(), false)),
        Sig::Missing => None,
        Sig::Garbage => Some(b"not a signature".to_vec()),
    };
    server.put(&format!("rel/{tag}/SHA2-256SUMS"), list);
    let program_url = program_url.unwrap_or_else(|| server.url(&format!("rel/{tag}/{asset}")));
    let mut assets = vec![
        serde_json::json!({"name": asset, "browser_download_url": program_url, "size": listed_size}),
        serde_json::json!({"name": "SHA2-256SUMS", "browser_download_url": server.url(&format!("rel/{tag}/SHA2-256SUMS")), "size": 100}),
    ];
    if let Some(signature) = signature {
        server.put(&format!("rel/{tag}/SHA2-256SUMS.sig"), signature);
        assets.push(serde_json::json!({"name": "SHA2-256SUMS.sig", "browser_download_url": server.url(&format!("rel/{tag}/SHA2-256SUMS.sig")), "size": 100}));
    }
    serde_json::json!({
        "tag_name": tag,
        "published_at": published,
        "draft": false,
        "prerelease": false,
        "assets": assets,
    })
}

/// Published well before the harness's clock (Monday 12 October 2026).
const OLD: &str = "2026-10-01T00:00:00Z";

async fn updatable(prefix: &str) -> (FileServer, crate::common::Harness, Vec<u8>) {
    let server = FileServer::start().await;
    let mut sources = publish(&server, &Releases::fake(), prefix);
    sources.ytdlp_releases = Some(server.url("api/releases"));
    sources.ytdlp_release_downloads = server.url("rel/");
    let h = harness(sources).await;
    assert_eq!(install(&h).await.state, AddonState::Installed);
    let new_ytdlp = [crate::media_support::fake_tool(), b"v2".to_vec()].concat();
    (server, h, new_ytdlp)
}

/// Checks for updates and waits until any quiet update has ended.
async fn check(h: &crate::common::Harness) -> wirecatch_protocol::app::AddonInfo {
    h.manager.check_addon_updates().await.unwrap();
    crate::common::wait_until("the update check ends", || {
        let info = h.manager.list_addons().remove(0);
        (!matches!(info.state, AddonState::Installing { .. })).then_some(info)
    })
    .await
}

fn installed_ytdlp(h: &crate::common::Harness) -> Vec<u8> {
    std::fs::read(addon_dir(h).join("yt-dlp").join(exe("yt-dlp"))).unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_check_that_cannot_reach_the_releases_says_so() {
    // Nothing is served at the release address.
    let (_server, h, _) = updatable("nr").await;
    let error = h.manager.check_addon_updates().await.unwrap_err();
    assert!(
        error
            .to_string()
            .starts_with("Wirecatch could not check for a newer media add-on"),
        "{error}"
    );
    // The add-on itself is fine.
    assert_eq!(h.manager.list_addons()[0].state, AddonState::Installed);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_newer_yt_dlp_is_installed_quietly_after_its_checksum_matches() {
    let (server, h, new_ytdlp) = updatable("up").await;
    let ffmpeg_requests = server.requests("up/ffmpeg.zip").len();
    let size = new_ytdlp.len() as u64;

    // A checksum list that does not match: refused, the old yt-dlp stays.
    serve_release(
        &server,
        "2026.09.30",
        &new_ytdlp,
        size,
        &sha256(b"other"),
        None,
        OLD,
    );
    let info = check(&h).await;
    assert!(info.installed_version.unwrap().contains("2026.08.19"));
    assert_eq!(info.state, AddonState::UpdateAvailable);
    assert_ne!(installed_ytdlp(&h), new_ytdlp);

    // The right checksum: installed without asking.
    serve_release(
        &server,
        "2026.09.30",
        &new_ytdlp,
        size,
        &sha256(&new_ytdlp),
        None,
        OLD,
    );
    let info = check(&h).await;
    assert_eq!(info.state, AddonState::Installed);
    let version = info.installed_version.unwrap();
    assert!(
        version.contains("2026.09.30") && version.contains("ffmpeg 9.0.2"),
        "{version}"
    );
    assert_eq!(installed_ytdlp(&h), new_ytdlp);
    // ffmpeg and Deno were not downloaded again.
    assert_eq!(server.requests("up/ffmpeg.zip").len(), ffmpeg_requests);
}

/// D21.1: a newer yt-dlp is installed only when its checksum list carries a
/// valid signature by the pinned key; otherwise nothing is downloaded and a
/// person who asked hears why.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_update_whose_checksum_list_is_not_signed_by_yt_dlp_is_refused() {
    let (server, h, new_ytdlp) = updatable("sig").await;
    let size = new_ytdlp.len() as u64;
    let hash = sha256(&new_ytdlp);
    let program = format!("rel/2026.09.30/{}", ytdlp_asset());
    for sig in [Sig::Missing, Sig::Garbage, Sig::WrongKey, Sig::OtherData] {
        serve_signed(
            &server,
            "2026.09.30",
            &new_ytdlp,
            size,
            &hash,
            None,
            OLD,
            sig,
        );
        let error = h.manager.check_addon_updates().await.unwrap_err();
        assert!(
            error.to_string().starts_with(
                "yt-dlp 2026.09.30 was not installed because its checksum list does not carry a valid signature from yt-dlp's developers"
            ),
            "{sig:?}: {error}"
        );
        let info = h.manager.list_addons().remove(0);
        assert_eq!(info.state, AddonState::Installed, "{sig:?}");
        assert!(info.latest_version.unwrap().contains("2026.08.19"));
        // Not even offered to the Update button.
        h.manager.update_addon("media").await.unwrap();
        assert_eq!(h.manager.list_addons()[0].state, AddonState::Installed);
    }
    assert!(
        server.requests(&program).is_empty(),
        "the program was fetched"
    );
    assert_ne!(installed_ytdlp(&h), new_ytdlp);

    // Signed by the pinned key: installed.
    serve_release(&server, "2026.09.30", &new_ytdlp, size, &hash, None, OLD);
    let info = check(&h).await;
    assert_eq!(info.state, AddonState::Installed);
    assert_eq!(installed_ytdlp(&h), new_ytdlp);
}

/// The release description may not send the download anywhere but yt-dlp's
/// own release folder, and a tag that is not a plain version is ignored.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_update_only_comes_from_yt_dlps_release_folder() {
    let (server, h, new_ytdlp) = updatable("pinned").await;
    let size = new_ytdlp.len() as u64;
    let hash = sha256(&new_ytdlp);
    server.put("elsewhere/yt-dlp", new_ytdlp.clone());
    let elsewhere = Some(server.url("elsewhere/yt-dlp"));
    serve_release(
        &server,
        "2026.09.30",
        &new_ytdlp,
        size,
        &hash,
        elsewhere,
        OLD,
    );
    let info = check(&h).await;
    assert_eq!(info.state, AddonState::Installed);
    assert!(info.latest_version.unwrap().contains("2026.08.19"));
    assert!(server.requests("elsewhere/yt-dlp").is_empty());

    for tag in ["nightly", "2026.09.30-x", "2026.09.30/../2026.09.30"] {
        serve_release(&server, tag, &new_ytdlp, size, &hash, None, OLD);
        let info = check(&h).await;
        assert_eq!(info.state, AddonState::Installed, "{tag}");
    }
    assert_ne!(installed_ytdlp(&h), new_ytdlp);
}

/// A file larger than announced, or of unknown or unreasonable size, is never
/// downloaded or hashed.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_update_is_capped_at_its_announced_size() {
    let (server, h, new_ytdlp) = updatable("cap").await;
    let hash = sha256(&new_ytdlp);
    for listed in [0, 300 * 1024 * 1024] {
        serve_release(&server, "2026.09.30", &new_ytdlp, listed, &hash, None, OLD);
        let info = check(&h).await;
        assert_eq!(info.state, AddonState::Installed, "size {listed}");
    }
    assert!(
        server
            .requests(&format!("rel/2026.09.30/{}", ytdlp_asset()))
            .is_empty()
    );

    // Announced far smaller than it is: offered, but the download stops.
    serve_release(&server, "2026.09.30", &new_ytdlp, 1000, &hash, None, OLD);
    let info = check(&h).await;
    assert_eq!(info.state, AddonState::UpdateAvailable);
    assert_ne!(installed_ytdlp(&h), new_ytdlp);
    let work = addon_dir(&h).join(".work");
    let big: Vec<_> = std::fs::read_dir(&work)
        .map(|d| {
            d.filter_map(Result::ok)
                .filter(|e| e.metadata().is_ok_and(|m| m.len() > 1000 + 4096))
                .map(|e| e.file_name())
                .collect()
        })
        .unwrap_or_default();
    assert!(big.is_empty(), "{big:?}");
}

/// D21.1: a release is installed only once it is two days old, whether by a
/// quiet update, the Update button, a repair or a reinstall.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_fresh_release_waits_two_days_on_every_install_path() {
    let (server, h, new_ytdlp) = updatable("fresh").await;
    let size = new_ytdlp.len() as u64;
    let hash = sha256(&new_ytdlp);
    // The harness's clock: Monday 12 October 2026, 12:00 local time.
    serve_release(
        &server,
        "2026.10.12",
        &new_ytdlp,
        size,
        &hash,
        None,
        "2026-10-12T00:00:00Z",
    );
    let program = format!("rel/2026.10.12/{}", ytdlp_asset());
    let waiting = "A newer version came out today; Wirecatch installs it once it is two days old.";
    // Not installed quietly, and not offered: the add-on stays as it is and
    // says why.
    let info = check(&h).await;
    assert_eq!(info.state, AddonState::Installed);
    assert_eq!(info.note.as_deref(), Some(waiting));
    assert!(info.latest_version.unwrap().contains("2026.08.19"));
    // The Update button does not install it either.
    h.manager.update_addon("media").await.unwrap();
    assert_eq!(h.manager.list_addons()[0].state, AddonState::Installed);
    // Nor does a repair (the path a reinstall takes too).
    let ytdlp = addon_dir(&h).join("yt-dlp").join(exe("yt-dlp"));
    std::fs::remove_file(&ytdlp).unwrap();
    let mut ui = h.manager.subscribe();
    h.manager.install_addon("media").await.unwrap();
    let info = wait_addon(&mut ui, "repaired", |a| a.state == AddonState::Installed).await;
    assert!(
        info.installed_version.unwrap().contains("2026.08.19"),
        "the repair installed the new release"
    );
    assert!(ytdlp.is_file());
    assert!(
        server.requests(&program).is_empty(),
        "the program was fetched"
    );

    // A day later it is still waiting.
    h.clock.advance(chrono::TimeDelta::hours(24));
    let info = check(&h).await;
    assert_eq!(info.state, AddonState::Installed);
    assert_eq!(
        info.note.as_deref(),
        Some("A newer version came out yesterday; Wirecatch installs it once it is two days old.")
    );
    // Two days old: offered at once, and installed quietly by the next check.
    h.clock.advance(chrono::TimeDelta::hours(24));
    let info = h.manager.list_addons().remove(0);
    assert_eq!(info.state, AddonState::UpdateAvailable);
    assert_eq!(info.note, None);
    let info = check(&h).await;
    assert_eq!(info.state, AddonState::Installed);
    assert_eq!(installed_ytdlp(&h), new_ytdlp);
}

/// While the newest release is younger than two days, the newest one that is
/// old enough is installed instead, found in the list of releases.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_newest_release_old_enough_is_used_while_a_newer_one_waits() {
    let (server, h, older) = updatable("list").await;
    let newest = [crate::media_support::fake_tool(), b"v3".to_vec()].concat();
    // Listed oldest first, to show the order the list comes in does not matter.
    serve_releases(
        &server,
        &[
            ("2026.09.01", &older, "2026-09-01T00:00:00Z"),
            ("2026.09.30", &older, OLD),
            ("2026.10.12", &newest, "2026-10-12T00:00:00Z"),
        ],
    );
    let info = check(&h).await;
    assert_eq!(info.state, AddonState::Installed);
    assert!(
        info.installed_version.unwrap().contains("2026.09.30"),
        "the newest release old enough was not installed"
    );
    assert_eq!(installed_ytdlp(&h), older);
    assert_eq!(
        info.note.as_deref(),
        Some("A newer version came out today; Wirecatch installs it once it is two days old.")
    );
    assert!(
        server
            .requests(&format!("rel/2026.10.12/{}", ytdlp_asset()))
            .is_empty()
    );
    // Once that one is two days old, it follows.
    h.clock.advance(chrono::TimeDelta::hours(48));
    let info = check(&h).await;
    assert!(info.installed_version.unwrap().contains("2026.10.12"));
    assert_eq!(info.note, None);
    assert_eq!(installed_ytdlp(&h), newest);
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

/// D6: a settings profile lists the installed add-ons. Importing it on another
/// machine offers them (the UI asks) and downloads nothing by itself.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn importing_settings_offers_the_add_on_without_downloading_it() {
    let server = FileServer::start().await;
    let sources = publish(&server, &Releases::fake(), "prof");
    let first = harness(sources.clone()).await;
    assert_eq!(install(&first).await.state, AddonState::Installed);
    let file = first.root.path().join("profile.json");
    first.manager.export_settings(&file).await.unwrap();
    let profile: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
    assert_eq!(profile["addons"], serde_json::json!(["media"]));
    let requests = server.requests("prof/yt-dlp").len();

    let second = harness(sources).await;
    let mut ui = second.manager.subscribe();
    second.manager.import_settings(&file).await.unwrap();
    let summary = next_event(&mut ui, "the import summary", |e| match e {
        UiEvent::SettingsImported(summary) => Some(summary.clone()),
        _ => None,
    })
    .await;
    assert_eq!(summary.addons_to_offer, vec!["media".to_string()]);
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    assert_eq!(
        second.manager.list_addons()[0].state,
        AddonState::NotInstalled
    );
    assert_eq!(
        server.requests("prof/yt-dlp").len(),
        requests,
        "it was downloaded"
    );

    // Where it is installed already, nothing is offered.
    let mut ui = first.manager.subscribe();
    first.manager.import_settings(&file).await.unwrap();
    let summary = next_event(&mut ui, "the import summary", |e| match e {
        UiEvent::SettingsImported(summary) => Some(summary.clone()),
        _ => None,
    })
    .await;
    assert!(summary.addons_to_offer.is_empty());
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
                // Others may read it (a checksum check), not delete or move it.
                .share_mode(1)
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
                later: false,
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

/// An update whose second part cannot be swapped in leaves a record that says
/// exactly what is on disk, and the next install finishes the job.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_half_finished_update_is_recorded_and_repaired() {
    let server = FileServer::start().await;
    let sources = publish(&server, &Releases::fake(), "half");
    let mut h = harness(sources.clone()).await;
    assert_eq!(install(&h).await.state, AddonState::Installed);

    // The app now pins a newer yt-dlp and a newer ffmpeg.
    let mut newer = sources;
    let new_ytdlp = [crate::media_support::fake_tool(), b"v2".to_vec()].concat();
    server.put("half2/yt-dlp", new_ytdlp.clone());
    newer.ytdlp.version = "2026.09.01".into();
    newer.ytdlp.url = server.url("half2/yt-dlp");
    newer.ytdlp.sha256 = sha256(&new_ytdlp);
    newer.ytdlp.size = new_ytdlp.len() as u64;
    let new_zip = crate::media_support::zip(&[
        (
            &format!("ff/bin/{}", exe("ffmpeg")),
            Some(b"new ffmpeg".as_slice()),
        ),
        ("ff/bin/avcodec-63.dll", Some(b"new library".as_slice())),
    ]);
    server.put("half2/ffmpeg.zip", new_zip.clone());
    newer.ffmpeg.version = "n9.1".into();
    newer.ffmpeg.url = server.url("half2/ffmpeg.zip");
    newer.ffmpeg.sha256 = sha256(&new_zip);
    newer.ffmpeg.size = new_zip.len() as u64;
    let newer = std::sync::Arc::new(newer);
    *h.tweak_mut() = Some(std::sync::Arc::new(
        move |c: wirecatch_core::ManagerConfig| c.with_addon_sources((*newer).clone()),
    ));
    h.restart().await;

    // ffmpeg's folder cannot be moved away, so only yt-dlp is swapped in.
    let dir = addon_dir(&h);
    let lock = Lock::hold(&dir.join("ffmpeg"), &format!("bin/{}", exe("ffmpeg")));
    let mut ui = h.manager.subscribe();
    h.manager.update_addon("media").await.unwrap();
    wait_addon(&mut ui, "the update failed", |a| {
        matches!(a.state, AddonState::Failed { .. })
    })
    .await;
    let record: serde_json::Value =
        serde_json::from_slice(&std::fs::read(dir.join("installed.json")).unwrap()).unwrap();
    assert_eq!(record["components"]["yt-dlp"]["version"], "2026.09.01");
    assert_eq!(
        record["components"]["ffmpeg"]["version"],
        "n9.0.2-25-g67b60c310b"
    );
    let shown = crate::common::wait_until("the record is read again", || {
        h.manager
            .list_addons()
            .remove(0)
            .installed_version
            .filter(|v| v.contains("2026.09.01"))
    })
    .await;
    assert!(shown.contains("ffmpeg 9.0.2"), "{shown}");

    drop(lock);
    h.manager.update_addon("media").await.unwrap();
    wait_addon(&mut ui, "repaired", |a| a.state == AddonState::Installed).await;
    assert_eq!(
        std::fs::read(dir.join("ffmpeg").join("bin").join(exe("ffmpeg"))).unwrap(),
        b"new ffmpeg"
    );
}

/// Each file's checksum is recorded at install. A file changed in place (same
/// size) is not looked for on every start, but a repair finds and replaces it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_repair_finds_a_file_changed_in_place() {
    let server = FileServer::start().await;
    let releases = Releases::fake();
    let mut h = harness(publish(&server, &releases, "tamper")).await;
    assert_eq!(install(&h).await.state, AddonState::Installed);
    let dir = addon_dir(&h);
    let record: serde_json::Value =
        serde_json::from_slice(&std::fs::read(dir.join("installed.json")).unwrap()).unwrap();
    let file = &record["components"]["yt-dlp"]["files"][0];
    let ytdlp = dir.join("yt-dlp").join(exe("yt-dlp"));
    assert_eq!(file["sha256"], sha256(&std::fs::read(&ytdlp).unwrap()));

    let mut bytes = std::fs::read(&ytdlp).unwrap();
    let last = bytes.len() - 1;
    bytes[last] ^= 0xff;
    std::fs::write(&ytdlp, &bytes).unwrap();
    h.restart().await;
    // Not checked on every start: sizes only.
    assert_eq!(h.manager.list_addons()[0].state, AddonState::Installed);

    let mut ui = h.manager.subscribe();
    h.manager.install_addon("media").await.unwrap();
    wait_addon(&mut ui, "repaired", |a| a.state == AddonState::Installed).await;
    assert_eq!(std::fs::read(&ytdlp).unwrap(), releases.ytdlp);
}

//! The media add-on: install from a local server, checksum refusal, safe
//! unpacking, progress, idempotence, updates and removal.

use wirecatch_protocol::app::{AddonId, AddonState, UiEvent};
use wirecatch_protocol::wire::ErrorCode;

use crate::common::TestResult;
use crate::media_support::{
    FileServer, Releases, addon_dir, digest, exe, harness, install, next_event, path_exists,
    publish, release_file, sha256, sign, tar_xz, wait_addon, ytdlp_asset,
};
use wirecatch_core::ReleaseFile;
use wirecatch_engine::units::{KIB, KIB_USIZE, MIB};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn install_downloads_checks_and_unpacks_only_the_expected_files() -> TestResult {
    let server = FileServer::start().await;
    let sources = publish(&server, &Releases::fake()?, "r1")?;
    let h = harness(sources).await?;
    assert_eq!(h.manager.list_addons()[0].state, AddonState::NotInstalled);
    let mut ui = h.manager.subscribe();
    h.manager.install_addon(AddonId::Media).await.unwrap();
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
    let before = server.requests("r1/yt-dlp");
    h.manager.install_addon(AddonId::Media).await.unwrap();
    assert_eq!(h.manager.list_addons()[0].state, AddonState::Installed);
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    assert_eq!(server.requests("r1/yt-dlp"), before);

    // It survives a restart.
    let mut h = h;
    h.restart().await?;
    assert_eq!(h.manager.list_addons()[0].state, AddonState::Installed);

    assert!(h.manager.install_addon(AddonId::Unknown).await.is_err());
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_release_that_does_not_match_its_checksum_is_never_unpacked() -> TestResult {
    let server = FileServer::start().await;
    let mut sources = publish(&server, &Releases::fake()?, "bad")?;
    sources.ffmpeg = sources.ffmpeg.for_release(
        sources.ffmpeg.version(),
        ReleaseFile::new(
            sources.ffmpeg.release().url().clone(),
            digest(b"something else"),
            sources.ffmpeg.release().size(),
        )?,
    );
    let h = harness(sources).await?;
    let info = install(&h).await?;
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
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_link_where_a_program_should_be_is_refused() -> TestResult {
    let server = FileServer::start().await;
    let mut releases = Releases::fake()?;
    releases.deno = tar_xz(&[(&format!("deno/{}", exe("deno")), None)])?;
    let sources = publish(&server, &releases, "link")?;
    let h = harness(sources).await?;
    let info = install(&h).await?;
    let AddonState::Failed { message } = info.state else {
        panic!("installed anyway: {info:?}");
    };
    assert!(message.contains("not a plain file"), "{message}");
    assert!(!addon_dir(&h).join("deno").exists());
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn remove_deletes_the_add_on() -> TestResult {
    let server = FileServer::start().await;
    let h = harness(publish(&server, &Releases::fake()?, "rm")?).await?;
    assert_eq!(install(&h).await?.state, AddonState::Installed);
    let mut ui = h.manager.subscribe();
    h.manager.remove_addon(AddonId::Media).await.unwrap();
    let info = next_event(&mut ui, "AddonsChanged", |e| match e {
        UiEvent::AddonsChanged(list) => Some(list[0].clone()),
        _ => None,
    })
    .await;
    assert_eq!(info.state, AddonState::NotInstalled);
    assert!(!addon_dir(&h).exists());
    // Removing what is not there is fine.
    h.manager.remove_addon(AddonId::Media).await.unwrap();
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn stopping_a_first_install_leaves_nothing_behind() -> TestResult {
    let server = FileServer::start().await;
    let h = harness(publish(&server, &Releases::fake()?, "cx")?).await?;
    // Slow enough to still be running when it is stopped.
    server.throttle("cx/yt-dlp", Some(32 * KIB));
    let mut ui = h.manager.subscribe();
    h.manager.install_addon(AddonId::Media).await.unwrap();
    wait_addon(&mut ui, "installing", |a| {
        matches!(a.state, AddonState::Installing { .. })
    })
    .await;
    h.manager
        .cancel_addon_operation(AddonId::Media)
        .await
        .unwrap();
    assert_eq!(h.manager.list_addons()[0].state, AddonState::NotInstalled);
    assert!(!path_exists(addon_dir(&h)), "the partial files are gone");
    // Nothing in progress: stopping again is fine.
    h.manager
        .cancel_addon_operation(AddonId::Media)
        .await
        .unwrap();
    assert!(
        h.manager
            .cancel_addon_operation(AddonId::Unknown)
            .await
            .is_err()
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn stopping_an_update_keeps_the_installed_version_working() -> TestResult {
    let (server, h, new_ytdlp) = updatable("cu").await?;
    let before = installed_ytdlp(&h)?;
    serve_release(
        &server,
        "2026.09.30",
        &new_ytdlp,
        new_ytdlp.len() as u64,
        &sha256(&new_ytdlp),
        None,
        OLD,
    )?;
    server.throttle(&format!("rel/2026.09.30/{}", ytdlp_asset()), Some(16 * KIB));
    let mut ui = h.manager.subscribe();
    // A quiet update starts (auto_update_addons is on by default).
    h.manager.check_addon_updates().await.unwrap();
    wait_addon(&mut ui, "updating", |a| {
        matches!(a.state, AddonState::Installing { .. })
    })
    .await;
    h.manager
        .cancel_addon_operation(AddonId::Media)
        .await
        .unwrap();

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
    assert_eq!(installed_ytdlp(&h)?, before, "the old yt-dlp is untouched");
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
    Ok(())
}

/// One yt-dlp release as a test serves it.
struct Release<'a> {
    tag: &'a str,
    program: &'a [u8],
    /// The size the release description announces.
    listed_size: u64,
    /// The hash the checksum list gives the program.
    listed_hash: &'a str,
    /// Where the description says the program is; by default its own
    /// release folder, `rel/<tag>/<asset>`.
    program_url: Option<String>,
    published: &'a str,
    sig: Sig,
}

/// Publishes yt-dlp release `tag` the way GitHub describes it: the program at
/// `rel/<tag>/<asset>` (or at `program_url`), announced as `listed_size`
/// bytes, a checksum list giving it `listed_hash` signed by the trusted key,
/// and the description at `api/releases`, published at `published`.
fn serve_release(
    server: &FileServer,
    tag: &str,
    program: &[u8],
    listed_size: u64,
    listed_hash: &str,
    program_url: Option<String>,
    published: &str,
) -> TestResult {
    serve(
        server,
        &Release {
            tag,
            program,
            listed_size,
            listed_hash,
            program_url,
            published,
            sig: Sig::Good,
        },
    )
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

/// Publishes `release` as the only one in yt-dlp's list of releases.
fn serve(server: &FileServer, release: &Release<'_>) -> TestResult {
    let release = put_release(server, release)?;
    server.put("api/releases", serde_json::json!([release]).to_string());
    Ok(())
}

/// Serves several releases `(tag, program, published)`, each signed and
/// listed correctly, as yt-dlp's list of releases in the order given.
fn serve_releases(server: &FileServer, releases: &[(&str, &[u8], &str)]) -> TestResult {
    let list = releases
        .iter()
        .map(|&(tag, program, published)| {
            put_release(
                server,
                &Release {
                    tag,
                    program,
                    listed_size: program.len() as u64,
                    listed_hash: &sha256(program),
                    program_url: None,
                    published,
                    sig: Sig::Good,
                },
            )
        })
        .collect::<TestResult<Vec<_>>>()?;
    server.put("api/releases", serde_json::Value::from(list).to_string());
    Ok(())
}

/// Puts one release's files on the server and returns its description.
fn put_release(server: &FileServer, release: &Release<'_>) -> TestResult<serde_json::Value> {
    let Release {
        tag, listed_hash, ..
    } = release;
    let asset = ytdlp_asset();
    server.put(&format!("rel/{tag}/{asset}"), release.program.to_vec());
    let list = format!(
        "0000000000000000000000000000000000000000000000000000000000000000  yt-dlp_other\n{listed_hash}  {asset}\n"
    );
    let signature = match release.sig {
        Sig::Good => Some(sign(list.as_bytes(), true)?),
        Sig::OtherData => Some(sign(b"another list", true)?),
        Sig::WrongKey => Some(sign(list.as_bytes(), false)?),
        Sig::Missing => None,
        Sig::Garbage => Some(b"not a signature".to_vec()),
    };
    server.put(&format!("rel/{tag}/SHA2-256SUMS"), list);
    let program_url = release
        .program_url
        .clone()
        .unwrap_or_else(|| server.url(&format!("rel/{tag}/{asset}")).to_string());
    let mut assets = vec![
        serde_json::json!({"name": asset, "browser_download_url": program_url, "size": release.listed_size}),
        serde_json::json!({"name": "SHA2-256SUMS", "browser_download_url": server.url(&format!("rel/{tag}/SHA2-256SUMS")).as_str(), "size": 100}),
    ];
    if let Some(signature) = signature {
        server.put(&format!("rel/{tag}/SHA2-256SUMS.sig"), signature);
        assets.push(serde_json::json!({"name": "SHA2-256SUMS.sig", "browser_download_url": server.url(&format!("rel/{tag}/SHA2-256SUMS.sig")).as_str(), "size": 100}));
    }
    Ok(serde_json::json!({
        "tag_name": tag,
        "published_at": release.published,
        "draft": false,
        "prerelease": false,
        "assets": assets,
    }))
}

/// Published well before the harness's clock (Monday 12 October 2026).
const OLD: &str = "2026-10-01T00:00:00Z";

async fn updatable(prefix: &str) -> TestResult<(FileServer, crate::common::Harness, Vec<u8>)> {
    let server = FileServer::start().await;
    let mut sources = publish(&server, &Releases::fake()?, prefix)?;
    sources.ytdlp_releases = Some(server.url("api/releases"));
    sources.ytdlp_release_downloads = server.url("rel/");
    let h = harness(sources).await?;
    assert_eq!(install(&h).await?.state, AddonState::Installed);
    let new_ytdlp = crate::media_support::ytdlp_release("2026.09.30")?;
    Ok((server, h, new_ytdlp))
}

/// Checks for updates and waits until any quiet update has ended.
async fn check(h: &crate::common::Harness) -> TestResult<wirecatch_protocol::app::AddonInfo> {
    h.manager.check_addon_updates().await?;
    Ok(crate::common::wait_until("the update check ends", || {
        let info = h.manager.list_addons().remove(0);
        (!matches!(info.state, AddonState::Installing { .. })).then_some(info)
    })
    .await)
}

fn installed_ytdlp(h: &crate::common::Harness) -> TestResult<Vec<u8>> {
    Ok(std::fs::read(
        addon_dir(h).join("yt-dlp").join(exe("yt-dlp")),
    )?)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_check_that_cannot_reach_the_releases_says_so() -> TestResult {
    // Nothing is served at the release address.
    let (_server, h, _) = updatable("nr").await?;
    let error = h.manager.check_addon_updates().await.unwrap_err();
    assert!(
        error
            .to_string()
            .starts_with("Wirecatch could not check for a newer media add-on"),
        "{error}"
    );
    // A client tells this from a request it got wrong by the code, never by the words.
    assert_eq!(error.to_protocol().code, ErrorCode::UpdateCheckFailed);
    // The add-on itself is fine.
    assert_eq!(h.manager.list_addons()[0].state, AddonState::Installed);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_newer_yt_dlp_is_installed_quietly_after_its_checksum_matches() -> TestResult {
    let (server, h, new_ytdlp) = updatable("up").await?;
    let ffmpeg_requests = server.requests("up/ffmpeg.zip");
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
    )?;
    let info = check(&h).await?;
    assert!(info.installed_version.unwrap().contains("2026.08.19"));
    assert_eq!(info.state, AddonState::UpdateAvailable);
    assert_ne!(installed_ytdlp(&h)?, new_ytdlp);

    // The right checksum: installed without asking.
    serve_release(
        &server,
        "2026.09.30",
        &new_ytdlp,
        size,
        &sha256(&new_ytdlp),
        None,
        OLD,
    )?;
    let info = check(&h).await?;
    assert_eq!(info.state, AddonState::Installed);
    let version = info.installed_version.unwrap();
    assert!(
        version.contains("2026.09.30") && version.contains("ffmpeg 9.0.2"),
        "{version}"
    );
    assert_eq!(installed_ytdlp(&h)?, new_ytdlp);
    // ffmpeg and Deno were not downloaded again.
    assert_eq!(server.requests("up/ffmpeg.zip"), ffmpeg_requests);
    Ok(())
}

/// A release published under a newer tag but holding another release's files (an older yt-dlp with its own signed checksum list, say) is not taken for the newer one: what yt-dlp says it is must be the tag.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_older_yt_dlp_under_a_newer_tag_is_refused() -> TestResult {
    let (server, h, _) = updatable("replay").await?;
    let before = installed_ytdlp(&h)?;
    // Another build of the pinned release, signed and listed, under a newer tag.
    let replayed = crate::media_support::ytdlp_release("2026.08.19")?;
    let size = replayed.len() as u64;
    serve_release(
        &server,
        "2026.09.30",
        &replayed,
        size,
        &sha256(&replayed),
        None,
        OLD,
    )?;
    let info = check(&h).await?;
    assert!(info.installed_version.unwrap().contains("2026.08.19"));
    assert_eq!(
        installed_ytdlp(&h)?,
        before,
        "the replayed release was installed"
    );
    Ok(())
}

/// A new yt-dlp is installed only when its version check succeeds and answers with exactly its tag: one that cannot start, crashes, fails, says more, or says too much is not installed, and the installed one keeps working. Only a successful answer that is not its tag is evidence of a replayed release, which the person hears of even from a quiet update; one that cannot start or fails may be an antivirus or a full temporary folder, and is only tried again later.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_yt_dlp_whose_version_check_fails_is_refused() -> TestResult {
    let (server, h, _) = updatable("probe").await?;
    let before = installed_ytdlp(&h)?;
    let cases: [(&str, Vec<u8>, bool); 5] = [
        ("2026.09.26", b"not a program at all".to_vec(), false),
        (
            "2026.09.27",
            crate::media_support::ytdlp_release("2026.09.27+crash")?,
            false,
        ),
        (
            "2026.09.28",
            crate::media_support::ytdlp_release("2026.09.28+fail")?,
            false,
        ),
        (
            "2026.09.29",
            crate::media_support::ytdlp_release("2026.09.29+extra")?,
            true,
        ),
        (
            "2026.09.30",
            crate::media_support::ytdlp_release("2026.09.30+flood")?,
            true,
        ),
    ];
    for (tag, program, warned) in cases {
        let size = program.len() as u64;
        serve_release(&server, tag, &program, size, &sha256(&program), None, OLD)?;
        let info = check(&h).await?;
        assert!(
            info.installed_version.unwrap().contains("2026.08.19"),
            "{tag}"
        );
        assert_eq!(installed_ytdlp(&h)?, before, "{tag} was installed");
        let noted = info.note.is_some_and(|n| n.contains(tag));
        assert_eq!(noted, warned, "{tag}: warned {noted}");
    }
    Ok(())
}

/// The release folder may be written without its last slash.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_release_folder_without_its_last_slash_still_matches() -> TestResult {
    let server = FileServer::start().await;
    let mut sources = publish(&server, &Releases::fake()?, "slash")?;
    sources.ytdlp_releases = Some(server.url("api/releases"));
    sources.ytdlp_release_downloads = server.url("rel");
    let h = harness(sources).await?;
    assert_eq!(install(&h).await?.state, AddonState::Installed);
    let new_ytdlp = crate::media_support::ytdlp_release("2026.09.30")?;
    let size = new_ytdlp.len() as u64;
    serve_release(
        &server,
        "2026.09.30",
        &new_ytdlp,
        size,
        &sha256(&new_ytdlp),
        None,
        OLD,
    )?;
    let info = check(&h).await?;
    assert!(info.installed_version.unwrap().contains("2026.09.30"));
    Ok(())
}

/// A release refused for not being what it claims is a warning sign, not a passing failure: the person hears of it even from a quiet update, and it is not downloaded or run again.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_refused_release_is_reported_and_not_tried_again() -> TestResult {
    let (server, h, _) = updatable("again").await?;
    let replayed = crate::media_support::ytdlp_release("2026.08.19")?;
    let size = replayed.len() as u64;
    serve_release(
        &server,
        "2026.09.30",
        &replayed,
        size,
        &sha256(&replayed),
        None,
        OLD,
    )?;
    let program = format!("rel/2026.09.30/{}", ytdlp_asset());
    let info = check(&h).await?;
    let note = info.note.unwrap_or_default();
    assert!(note.contains("2026.09.30"), "{note:?}");
    let fetched = server.requests(&program);
    assert!(
        !path_exists(
            addon_dir(&h)
                .join(".work")
                .join("yt-dlp-2026.09.30.download")
        ),
        "the refused release is kept"
    );
    h.manager.update_addon(AddonId::Media).await?;
    let info = check(&h).await?;
    assert_eq!(
        server.requests(&program),
        fetched,
        "the refused release was fetched again"
    );
    assert!(info.installed_version.unwrap().contains("2026.08.19"));
    assert!(info.note.is_some_and(|n| n.contains("2026.09.30")));
    Ok(())
}

/// A release refused after the Update button is told once, in the note, not again as a failed install.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_refusal_after_the_update_button_is_told_once() -> TestResult {
    let (server, h, _) = updatable("button").await?;
    let mut settings = h.manager.get_settings();
    settings.auto_update_addons = false;
    h.manager.update_settings(settings)?;
    let replayed = crate::media_support::ytdlp_release("2026.08.19")?;
    let size = replayed.len() as u64;
    serve_release(
        &server,
        "2026.09.30",
        &replayed,
        size,
        &sha256(&replayed),
        None,
        OLD,
    )?;
    assert_eq!(check(&h).await?.state, AddonState::UpdateAvailable);
    let mut ui = h.manager.subscribe();
    h.manager.update_addon(AddonId::Media).await?;
    let info = wait_addon(&mut ui, "the update ended", |a| {
        !matches!(a.state, AddonState::Installing { .. })
    })
    .await;
    assert!(info.note.is_some_and(|n| n.contains("2026.09.30")));
    assert!(
        !matches!(info.state, AddonState::Failed { .. }),
        "told twice: {:?}",
        info.state
    );
    assert!(info.installed_version.unwrap().contains("2026.08.19"));
    Ok(())
}

/// A refusal is shown while it still concerns the installed yt-dlp, next to a newer release that is waiting, and goes once a newer release is installed.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_refusal_is_shown_until_a_newer_release_is_installed() -> TestResult {
    let (server, h, _) = updatable("clear").await?;
    let replayed = crate::media_support::ytdlp_release("2026.08.19")?;
    let size = replayed.len() as u64;
    serve_release(
        &server,
        "2026.09.30",
        &replayed,
        size,
        &sha256(&replayed),
        None,
        OLD,
    )?;
    assert!(
        check(&h)
            .await?
            .note
            .is_some_and(|n| n.contains("2026.09.30"))
    );
    let waiting = "A newer version was released today; Wirecatch installs it after two days.";
    let fresh = crate::media_support::ytdlp_release("2026.10.12")?;
    serve_releases(&server, &[("2026.10.12", &fresh, "2026-10-12T00:00:00Z")])?;
    let note = check(&h).await?.note.unwrap_or_default();
    assert!(
        note.contains("2026.09.30") && note.contains(waiting),
        "{note}"
    );
    let genuine = crate::media_support::ytdlp_release("2026.10.05")?;
    serve_releases(
        &server,
        &[
            ("2026.10.05", &genuine, OLD),
            ("2026.10.12", &fresh, "2026-10-12T00:00:00Z"),
        ],
    )?;
    let info = check(&h).await?;
    assert!(info.installed_version.unwrap().contains("2026.10.05"));
    assert_eq!(info.note.as_deref(), Some(waiting));
    Ok(())
}

/// `url` with its host written as a name that resolves to this computer, as a poisoned name server or a hostile network could answer for GitHub.
fn by_name(mut url: wirecatch_engine::Url) -> TestResult<wirecatch_engine::Url> {
    url.set_host(Some("localhost"))?;
    Ok(url)
}

/// GitHub is never on the local network: an add-on release whose name answers with a local address is not fetched.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_release_whose_name_answers_local_is_not_fetched() -> TestResult {
    let server = FileServer::start().await;
    let releases = Releases::fake()?;
    let mut sources = publish(&server, &releases, "lan")?;
    sources.ffmpeg = sources.ffmpeg.for_release(
        sources.ffmpeg.version(),
        ReleaseFile::new(
            by_name(server.url("lan/ffmpeg.zip"))?,
            digest(&releases.ffmpeg),
            releases.ffmpeg.len() as u64,
        )?,
    );
    let h = harness(sources).await?;
    let info = install(&h).await?;
    assert!(
        matches!(info.state, AddonState::Failed { .. }),
        "{:?}",
        info.state
    );
    assert_eq!(
        server.requests("lan/ffmpeg.zip"),
        0,
        "the local server was contacted"
    );
    Ok(())
}

/// The list of yt-dlp releases is not read from a name that answers with a local address either.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_release_list_whose_name_answers_local_is_not_read() -> TestResult {
    let server = FileServer::start().await;
    let mut sources = publish(&server, &Releases::fake()?, "lanlist")?;
    sources.ytdlp_releases = Some(by_name(server.url("api/releases"))?);
    sources.ytdlp_release_downloads = server.url("rel/");
    let h = harness(sources).await?;
    assert_eq!(install(&h).await?.state, AddonState::Installed);
    server.put("api/releases", b"[]".to_vec());
    let error = h.manager.check_addon_updates().await.unwrap_err();
    assert!(error.to_string().contains("could not check"), "{error}");
    assert_eq!(
        server.requests("api/releases"),
        0,
        "the local server was contacted"
    );
    Ok(())
}

/// yt-dlp's cache belongs to the version that wrote it: a new yt-dlp starts without it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn updating_yt_dlp_clears_its_cache() -> TestResult {
    let (server, h, new_ytdlp) = updatable("cache").await?;
    let cache = addon_dir(&h).join("cache");
    std::fs::create_dir_all(cache.join("youtube-nsig"))?;
    std::fs::write(cache.join("youtube-nsig").join("old.json"), b"{}")?;
    let size = new_ytdlp.len() as u64;
    serve_release(
        &server,
        "2026.09.30",
        &new_ytdlp,
        size,
        &sha256(&new_ytdlp),
        None,
        OLD,
    )?;
    let info = check(&h).await?;
    assert!(info.installed_version.unwrap().contains("2026.09.30"));
    assert!(
        !path_exists(cache.join("youtube-nsig")),
        "the old cache is still there"
    );
    Ok(())
}

/// D21.1: a newer yt-dlp is installed only when its checksum list carries a
/// valid signature by the pinned key; otherwise nothing is downloaded and a
/// person who asked hears why.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_update_whose_checksum_list_is_not_signed_by_yt_dlp_is_refused() -> TestResult {
    let (server, h, new_ytdlp) = updatable("sig").await?;
    let size = new_ytdlp.len() as u64;
    let hash = sha256(&new_ytdlp);
    let program = format!("rel/2026.09.30/{}", ytdlp_asset());
    for sig in [Sig::Missing, Sig::Garbage, Sig::WrongKey, Sig::OtherData] {
        serve(
            &server,
            &Release {
                tag: "2026.09.30",
                program: &new_ytdlp,
                listed_size: size,
                listed_hash: &hash,
                program_url: None,
                published: OLD,
                sig,
            },
        )?;
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
        h.manager.update_addon(AddonId::Media).await.unwrap();
        assert_eq!(h.manager.list_addons()[0].state, AddonState::Installed);
    }
    assert_eq!(server.requests(&program), 0, "the program was fetched");
    assert_ne!(installed_ytdlp(&h)?, new_ytdlp);

    // Signed by the pinned key: installed.
    serve_release(&server, "2026.09.30", &new_ytdlp, size, &hash, None, OLD)?;
    let info = check(&h).await?;
    assert_eq!(info.state, AddonState::Installed);
    assert_eq!(installed_ytdlp(&h)?, new_ytdlp);
    Ok(())
}

/// The release description may not send the download anywhere but yt-dlp's
/// own release folder, and a tag that is not a plain version is ignored.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_update_only_comes_from_yt_dlps_release_folder() -> TestResult {
    let (server, h, new_ytdlp) = updatable("pinned").await?;
    let size = new_ytdlp.len() as u64;
    let hash = sha256(&new_ytdlp);
    server.put("elsewhere/yt-dlp", new_ytdlp.clone());
    let elsewhere = Some(server.url("elsewhere/yt-dlp").to_string());
    serve_release(
        &server,
        "2026.09.30",
        &new_ytdlp,
        size,
        &hash,
        elsewhere,
        OLD,
    )?;
    let info = check(&h).await?;
    assert_eq!(info.state, AddonState::Installed);
    assert!(info.latest_version.unwrap().contains("2026.08.19"));
    assert_eq!(server.requests("elsewhere/yt-dlp"), 0);

    for tag in ["nightly", "2026.09.30-x", "2026.09.30/../2026.09.30"] {
        serve_release(&server, tag, &new_ytdlp, size, &hash, None, OLD)?;
        let info = check(&h).await?;
        assert_eq!(info.state, AddonState::Installed, "{tag}");
    }
    assert_ne!(installed_ytdlp(&h)?, new_ytdlp);
    Ok(())
}

/// A file larger than announced, or of unknown or unreasonable size, is never
/// downloaded or hashed.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_update_is_capped_at_its_announced_size() -> TestResult {
    let (server, h, new_ytdlp) = updatable("cap").await?;
    let hash = sha256(&new_ytdlp);
    for listed in [0, 300 * MIB] {
        serve_release(&server, "2026.09.30", &new_ytdlp, listed, &hash, None, OLD)?;
        let info = check(&h).await?;
        assert_eq!(info.state, AddonState::Installed, "size {listed}");
    }
    assert_eq!(
        server.requests(&format!("rel/2026.09.30/{}", ytdlp_asset())),
        0
    );

    // Announced far smaller than it is: offered, but the download stops.
    serve_release(&server, "2026.09.30", &new_ytdlp, 1000, &hash, None, OLD)?;
    let info = check(&h).await?;
    assert_eq!(info.state, AddonState::UpdateAvailable);
    assert_ne!(installed_ytdlp(&h)?, new_ytdlp);
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
    Ok(())
}

/// D21.1: a release is installed only once it is two days old, whether by a
/// quiet update, the Update button, a repair or a reinstall.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_fresh_release_waits_two_days_on_every_install_path() -> TestResult {
    let (server, h, _) = updatable("fresh").await?;
    let new_ytdlp = crate::media_support::ytdlp_release("2026.10.12")?;
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
    )?;
    let program = format!("rel/2026.10.12/{}", ytdlp_asset());
    let waiting = "A newer version was released today; Wirecatch installs it after two days.";
    // Not installed quietly, and not offered: the add-on stays as it is and
    // says why.
    let info = check(&h).await?;
    assert_eq!(info.state, AddonState::Installed);
    assert_eq!(info.note.as_deref(), Some(waiting));
    assert!(info.latest_version.unwrap().contains("2026.08.19"));
    // The Update button does not install it either.
    h.manager.update_addon(AddonId::Media).await.unwrap();
    assert_eq!(h.manager.list_addons()[0].state, AddonState::Installed);
    // Nor does a repair (the path a reinstall takes too).
    let ytdlp = addon_dir(&h).join("yt-dlp").join(exe("yt-dlp"));
    std::fs::remove_file(&ytdlp).unwrap();
    let mut ui = h.manager.subscribe();
    h.manager.install_addon(AddonId::Media).await.unwrap();
    let info = wait_addon(&mut ui, "repaired", |a| a.state == AddonState::Installed).await;
    assert!(
        info.installed_version.unwrap().contains("2026.08.19"),
        "the repair installed the new release"
    );
    assert!(ytdlp.is_file());
    assert_eq!(server.requests(&program), 0, "the program was fetched");

    // A day later it is still waiting.
    h.clock.advance(chrono::TimeDelta::hours(24));
    let info = check(&h).await?;
    assert_eq!(info.state, AddonState::Installed);
    assert_eq!(
        info.note.as_deref(),
        Some("A newer version was released yesterday; Wirecatch installs it after two days.")
    );
    // Two days old: offered at once, and installed quietly by the next check.
    h.clock.advance(chrono::TimeDelta::hours(24));
    let info = h.manager.list_addons().remove(0);
    assert_eq!(info.state, AddonState::UpdateAvailable);
    assert_eq!(info.note, None);
    let info = check(&h).await?;
    assert_eq!(info.state, AddonState::Installed);
    assert_eq!(installed_ytdlp(&h)?, new_ytdlp);
    Ok(())
}

/// While the newest release is younger than two days, the newest one that is
/// old enough is installed instead, found in the list of releases.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_newest_release_old_enough_is_used_while_a_newer_one_waits() -> TestResult {
    let (server, h, older) = updatable("list").await?;
    let newest = crate::media_support::ytdlp_release("2026.10.12")?;
    // Listed oldest first, to show the order the list comes in does not matter.
    serve_releases(
        &server,
        &[
            ("2026.09.01", &older, "2026-09-01T00:00:00Z"),
            ("2026.09.30", &older, OLD),
            ("2026.10.12", &newest, "2026-10-12T00:00:00Z"),
        ],
    )?;
    let info = check(&h).await?;
    assert_eq!(info.state, AddonState::Installed);
    assert!(
        info.installed_version.unwrap().contains("2026.09.30"),
        "the newest release old enough was not installed"
    );
    assert_eq!(installed_ytdlp(&h)?, older);
    assert_eq!(
        info.note.as_deref(),
        Some("A newer version was released today; Wirecatch installs it after two days.")
    );
    assert_eq!(
        server.requests(&format!("rel/2026.10.12/{}", ytdlp_asset())),
        0
    );
    // Once that one is two days old, it follows.
    h.clock.advance(chrono::TimeDelta::hours(48));
    let info = check(&h).await?;
    assert!(info.installed_version.unwrap().contains("2026.10.12"));
    assert_eq!(info.note, None);
    assert_eq!(installed_ytdlp(&h)?, newest);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_new_pinned_ffmpeg_replaces_only_ffmpeg() -> TestResult {
    let server = FileServer::start().await;
    let releases = Releases::fake()?;
    let sources = publish(&server, &releases, "pin")?;
    let mut h = harness(sources.clone()).await?;
    assert_eq!(install(&h).await?.state, AddonState::Installed);
    let others = (
        server.requests("pin/yt-dlp"),
        server.requests("pin/deno.tar.xz"),
    );

    // The app is updated with a new ffmpeg pin.
    let mut newer = sources;
    let new_zip = crate::media_support::zip(&[
        (
            &format!("ff/bin/{}", exe("ffmpeg")),
            Some(b"new ffmpeg".as_slice()),
        ),
        ("ff/bin/avcodec-63.dll", Some(b"new library".as_slice())),
    ])?;
    server.put("pin2/ffmpeg.zip", new_zip.clone());
    newer.ffmpeg = newer
        .ffmpeg
        .for_release("n9.1", release_file(&server, "pin2/ffmpeg.zip", &new_zip)?);
    let newer = std::sync::Arc::new(newer);
    *h.tweak_mut() = Some(std::sync::Arc::new(
        move |c: wirecatch_core::ManagerConfig| c.with_addon_sources((*newer).clone()),
    ));
    h.restart().await?;
    let info = h.manager.list_addons().remove(0);
    assert_eq!(info.state, AddonState::UpdateAvailable);
    assert_eq!(info.size, Some(new_zip.len() as u64));
    h.manager.update_addon(AddonId::Media).await.unwrap();
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
            server.requests("pin/yt-dlp"),
            server.requests("pin/deno.tar.xz")
        ),
        others
    );
    Ok(())
}

/// D6: a settings profile lists the installed add-ons. Importing it on another
/// machine offers them (the UI asks) and downloads nothing by itself.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn importing_settings_offers_the_add_on_without_downloading_it() -> TestResult {
    let server = FileServer::start().await;
    let sources = publish(&server, &Releases::fake()?, "prof")?;
    let first = harness(sources.clone()).await?;
    assert_eq!(install(&first).await?.state, AddonState::Installed);
    let file = first.root.path().join("profile.json");
    first.manager.export_settings(&file).await.unwrap();
    let profile: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
    assert_eq!(profile["addons"], serde_json::json!(["media"]));
    let requests = server.requests("prof/yt-dlp");

    let second = harness(sources).await?;
    let mut ui = second.manager.subscribe();
    second.manager.import_settings(&file).await.unwrap();
    let summary = next_event(&mut ui, "the import summary", |e| match e {
        UiEvent::SettingsImported(summary) => Some(summary.clone()),
        _ => None,
    })
    .await;
    assert_eq!(summary.addons_to_offer, vec![AddonId::Media]);
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    assert_eq!(
        second.manager.list_addons()[0].state,
        AddonState::NotInstalled
    );
    assert_eq!(
        server.requests("prof/yt-dlp"),
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
    assert_eq!(summary.addons_to_offer, [] as [AddonId; 0]);
    Ok(())
}

/// An antivirus may take a program away; installing again must look at the
/// disk rather than trust what was found at startup.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_program_taken_away_is_reinstalled() -> TestResult {
    let server = FileServer::start().await;
    let h = harness(publish(&server, &Releases::fake()?, "gone")?).await?;
    assert_eq!(install(&h).await?.state, AddonState::Installed);
    let ytdlp = addon_dir(&h).join("yt-dlp").join(exe("yt-dlp"));
    std::fs::remove_file(&ytdlp).unwrap();
    let mut ui = h.manager.subscribe();
    h.manager.install_addon(AddonId::Media).await.unwrap();
    let info = wait_addon(&mut ui, "repaired", |a| a.state == AddonState::Installed).await;
    assert_eq!(info.id, AddonId::Media);
    assert!(ytdlp.is_file(), "yt-dlp was not put back");
    Ok(())
}

/// When a program cannot be started, the add-on is looked at again, so the UI
/// offers the repair instead of showing it as installed.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_program_that_cannot_start_shows_the_add_on_as_damaged() -> TestResult {
    let server = FileServer::start().await;
    let h = harness(publish(&server, &Releases::fake()?, "nostart")?).await?;
    assert_eq!(install(&h).await?.state, AddonState::Installed);
    std::fs::remove_file(addon_dir(&h).join("yt-dlp").join(exe("yt-dlp"))).unwrap();
    h.manager
        .add_capture(crate::media_support::media_capture(
            server.url("watch/1"),
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
    Ok(())
}

/// A record that cannot be read at startup (held by an antivirus, say) leaves the add-on's state unknown: it is not shown as never installed, which would offer a download of what may be there. Once it can be read, the state is known again.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_unreadable_record_at_startup_leaves_the_state_unknown() -> TestResult {
    let server = FileServer::start().await;
    let mut h = harness(publish(&server, &Releases::fake()?, "unreadable")?).await?;
    assert_eq!(install(&h).await?.state, AddonState::Installed);
    let record = addon_dir(&h).join("installed.json");
    let kept = addon_dir(&h).join("installed.json.kept");
    std::fs::rename(&record, &kept)?;
    // A folder where the record should be cannot be read as one, even by an administrator.
    std::fs::create_dir_all(&record)?;
    h.restart().await?;
    let info = h.manager.list_addons().remove(0);
    assert!(
        matches!(&info.state, AddonState::Failed { message } if message.contains("could not be read")),
        "{info:?}"
    );
    // Readable again: the next look at the files (here the one Install starts with) says so, and tells the interface.
    std::fs::remove_dir(&record)?;
    std::fs::rename(&kept, &record)?;
    assert_eq!(install(&h).await?.state, AddonState::Installed);
    Ok(())
}

/// A record that could not be read at startup and is simply not there now: the next look at the files says so even though nothing else changed, rather than leave the add-on shown as unreadable.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_record_no_longer_unreadable_shows_the_state_on_disk() -> TestResult {
    let server = FileServer::start().await;
    let mut h = harness(publish(&server, &Releases::fake()?, "unreadable-gone")?).await?;
    let record = addon_dir(&h).join("installed.json");
    std::fs::create_dir_all(&record)?;
    h.restart().await?;
    std::fs::remove_dir(&record)?;
    let mut ui = h.manager.subscribe();
    h.manager.install_addon(AddonId::Media).await?;
    // The look Install starts with comes first, before the install itself reports.
    let first = next_event(&mut ui, "AddonsChanged", |e| match e {
        UiEvent::AddonsChanged(list) => Some(list[0].clone()),
        _ => None,
    })
    .await;
    assert_eq!(first.state, AddonState::NotInstalled);
    Ok(())
}

/// A removal that fails half way leaves the add-on shown as it is on disk.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_failed_removal_leaves_a_consistent_state() -> TestResult {
    let server = FileServer::start().await;
    let mut h = harness(publish(&server, &Releases::fake()?, "stuck")?).await?;
    assert_eq!(install(&h).await?.state, AddonState::Installed);
    let dir = addon_dir(&h);
    // Part of it is already gone, and another part cannot be deleted.
    std::fs::remove_file(dir.join("yt-dlp").join(exe("yt-dlp"))).unwrap();
    let lock = Lock::hold(&dir.join("deno"), &exe("deno"))?;
    let removed = h.manager.remove_addon(AddonId::Media).await;
    assert!(removed.is_err(), "the removal should have failed");
    let shown = h.manager.list_addons().remove(0).state;
    drop(lock);
    h.restart().await?;
    assert_eq!(shown, h.manager.list_addons().remove(0).state);
    assert!(
        matches!(shown, AddonState::Failed { .. } | AddonState::NotInstalled),
        "{shown:?}"
    );
    Ok(())
}

/// Keeps a file from being deleted until dropped.
struct Lock {
    #[cfg(windows)]
    _file: std::fs::File,
    #[cfg(unix)]
    dir: std::path::PathBuf,
}

impl Lock {
    #[cfg(windows)]
    fn hold(dir: &std::path::Path, name: &str) -> TestResult<Self> {
        use std::os::windows::fs::OpenOptionsExt;
        let file = std::fs::OpenOptions::new()
            .read(true)
            // Others may read it (a checksum check), not delete or move it.
            .share_mode(1)
            .open(dir.join(name))?;
        Ok(Lock { _file: file })
    }

    /// Unix deletes an open file, so the whole folder is made read-only instead.
    #[cfg(unix)]
    fn hold(dir: &std::path::Path, _name: &str) -> TestResult<Self> {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o555))?;
        Ok(Lock {
            dir: dir.to_path_buf(),
        })
    }
}

#[cfg(unix)]
impl Drop for Lock {
    fn drop(&mut self) {
        use std::os::unix::fs::PermissionsExt;
        if let Err(error) =
            std::fs::set_permissions(&self.dir, std::fs::Permissions::from_mode(0o755))
        {
            eprintln!(
                "could not make {} writable again: {error}",
                self.dir.display()
            );
        }
    }
}

/// Removing the add-on while a video is downloaded with it is refused.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_add_on_is_not_removed_while_in_use() -> TestResult {
    let server = FileServer::start().await;
    let h = harness(publish(&server, &Releases::fake()?, "busy")?).await?;
    assert_eq!(install(&h).await?.state, AddonState::Installed);
    server.put("fmt/v.mp4", crate::common::test_data(400 * KIB_USIZE));
    server.throttle("fmt/v.mp4", Some(50 * KIB));
    let info = serde_json::json!({"title": "Busy", "formats": [
        {"format_id": "1", "ext": "mp4", "vcodec": "avc1", "acodec": "mp4a", "height": 360,
         "protocol": "https", "url": server.url("fmt/v.mp4").as_str()}
    ]});
    crate::media_support::tell_tool(&h, "info.json", &info.to_string())?;
    let mut ui = h.manager.subscribe();
    h.manager
        .add_capture(crate::media_support::media_capture(
            server.url("watch/9"),
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
                output: wirecatch_protocol::app::OutputFormat::default(),
                audio_kbps: None,
                subtitles: Vec::new(),
                subtitle_files: false,
                chapters: false,
                every_video: None,
            },
        )
        .await
        .unwrap();
    let id = h.manager.list_downloads()[0].id;
    h.wait_for(id, "downloading", |r| r.done > 0).await;
    let refused = h.manager.remove_addon(AddonId::Media).await;
    assert!(
        matches!(refused, Err(wirecatch_core::CoreError::WrongState(_))),
        "{refused:?}"
    );
    assert!(addon_dir(&h).join("installed.json").is_file());
    assert_eq!(h.manager.list_addons()[0].state, AddonState::Installed);
    Ok(())
}

/// An update whose second part cannot be swapped in leaves a record that says
/// exactly what is on disk, and the next install finishes the job.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_half_finished_update_is_recorded_and_repaired() -> TestResult {
    let server = FileServer::start().await;
    let sources = publish(&server, &Releases::fake()?, "half")?;
    let mut h = harness(sources.clone()).await?;
    assert_eq!(install(&h).await?.state, AddonState::Installed);

    // The app now pins a newer yt-dlp and a newer ffmpeg.
    let mut newer = sources;
    let new_ytdlp = [crate::media_support::fake_tool()?, b"v2".to_vec()].concat();
    server.put("half2/yt-dlp", new_ytdlp.clone());
    newer.ytdlp = newer.ytdlp.for_release(
        "2026.09.01",
        release_file(&server, "half2/yt-dlp", &new_ytdlp)?,
    );
    let new_zip = crate::media_support::zip(&[
        (
            &format!("ff/bin/{}", exe("ffmpeg")),
            Some(b"new ffmpeg".as_slice()),
        ),
        ("ff/bin/avcodec-63.dll", Some(b"new library".as_slice())),
    ])?;
    server.put("half2/ffmpeg.zip", new_zip.clone());
    newer.ffmpeg = newer
        .ffmpeg
        .for_release("n9.1", release_file(&server, "half2/ffmpeg.zip", &new_zip)?);
    let newer = std::sync::Arc::new(newer);
    *h.tweak_mut() = Some(std::sync::Arc::new(
        move |c: wirecatch_core::ManagerConfig| c.with_addon_sources((*newer).clone()),
    ));
    h.restart().await?;

    // ffmpeg's folder cannot be moved away, so only yt-dlp is swapped in.
    let dir = addon_dir(&h);
    let lock = Lock::hold(&dir.join("ffmpeg"), &format!("bin/{}", exe("ffmpeg")))?;
    let mut ui = h.manager.subscribe();
    h.manager.update_addon(AddonId::Media).await.unwrap();
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
    h.manager.update_addon(AddonId::Media).await.unwrap();
    wait_addon(&mut ui, "repaired", |a| a.state == AddonState::Installed).await;
    assert_eq!(
        std::fs::read(dir.join("ffmpeg").join("bin").join(exe("ffmpeg"))).unwrap(),
        b"new ffmpeg"
    );
    Ok(())
}

/// Each file's checksum is recorded at install. A file changed in place (same
/// size) is not looked for on every start, but a repair finds and replaces it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_repair_finds_a_file_changed_in_place() -> TestResult {
    let server = FileServer::start().await;
    let releases = Releases::fake()?;
    let mut h = harness(publish(&server, &releases, "tamper")?).await?;
    assert_eq!(install(&h).await?.state, AddonState::Installed);
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
    h.restart().await?;
    // Not checked on every start: sizes only.
    assert_eq!(h.manager.list_addons()[0].state, AddonState::Installed);

    let mut ui = h.manager.subscribe();
    h.manager.install_addon(AddonId::Media).await.unwrap();
    wait_addon(&mut ui, "repaired", |a| a.state == AddonState::Installed).await;
    assert_eq!(std::fs::read(&ytdlp).unwrap(), releases.ytdlp);
    Ok(())
}

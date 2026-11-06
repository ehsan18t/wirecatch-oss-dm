use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use wirecatch_engine::{
    CancellationToken, DownloadOptions, DownloadRequest, Error as EngineError, FileWork,
    PrivateNetwork, Progress, Url,
};
use wirecatch_media::ytdlp::Format;
use wirecatch_media::{MediaJob, MediaProgress, Pick};
use wirecatch_sites::local_network::ChosenBy;

use super::network::{format_rule, network_rule};
use super::page::{
    PartError, address_gone, fetch_parts, formats_ytdlp_takes, part_name, part_options, parts_end,
};
use super::placement::{self, Placed, final_path, place_noting, remember_placing};
use super::workdir;
use super::*;
use crate::headers::Origin;

/// A job on a website's page, for a test to change what it is about.
fn job() -> Job {
    Job {
        output: PathBuf::from("/v/out.mp4"),
        url: Url::parse("http://93.184.215.14/watch").unwrap(),
        origin: Origin::default(),
        chosen_by: ChosenBy::Website,
        tools: None,
        options: DownloadOptions::default(),
        temp_dir: PathBuf::new(),
        timeout: Duration::from_secs(1),
        refresh_delays: Arc::new([]),
        clock: Arc::new(crate::clock::SystemClock),
        readings: Arc::default(),
        work_owner: String::new(),
        quality: String::new(),
        reserve: Arc::new(|_: &Path, _| true),
        placing: Arc::new(|| {}),
        file_work: FileWork::new(),
        taken: Taken::KeepBoth,
    }
}

fn place(file: &Path, planned: &Path, reserve: &Reserve, taken: Taken) -> std::io::Result<Placed> {
    place_noting(file, planned, reserve, taken, &|_, _| Ok(()))
}

#[test]
fn a_typed_name_keeps_the_words_after_its_dot() {
    for (typed, wanted) in [
        ("/v/Dr. Who", "/v/Dr. Who.mp4"),
        ("/v/Episode 1.5 - Finale", "/v/Episode 1.5 - Finale.mp4"),
        ("/v/clip.webm", "/v/clip.mp4"),
        ("/v/clip.MP4", "/v/clip.MP4"),
    ] {
        assert_eq!(
            final_path(Path::new("/w/media.mp4"), Path::new(typed)),
            Path::new(wanted)
        );
    }
}

/// The page is read again for a refused or broken format address, whatever the person is told.
#[test]
fn a_refused_format_address_is_fetched_again_from_the_page() {
    let status = |status| EngineError::HttpStatus {
        status,
        url: "https://example.com/v.mp4".into(),
    };
    for gone in [401, 403, 404, 410] {
        assert!(address_gone(&status(gone)), "{gone}");
    }
    assert!(address_gone(&EngineError::TooManyRedirects {
        url: "https://example.com/v.mp4".into()
    }));
    for kept in [429, 500, 503] {
        assert!(!address_gone(&status(kept)), "{kept}");
    }
    assert!(!address_gone(&EngineError::Stalled {
        after: Duration::from_secs(30)
    }));
}

/// A row with no saved formats runs the best selection, so its total waits for each format that takes, not only the first to report.
#[test]
fn yt_dlps_formats_are_those_named_or_else_its_best_selection() {
    let info: wirecatch_media::ytdlp::Info = serde_json::from_value(serde_json::json!({
        "format_id": "137+140",
        "formats": [{"format_id": "137", "filesize": 100}, {"format_id": "140"}],
    }))
    .unwrap();
    assert_eq!(
        formats_ytdlp_takes(&info, &[]),
        [("137".to_owned(), Some(100)), ("140".to_owned(), None)]
    );
    assert_eq!(
        formats_ytdlp_takes(&info, &[&info.formats[1]]),
        [("140".to_owned(), None)]
    );
}

#[tokio::test]
async fn formats_of_a_page_on_the_internet_stay_on_the_internet() {
    let url = |u: &str| Url::parse(u).unwrap();
    let public_page = url("http://93.184.215.14/watch");
    // A page at home may point anywhere: one the person chose, or one on
    // their network by its own address (D39).
    for chosen_by in [ChosenBy::Person, ChosenBy::LocalPage] {
        assert_eq!(
            network_rule(
                chosen_by,
                &url("http://127.0.0.1:9/watch"),
                &url("http://192.168.1.10/v.mp4")
            )
            .await
            .unwrap(),
            PrivateNetwork::Allow
        );
    }
    // A page on the internet: public formats download with the local
    // network blocked for every hop and connection.
    assert_eq!(
        network_rule(
            ChosenBy::Website,
            &public_page,
            &url("http://93.184.215.15/v.mp4")
        )
        .await
        .unwrap(),
        PrivateNetwork::Block
    );
    for local in [
        "http://127.0.0.1:8080/v.mp4",
        "http://localhost/v.mp4",
        "http://10.0.0.1/v.mp4",
        "http://[::1]/v.mp4",
        "http://169.254.169.254/latest",
    ] {
        let error = network_rule(ChosenBy::Website, &public_page, &url(local))
            .await
            .unwrap_err();
        assert!(
            error.message.contains("your own network"),
            "{local}: {}",
            error.message
        );
    }
    // A name that does not resolve cannot be checked, so it is refused.
    let error = network_rule(
        ChosenBy::Website,
        &public_page,
        &url("http://no-such-host.invalid/v.mp4"),
    )
    .await
    .unwrap_err();
    assert!(
        error.message.contains("could not be found"),
        "{}",
        error.message
    );
}

/// A website's page gets no exception for being written as a local
/// address: whether the page is local was decided where it was captured,
/// from the page it came from (D39).
#[tokio::test]
async fn a_websites_page_is_never_local() {
    let url = |u: &str| Url::parse(u).unwrap();
    let error = network_rule(
        ChosenBy::Website,
        &url("http://127.0.0.1:9/watch"),
        &url("http://192.168.1.10/v.mp4"),
    )
    .await
    .unwrap_err();
    assert!(
        error.message.contains("your own network"),
        "{}",
        error.message
    );
}

/// Where a page's name resolves counts only for a page the person chose:
/// a website's owner decides what its name answers (CORE-3).
#[tokio::test]
async fn only_the_persons_page_is_local_by_its_name() {
    let url = |u: &str| Url::parse(u).unwrap();
    let page = url("http://localhost/watch");
    let format = url("http://192.168.1.10/v.mp4");
    assert_eq!(
        network_rule(ChosenBy::Person, &page, &format)
            .await
            .unwrap(),
        PrivateNetwork::Allow
    );
    let error = network_rule(ChosenBy::Website, &page, &format)
        .await
        .unwrap_err();
    assert!(
        error.message.contains("your own network"),
        "{}",
        error.message
    );
}

/// Core and the engine agree on what is local (CORE-2): a page on a
/// carrier-grade NAT address (a Tailscale media server) is local, and a
/// format at a NAT64 or 6to4 address that carries a local one is refused.
#[tokio::test]
async fn every_kind_of_local_address_counts_as_local() {
    let url = |u: &str| Url::parse(u).unwrap();
    assert_eq!(
        network_rule(
            ChosenBy::Person,
            &url("http://100.100.100.100/watch"),
            &url("http://192.168.1.10/v.mp4")
        )
        .await
        .unwrap(),
        PrivateNetwork::Allow
    );
    let public_page = url("http://93.184.215.14/watch");
    for local in [
        "http://[64:ff9b::c0a8:101]/v.mp4",
        "http://[2002:c0a8:101::1]/v.mp4",
        "http://100.64.0.1/v.mp4",
    ] {
        let error = network_rule(ChosenBy::Website, &public_page, &url(local))
            .await
            .unwrap_err();
        assert!(error.message.contains("your own network"), "{local}");
    }
}

/// A format's name that answered public to the early check and answers
/// local when the engine looks again is still refused, by the engine
/// under the rule core hands on: the early check only ever refuses. (The
/// refusal at the connection itself is the engine's own test,
/// `net_guard::tests::a_download_cannot_be_rebound_to_the_local_network`.)
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_format_rebound_to_the_local_network_after_the_check_is_refused() {
    let server = wirecatch_test_support::files::FileServer::start().await;
    server.put("v.mp4", b"video".as_slice());
    let mut format = server.url("v.mp4");
    format.set_host(Some("localhost")).unwrap();
    // What the check saw: a public address.
    let rule = format_rule(ChosenBy::Website, &["93.184.215.15".parse().unwrap()]).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let mut request = DownloadRequest::new(format, dir.path());
    request.options.private_network = rule;
    let result = wirecatch_engine::download(
        request,
        Arc::new(Progress::default()),
        CancellationToken::new(),
    )
    .await;
    assert!(
        matches!(result, Err(EngineError::PrivateNetwork { .. })),
        "{result:?}"
    );
    assert_eq!(server.requests("v.mp4"), 0);
}

/// A stream job runs under the rule of whoever chose its link.
#[test]
fn a_stream_downloads_under_the_rule_of_whoever_chose_it() {
    for (chosen_by, rule) in [
        (ChosenBy::Website, PrivateNetwork::Block),
        (ChosenBy::LocalPage, PrivateNetwork::Auto),
        (ChosenBy::Person, PrivateNetwork::Auto),
    ] {
        let job = Job {
            url: Url::parse("https://videos.example.com/master.m3u8").unwrap(),
            chosen_by,
            ..job()
        };
        let request = stream_request(&job, Path::new("/v/w/out"), Pick::Auto, Pick::Auto);
        assert_eq!(request.private_network, rule, "{chosen_by:?}");
    }
}

#[test]
fn a_format_downloads_under_its_network_rule() {
    let format: Format = serde_json::from_value(serde_json::json!({
        "format_id": "1", "ext": "mp4", "http_headers": {"X-Format": "yes"}
    }))
    .unwrap();
    let job = Job { ..job() };
    let options = part_options(&job, &format, PrivateNetwork::Block);
    assert_eq!(options.private_network, PrivateNetwork::Block);
    assert_eq!(options.headers.get("x-format").unwrap(), "yes");
    assert_eq!(options.max_request_bytes, None);

    // yt-dlp's chunk size becomes the engine's limit on one request.
    let chunked: Format = serde_json::from_value(serde_json::json!({
        "format_id": "137", "ext": "mp4",
        "downloader_options": {"http_chunk_size": 10_485_760}
    }))
    .unwrap();
    let options = part_options(&job, &chunked, PrivateNetwork::Block);
    assert_eq!(options.max_request_bytes, Some(10_485_760));
}

#[tokio::test]
async fn a_job_that_cannot_be_read_never_runs() {
    let job = Job {
        url: Url::parse("http://127.0.0.1:9/watch").unwrap(),
        ..job()
    };
    let progress = MediaProgress::default();
    let ended = run(
        job,
        MediaJob::Unreadable,
        &progress,
        &CancellationToken::new(),
    )
    .await;
    assert_eq!(ended, Err(wirecatch_media::unreadable()));
}

/// A stream (here one HLS segment, joined without ffmpeg) is saved in the
/// work folder, marked, and only then put at its final name: the final
/// file never exists without the mark.
#[cfg(windows)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_finished_stream_is_marked_before_it_is_put_in_place() {
    // MPEG-TS packets: 188 bytes each, starting with the sync byte.
    let segment: Vec<u8> = (0..188 * 40)
        .map(|i| {
            if i % 188 == 0 {
                0x47
            } else {
                u8::try_from(i % 251).unwrap()
            }
        })
        .collect();
    let server = wirecatch_test_support::files::FileServer::start().await;
    server.put(
        "index.m3u8",
        "#EXTM3U\n#EXT-X-VERSION:3\n#EXT-X-TARGETDURATION:4\n#EXTINF:4.0,\nseg0.ts\n#EXT-X-ENDLIST\n",
    );
    server.put("seg0.ts", segment);

    let dir = tempfile::tempdir().unwrap();
    let output = dir.path().join("clip.mp4");
    let job = Job {
        output: output.clone(),
        url: server.url("index.m3u8"),
        timeout: Duration::from_secs(5),
        ..job()
    };
    let done = run_stream(
        &job,
        Pick::Auto,
        Pick::Auto,
        &MediaProgress::default(),
        &CancellationToken::new(),
    )
    .await
    .unwrap();
    let crate::manager::run::MediaDone::Completed { path, .. } = done else {
        panic!("not completed: {done:?}");
    };
    assert_eq!(path.parent(), output.parent(), "{}", path.display());
    let mark = std::fs::read_to_string(format!("{}:Zone.Identifier", path.display()))
        .expect("the stream was put in place without the mark");
    assert!(mark.contains("ZoneId=3"), "{mark}");
    assert!(
        !wirecatch_media::work_dir(&output).exists(),
        "the work folder was left"
    );
}

/// The finished file carries the Mark of the Web the moment it is in place.
#[cfg(windows)]
#[tokio::test]
async fn a_finished_video_is_marked_before_it_is_put_in_place() {
    let dir = tempfile::tempdir().unwrap();
    let work = dir.path().join("out.mp4.wcmedia");
    std::fs::create_dir_all(&work).unwrap();
    let part = work.join("video.mp4");
    std::fs::write(&part, b"video").unwrap();
    let job = Job {
        output: dir.path().join("out.mp4"),
        url: Url::parse("https://videos.example/watch?v=1&token=secret").unwrap(),
        ..job()
    };
    let done = finish(&part, &job, &work, &[]).await.unwrap();
    let crate::manager::run::MediaDone::Completed { path, .. } = done else {
        panic!("not completed: {done:?}");
    };
    let mark = std::fs::read_to_string(format!("{}:Zone.Identifier", path.display()))
        .expect("no mark when the file was put in place");
    assert!(
        mark.contains("HostUrl=https://videos.example/watch\r\n"),
        "{mark}"
    );
    assert!(!mark.contains("secret"));
}

/// A file left waiting is taken only by the download it came from, and only from its own work
/// folder.
#[tokio::test]
async fn a_waiting_file_belongs_to_its_link() {
    let dir = tempfile::tempdir().unwrap();
    let work = dir.path().join("work");
    std::fs::create_dir(&work).unwrap();
    let file = work.join("clip.ts");
    std::fs::write(&file, b"clip").unwrap();
    let mine = Url::parse("https://example.com/a.m3u8").unwrap();
    remember_placing(&work, mine.as_str(), &file, &[]).unwrap();
    assert_eq!(waiting_to_place(&work, &mine).await, Some((file, vec![])));
    let other = Url::parse("https://example.com/b.m3u8").unwrap();
    assert_eq!(waiting_to_place(&work, &other).await, None);

    std::fs::write(dir.path().join("outside.ts"), b"x").unwrap();
    std::fs::write(
        work.join(workdir::PLACING),
        format!("{}\n../outside.ts", placement::link_digest(mine.as_str())),
    )
    .unwrap();
    assert_eq!(waiting_to_place(&work, &mine).await, None);
}

#[test]
fn keeping_both_never_replaces_a_file_or_takes_a_reserved_name() {
    let dir = tempfile::tempdir().unwrap();
    let planned = dir.path().join("out.mp4");
    std::fs::write(&planned, b"someone else's").unwrap();
    let file = dir.path().join("video.mp4");
    std::fs::write(&file, b"video").unwrap();
    let free: Reserve = Arc::new(|_: &Path, _| true);
    let placed = place(&file, &planned, &free, Taken::KeepBoth).unwrap();
    let next = dir.path().join("out (1).mp4");
    assert_eq!(placed, Placed::At(next.clone()));
    assert_eq!(std::fs::read(&planned).unwrap(), b"someone else's");
    assert_eq!(std::fs::read(&next).unwrap(), b"video");
    assert!(!file.exists());

    // Another extension: a name another download holds is skipped.
    let file = dir.path().join("video.webm");
    std::fs::write(&file, b"webm").unwrap();
    let held = dir.path().join("out.webm");
    let reserve: Reserve = Arc::new(move |p: &Path, _| p != held);
    let placed = place(&file, &planned, &reserve, Taken::KeepBoth).unwrap();
    assert_eq!(placed, Placed::At(dir.path().join("out (1).webm")));
}

/// A video that finds itself in another format may land on a name another download holds, which neither replaces nor keeps.
#[test]
fn replacing_or_keeping_skips_a_name_another_download_holds() {
    for taken in [Taken::Replace, Taken::KeepExisting] {
        let dir = tempfile::tempdir().unwrap();
        let planned = dir.path().join("out.mp4");
        let held = dir.path().join("out.webm");
        std::fs::write(&held, b"someone else's").unwrap();
        let file = dir.path().join("video.webm");
        std::fs::write(&file, b"webm").unwrap();
        let refused = held.clone();
        let reserve: Reserve = Arc::new(move |p: &Path, _| p != refused);
        let placed = place(&file, &planned, &reserve, taken).unwrap();
        let next = dir.path().join("out (1).webm");
        assert_eq!(placed, Placed::At(next.clone()));
        assert_eq!(std::fs::read(&held).unwrap(), b"someone else's");
        assert_eq!(std::fs::read(&next).unwrap(), b"webm");
    }
}

/// A taken name does what the setting says, as a file download does (D96).
#[test]
fn a_taken_name_is_replaced_kept_or_asked_about() {
    let dir = tempfile::tempdir().unwrap();
    let planned = dir.path().join("out.mp4");
    let free: Reserve = Arc::new(|_: &Path, _| true);
    let finished = |content: &[u8]| {
        let file = dir.path().join("video.mp4");
        std::fs::write(&file, content).unwrap();
        file
    };
    std::fs::write(&planned, b"older").unwrap();

    let file = finished(b"newer");
    let placed = place(&file, &planned, &free, Taken::Ask).unwrap();
    let Placed::Asks(found) = placed else {
        panic!("not asked: {placed:?}");
    };
    assert_eq!(
        (found.path.as_path(), found.size, found.same),
        (planned.as_path(), 5, true)
    );
    assert!(file.exists(), "the new file waits for the answer");
    let other = finished(b"different size");
    let Placed::Asks(found) = place(&other, &planned, &free, Taken::Ask).unwrap() else {
        panic!("not asked");
    };
    assert!(!found.same);

    let placed = place(&other, &planned, &free, Taken::KeepExisting).unwrap();
    assert_eq!(
        placed,
        Placed::Kept {
            path: planned.clone(),
            size: 5
        }
    );
    assert_eq!(std::fs::read(&planned).unwrap(), b"older");
    assert!(!other.exists());

    let file = finished(b"newer");
    let placed = place(&file, &planned, &free, Taken::Replace).unwrap();
    assert_eq!(placed, Placed::At(planned.clone()));
    assert_eq!(std::fs::read(&planned).unwrap(), b"newer");
}

#[test]
fn parts_get_media_extensions() {
    let format = |ext: &str, vcodec: &str| -> Format {
        serde_json::from_value(serde_json::json!({
            "format_id": "1", "ext": ext, "vcodec": vcodec, "acodec": "mp4a"
        }))
        .unwrap()
    };
    assert_eq!(part_name(&format("exe", "avc1"), "video"), "video.mp4");
    assert_eq!(part_name(&format("lnk", "none"), "audio"), "audio.m4a");
    assert_eq!(part_name(&format("webm", "vp9"), "video"), "video.webm");
}

#[test]
fn a_file_keeps_its_extension_only_when_it_is_a_media_one() {
    let planned = Path::new("/v/Invoice.mp4");
    assert_eq!(
        final_path(Path::new("/w/media.webm"), planned),
        Path::new("/v/Invoice.webm")
    );
    assert_eq!(
        final_path(Path::new("/w/media.MKV"), planned),
        Path::new("/v/Invoice.mkv")
    );
    for bad in [
        "media.exe",
        "media.bat",
        "media.lnk",
        "media",
        "media.mp4.scr",
    ] {
        assert_eq!(
            final_path(&Path::new("/w").join(bad), planned),
            planned,
            "{bad}"
        );
    }
}

/// A small file past its limit is refused before any of it is written, however fast it arrives.
#[tokio::test]
async fn a_small_file_past_its_limit_is_never_written() {
    use wirecatch_test_support::http::{Config, TestServer};
    let server = TestServer::start(vec![b'x'; 64 * 1024], Config::default()).await;
    let dir = tempfile::tempdir().unwrap();
    let job = Job {
        output: dir.path().join("out.mp4"),
        url: server.url("/watch"),
        chosen_by: ChosenBy::Person,
        temp_dir: dir.path().to_path_buf(),
        ..job()
    };
    let path = dir.path().join("sub-0.vtt");
    let fetched = super::extras::fetch_small(
        &job,
        &server.url("/sub.vtt"),
        &std::collections::HashMap::new(),
        &path,
        1024,
        &CancellationToken::new(),
    )
    .await;
    assert!(matches!(fetched, super::extras::Fetch::Failed(_)));
    assert!(!path.exists(), "the file past its limit was written");
}

/// A scanner often holds a finished file without sharing its deletion or its writing. The video still reaches its place; what stays behind goes with the work folder.
#[cfg(windows)]
#[test]
fn a_video_a_scanner_holds_is_still_put_in_place() {
    use std::os::windows::fs::OpenOptionsExt;
    const FILE_SHARE_READ: u32 = 1;
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("media.mp4");
    std::fs::write(&file, b"video").unwrap();
    let _scanner = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ)
        .open(&file)
        .unwrap();
    let planned = dir.path().join("out").join("video.mp4");
    std::fs::create_dir_all(planned.parent().unwrap()).unwrap();
    let reserve: super::placement::Reserve = Arc::new(|_: &Path, _| true);
    wirecatch_files::whole_file::flush(&file).unwrap();
    let placed = place(&file, &planned, &reserve, super::placement::Taken::KeepBoth).unwrap();
    assert_eq!(placed, Placed::At(planned.clone()));
    assert_eq!(std::fs::read(&planned).unwrap(), b"video");
}

fn job_at(dir: &Path, output: &Path, url: &str, taken: Taken) -> Job {
    let tools: wirecatch_media::MediaTools = wirecatch_addons::ToolPaths {
        ytdlp: dir.join("no-such-yt-dlp"),
        ffmpeg: dir.join("no-such-ffmpeg"),
        deno: dir.join("no-such-deno"),
        ytdlp_cache: dir.join("cache"),
    }
    .into();
    Job {
        output: output.to_path_buf(),
        url: Url::parse(url).unwrap(),
        chosen_by: ChosenBy::Person,
        tools: Some(tools),
        temp_dir: dir.to_path_buf(),
        taken,
        ..job()
    }
}

/// A list another download left in the work folder of the same name is never saved as this download.
#[tokio::test]
async fn a_new_link_does_not_take_a_list_another_link_left() {
    let dir = tempfile::tempdir().unwrap();
    let output = dir.path().join("Clip.mp4");
    std::fs::write(&output, b"the person's file").unwrap();
    let work = wirecatch_media::work_dir(&output);
    std::fs::create_dir(&work).unwrap();
    let old = work.join("video.mp4");
    std::fs::write(&old, b"the earlier download's video").unwrap();
    workdir::Downloaded::of_media(std::slice::from_ref(&old))
        .write(&work)
        .await
        .unwrap();
    let job = job_at(
        dir.path(),
        &output,
        "https://example.com/another-video",
        Taken::Replace,
    );
    let what = MediaJob::Page {
        video: None,
        audio: None,
        output: None,
        keep_ext: None,
        conversion_failed: false,
    };
    let end = run(
        job,
        what,
        &MediaProgress::default(),
        &CancellationToken::new(),
    )
    .await;
    assert_eq!(
        std::fs::read(&output).unwrap(),
        b"the person's file",
        "completed as {end:?} with another link's video"
    );
    assert!(!old.exists());
}

/// Parts in the work folder belong to the link that left them: its next run keeps them, and another link's run never sees them.
#[tokio::test]
async fn a_work_folder_keeps_its_parts_only_for_its_own_link() {
    let dir = tempfile::tempdir().unwrap();
    let work = dir.path().join("Clip.mp4.wcmedia");
    let part = work.join(workdir::marker("video"));
    let link = |u: &str| Url::parse(u).unwrap();
    workdir::claim_for_link(&work, &link("https://example.com/first"))
        .await
        .unwrap();
    std::fs::write(&part, "video.mp4").unwrap();
    workdir::claim_for_link(&work, &link("https://example.com/first"))
        .await
        .unwrap();
    assert!(part.exists());
    workdir::claim_for_link(&work, &link("https://example.com/second"))
        .await
        .unwrap();
    assert!(!part.exists());
    assert!(work.is_dir());
}

/// A folder that names no link, as one a crash left early, belongs to no one.
#[tokio::test]
async fn a_work_folder_that_names_no_link_is_cleared() {
    let dir = tempfile::tempdir().unwrap();
    let work = dir.path().join("Clip.mp4.wcmedia");
    std::fs::create_dir(&work).unwrap();
    let part = work.join(workdir::marker("video"));
    std::fs::write(&part, "video.mp4").unwrap();
    workdir::claim_for_link(&work, &Url::parse("https://example.com/v").unwrap())
        .await
        .unwrap();
    assert!(!part.exists());
}

/// A refreshed stream may come from another host of its site and keeps its segments; another site's never does.
#[test]
fn a_stream_is_owned_by_its_site_and_port() {
    let suffixes = wirecatch_sites::public_suffix::SuffixList::built_in();
    let owner = |u: &str| workdir::stream_owner(&Url::parse(u).unwrap(), &suffixes);
    let first = owner("https://cdn1.example.com/hls/master.m3u8?token=a");
    assert_eq!(
        first,
        owner("https://cdn2.example.com/hls/master.m3u8?token=b")
    );
    assert_ne!(
        first,
        owner("https://cdn1.example.org/hls/master.m3u8?token=a")
    );
    assert_ne!(
        first,
        owner("https://cdn1.example.com:8443/hls/master.m3u8?token=a")
    );
}

/// The request carries the owner the job worked out, not the host the stream crate would default to.
#[test]
fn a_stream_request_names_the_jobs_owner() {
    let dir = tempfile::tempdir().unwrap();
    let output = dir.path().join("Clip.mp4");
    let mut job = job_at(
        dir.path(),
        &output,
        "https://cdn1.example.com/m.m3u8",
        Taken::Replace,
    );
    job.work_owner = "https://example.com:443".to_owned();
    let request = stream_request(&job, &output, Pick::Auto, Pick::Auto);
    assert_eq!(request.work_owner, "https://example.com:443");
}

/// A part says only its role, so what one set of formats finished is never taken for another's, whichever way the page changes back.
#[tokio::test]
async fn parts_are_kept_only_for_the_formats_that_wrote_them() {
    let dir = tempfile::tempdir().unwrap();
    let work = dir.path().join("Clip.mp4.wcmedia");
    let part = work.join(workdir::marker("video"));
    let link = Url::parse("https://example.com/v").unwrap();
    workdir::claim_for_link(&work, &link).await.unwrap();
    std::fs::write(&part, "video.mp4").unwrap();

    // The saved formats stay listed: the attempt goes on with its parts.
    workdir::claim_for_formats(&work, &link, None)
        .await
        .unwrap();
    assert!(part.exists());

    workdir::claim_for_formats(&work, &link, Some("299+140"))
        .await
        .unwrap();
    assert!(!part.exists());
    std::fs::write(&part, "video.mp4").unwrap();
    workdir::claim_for_formats(&work, &link, Some("299+140"))
        .await
        .unwrap();
    assert!(part.exists());

    // The page lists the saved formats again: the replacement's part is not theirs.
    workdir::claim_for_formats(&work, &link, None)
        .await
        .unwrap();
    assert!(!part.exists());
    assert!(work.is_dir());
}

const LINK: &str = "https://example.com/v";

/// A format that server `at` answers from.
fn format_at(id: &str, url: &Url) -> Format {
    serde_json::from_value(serde_json::json!({ "format_id": id, "url": url.as_str() })).unwrap()
}

/// Fetches `found` under a job that gives up on the first stall, and gives what the person is told.
async fn failure_of_parts(found: &[Format]) -> String {
    let dir = tempfile::tempdir().unwrap();
    let work = dir.path().join("Clip.mp4.wcmedia");
    std::fs::create_dir(&work).unwrap();
    let mut job = job_at(dir.path(), &dir.path().join("Clip.mp4"), LINK, Taken::Ask);
    job.options.stall_timeout = Duration::from_millis(100);
    job.options.max_retries = 0;
    job.chosen_by = ChosenBy::Person;
    // A page on the same machine as its formats, which it may then reach.
    job.url = Url::parse("http://127.0.0.1/watch").unwrap();
    let found: Vec<&Format> = found.iter().collect();
    let progress = MediaProgress::default();
    let ended = fetch_parts(&job, &found, &work, &progress, &CancellationToken::new()).await;
    match ended {
        Err(PageError::Failed(error)) => error.message,
        Err(PageError::Refused { status }) => panic!("refused: {status:?}"),
        Ok(files) => panic!("not a failure: {files:?}"),
    }
}

/// A server that holds every body: its part stalls before its first byte.
async fn stalling_server(ranges: bool) -> wirecatch_test_support::http::TestServer {
    wirecatch_test_support::http::TestServer::start(
        wirecatch_test_support::test_data(1024),
        wirecatch_test_support::http::Config {
            ranges,
            hold_bodies_from: Some(0),
            ..Default::default()
        },
    )
    .await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_part_that_stalls_before_its_first_byte_is_not_promised_a_kept_part() {
    let stalling = stalling_server(true).await;
    let told = failure_of_parts(&[format_at("137", &stalling.url("v.mp4"))]).await;
    assert!(told.contains("stopped sending data"), "{told}");
    assert!(!told.contains("is kept"), "{told}");
}

/// The part that fails first is worded by what the other, which ended after it, leaves behind.
#[test]
fn a_failure_promises_the_part_that_finished_after_it() {
    let dir = tempfile::tempdir().unwrap();
    let job = job_at(dir.path(), &dir.path().join("Clip.mp4"), LINK, Taken::Ask);
    let progress = MediaProgress::default();
    progress.add_done(1024);
    let stalled = EngineError::Stalled {
        after: Duration::from_secs(1),
    };
    let results = vec![
        Err(PartError::Transfer(stalled)),
        Ok(Some(dir.path().join("video.mp4"))),
    ];
    let Err(PageError::Failed(told)) = parts_end(&job, results, &progress) else {
        panic!("not a failure");
    };
    assert!(told.message.contains("stopped sending data"), "{told:?}");
    assert!(told.message.contains("is kept"), "{told:?}");
}

/// A work folder holding a finished `Clip.mp4` beside its planned path.
fn finished_in_work(dir: &Path) -> (PathBuf, PathBuf, PathBuf) {
    let output = dir.join("Clip.mp4");
    let work = wirecatch_media::work_dir(&output);
    std::fs::create_dir(&work).unwrap();
    let file = work.join("Clip.mp4");
    std::fs::write(&file, b"the video").unwrap();
    (output, work, file)
}

/// What a next run does with nothing to download: any fetch would end in the unreadable job's error.
async fn next_run(job: Job) -> Result<MediaDone, UserError> {
    run(
        job,
        MediaJob::Unreadable,
        &MediaProgress::default(),
        &CancellationToken::new(),
    )
    .await
}

/// A file that could not be put in place, as when a player holds the old one, waits there: the next run places it and fetches nothing.
#[tokio::test]
async fn a_file_that_could_not_be_placed_waits_for_the_next_run() {
    let dir = tempfile::tempdir().unwrap();
    let (output, work, file) = finished_in_work(dir.path());
    std::fs::write(&output, b"the person's file").unwrap();
    let mut job = job_at(dir.path(), &output, LINK, Taken::KeepBoth);
    job.reserve = Arc::new(|_: &Path, _| false);
    assert!(placement::finish(&file, &job, &work, &[]).await.is_err());

    let job = job_at(dir.path(), &output, LINK, Taken::KeepBoth);
    let end = next_run(job).await;
    let Ok(MediaDone::Completed { path, .. }) = end else {
        panic!("{end:?}");
    };
    assert_eq!(std::fs::read(&path).unwrap(), b"the video");
    assert_eq!(std::fs::read(&output).unwrap(), b"the person's file");
    assert!(!work.exists());
}

/// Dying after the link to a free name and before the work folder went leaves the file in both places: the next run neither copies it again nor asks about its own file.
#[tokio::test]
async fn a_crash_after_linking_does_not_make_a_second_copy() {
    let dir = tempfile::tempdir().unwrap();
    let (output, work, file) = finished_in_work(dir.path());
    remember_placing(&work, LINK, &file, &[]).unwrap();
    placement::note_move(&work, placement::Moving::ToFreeName, &output).unwrap();
    std::fs::hard_link(&file, &output).unwrap();
    let job = job_at(dir.path(), &output, LINK, Taken::Ask);
    let end = next_run(job).await;
    let Ok(MediaDone::Completed { path, .. }) = end else {
        panic!("{end:?}");
    };
    assert_eq!(path, output);
    assert!(!work.exists());
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
}

/// Dying once the file left the work folder: the note says where it went, and the row completes there.
#[tokio::test]
async fn a_crash_after_moving_completes_at_the_noted_name() {
    let dir = tempfile::tempdir().unwrap();
    let (output, work, file) = finished_in_work(dir.path());
    let other = dir.path().join("Clip (1).mp4");
    remember_placing(&work, LINK, &file, &[]).unwrap();
    placement::note_move(&work, placement::Moving::ToFreeName, &other).unwrap();
    std::fs::rename(&file, &other).unwrap();
    let end = next_run(job_at(dir.path(), &output, LINK, Taken::KeepBoth)).await;
    let Ok(MediaDone::Completed { path, .. }) = end else {
        panic!("{end:?}");
    };
    assert_eq!(path, other);
    assert!(!work.exists());
}

/// `Clip.mp4` placed with its note, and `subtitle-0.srt` in the work folder ready to follow it as English.
fn subtitle_waiting(dir: &Path) -> (PathBuf, PathBuf, PathBuf, PathBuf) {
    let (output, work, file) = finished_in_work(dir);
    let subtitle = work.join(workdir::converted_subtitle(0));
    std::fs::write(&subtitle, b"words").unwrap();
    let sidecars = [(subtitle.clone(), "en".to_owned())];
    remember_placing(&work, LINK, &file, &sidecars).unwrap();
    placement::note_move(&work, placement::Moving::ToFreeName, &output).unwrap();
    std::fs::rename(&file, &output).unwrap();
    let beside = dir.join("Clip.en.srt");
    (output, work, subtitle, beside)
}

/// The subtitles listed on the row a next run completes.
async fn placed_subtitles(dir: &Path, output: &Path) -> Vec<PathBuf> {
    let end = next_run(job_at(dir, output, LINK, Taken::KeepBoth)).await;
    let Ok(MediaDone::Completed { sidecars, .. }) = end else {
        panic!("{end:?}");
    };
    sidecars
}

/// Dying after a subtitle's link to its name and before its discard leaves it in both places, and keeping both would give `Clip.en (1).srt`.
#[tokio::test]
async fn a_crash_after_linking_a_subtitle_does_not_make_a_second_copy() {
    let dir = tempfile::tempdir().unwrap();
    let (output, work, subtitle, beside) = subtitle_waiting(dir.path());
    let name = crate::naming::file_name_of(&subtitle);
    placement::note_subtitle_move(&work, &name, placement::Moving::ToFreeName, &beside).unwrap();
    std::fs::hard_link(&subtitle, &beside).unwrap();

    assert_eq!(placed_subtitles(dir.path(), &output).await, [beside]);
    assert!(!work.exists());
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 2);
}

/// Dying once the subtitle left the work folder: the note says where it went, so the row still lists it.
#[tokio::test]
async fn a_crash_after_moving_a_subtitle_lists_it_at_the_noted_name() {
    let dir = tempfile::tempdir().unwrap();
    let (output, work, subtitle, beside) = subtitle_waiting(dir.path());
    let name = crate::naming::file_name_of(&subtitle);
    placement::note_subtitle_move(&work, &name, placement::Moving::ToFreeName, &beside).unwrap();
    std::fs::rename(&subtitle, &beside).unwrap();

    assert_eq!(placed_subtitles(dir.path(), &output).await, [beside]);
}

/// A note left by a move that failed says nothing of a file that later took that name: that file stays, and the subtitle takes a free name.
#[tokio::test]
async fn a_subtitles_note_never_claims_another_file_at_that_name() {
    let dir = tempfile::tempdir().unwrap();
    let (output, work, subtitle, beside) = subtitle_waiting(dir.path());
    let name = crate::naming::file_name_of(&subtitle);
    placement::note_subtitle_move(&work, &name, placement::Moving::ToFreeName, &beside).unwrap();
    std::fs::write(&beside, b"someone else's").unwrap();

    let placed = placed_subtitles(dir.path(), &output).await;
    assert_eq!(placed, [dir.path().join("Clip.en (1).srt")]);
    assert_eq!(std::fs::read(&placed[0]).unwrap(), b"words");
    assert_eq!(std::fs::read(&beside).unwrap(), b"someone else's");
}

/// Dying just before replacing: the old file is still at the name, so the replace still happens.
#[tokio::test]
async fn a_crash_before_replacing_still_replaces() {
    let dir = tempfile::tempdir().unwrap();
    let (output, work, file) = finished_in_work(dir.path());
    std::fs::write(&output, b"the old one").unwrap();
    remember_placing(&work, LINK, &file, &[]).unwrap();
    placement::note_move(&work, placement::Moving::OntoTakenName, &output).unwrap();
    let end = next_run(job_at(dir.path(), &output, LINK, Taken::Replace)).await;
    assert!(matches!(end, Ok(MediaDone::Completed { .. })), "{end:?}");
    assert_eq!(std::fs::read(&output).unwrap(), b"the video");
}

/// A note left by a move that failed says nothing of a file that later took that name: same size, other bytes, and ours is not thrown away for it.
#[tokio::test]
async fn a_note_never_claims_a_file_that_only_has_the_same_size() {
    let dir = tempfile::tempdir().unwrap();
    let (output, work, file) = finished_in_work(dir.path());
    std::fs::write(&output, b"not a video").unwrap();
    remember_placing(&work, LINK, &file, &[]).unwrap();
    placement::note_move(&work, placement::Moving::ToFreeName, &output).unwrap();
    let end = next_run(job_at(dir.path(), &output, LINK, Taken::KeepBoth)).await;
    let Ok(MediaDone::Completed { path, .. }) = end else {
        panic!("{end:?}");
    };
    assert_ne!(path, output);
    assert_eq!(std::fs::read(&path).unwrap(), b"the video");
    assert_eq!(std::fs::read(&output).unwrap(), b"not a video");
}

/// Both the file and the name it went to gone: nothing waits to be placed, so the next run starts again rather than failing on a missing file each time.
#[tokio::test]
async fn a_file_gone_from_both_places_does_not_wait() {
    let dir = tempfile::tempdir().unwrap();
    let (output, work, file) = finished_in_work(dir.path());
    remember_placing(&work, LINK, &file, &[]).unwrap();
    placement::note_move(&work, placement::Moving::ToFreeName, &output).unwrap();
    std::fs::remove_file(&file).unwrap();
    let link = Url::parse(LINK).unwrap();
    assert_eq!(placement::waiting_to_place(&work, &link).await, None);
}

/// What is noted is on disk before the file moves, whichever way it moves, or a crash in between would be read as no move at all.
#[test]
fn every_move_is_noted_before_it_happens() {
    for (taken, how) in [
        (Taken::KeepBoth, placement::Moving::ToFreeName),
        (Taken::Replace, placement::Moving::OntoTakenName),
        (Taken::KeepExisting, placement::Moving::OntoTakenName),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let (output, _, file) = finished_in_work(dir.path());
        if how == placement::Moving::OntoTakenName {
            std::fs::write(&output, b"the old one").unwrap();
        }
        let notes = std::cell::Cell::new(0);
        let note = |noted: placement::Moving, to: &Path| {
            assert_eq!(noted, how);
            assert_eq!(to, output);
            assert!(file.is_file(), "moved before the note");
            notes.set(notes.get() + 1);
            Ok(())
        };
        let free: Reserve = Arc::new(|_: &Path, _| true);
        place_noting(&file, &output, &free, taken, &note).unwrap();
        assert_eq!(notes.get(), 1, "{taken:?}");
    }
}

/// A name reserved for a move that could not be noted goes back, or no other download could ever have it.
#[test]
fn a_move_that_could_not_be_noted_gives_its_name_back() {
    let dir = tempfile::tempdir().unwrap();
    let (output, _, file) = finished_in_work(dir.path());
    std::fs::write(&output, b"another").unwrap();
    let given_back = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counted = given_back.clone();
    let reserve: Reserve = Arc::new(move |_: &Path, take: bool| {
        if !take {
            counted.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
        true
    });
    let note = |_: placement::Moving, _: &Path| Err(std::io::Error::other("no room"));
    assert!(place_noting(&file, &output, &reserve, Taken::KeepBoth, &note).is_err());
    assert_eq!(given_back.load(std::sync::atomic::Ordering::Relaxed), 1);
    assert!(file.is_file());
}

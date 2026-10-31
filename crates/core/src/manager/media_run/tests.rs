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
use super::page::{address_gone, part_name, part_options};
use super::placement::{Placed, final_path, place, remember_placing};
use super::workdir;
use super::*;
use crate::headers::Origin;

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
            output: PathBuf::from("/v/out.mp4"),
            url: Url::parse("https://videos.example.com/master.m3u8").unwrap(),
            origin: Origin::default(),
            chosen_by,
            tools: None,
            options: DownloadOptions::default(),
            temp_dir: PathBuf::new(),
            timeout: Duration::from_secs(1),
            refresh_delays: Arc::new([]),
            clock: Arc::new(crate::clock::SystemClock),
            reserve: Arc::new(|_: &Path, _| true),
            file_work: FileWork::new(),
            taken: Taken::KeepBoth,
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
    let job = Job {
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
        reserve: Arc::new(|_: &Path, _| true),
        file_work: FileWork::new(),
        taken: Taken::KeepBoth,
    };
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
        output: PathBuf::from("/v/out.mp4"),
        url: Url::parse("http://127.0.0.1:9/watch").unwrap(),
        origin: Origin::default(),
        chosen_by: ChosenBy::Website,
        tools: None,
        options: DownloadOptions::default(),
        temp_dir: PathBuf::new(),
        timeout: Duration::from_secs(1),
        refresh_delays: Arc::new([]),
        clock: Arc::new(crate::clock::SystemClock),
        reserve: Arc::new(|_: &Path, _| true),
        file_work: FileWork::new(),
        taken: Taken::KeepBoth,
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
        origin: Origin::default(),
        chosen_by: ChosenBy::Website,
        tools: None,
        options: DownloadOptions::default(),
        temp_dir: PathBuf::new(),
        timeout: Duration::from_secs(5),
        refresh_delays: Arc::new([]),
        clock: Arc::new(crate::clock::SystemClock),
        reserve: Arc::new(|_: &Path, _| true),
        file_work: FileWork::new(),
        taken: Taken::KeepBoth,
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
        origin: Origin::default(),
        chosen_by: ChosenBy::Website,
        tools: None,
        options: DownloadOptions::default(),
        temp_dir: PathBuf::new(),
        timeout: Duration::from_secs(1),
        refresh_delays: Arc::new([]),
        clock: Arc::new(crate::clock::SystemClock),
        reserve: Arc::new(|_: &Path, _| true),
        file_work: FileWork::new(),
        taken: Taken::KeepBoth,
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
        format!(
            "{mine}
../outside.ts"
        ),
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
        origin: Origin::default(),
        chosen_by: ChosenBy::Person,
        tools: None,
        options: DownloadOptions::default(),
        temp_dir: dir.path().to_path_buf(),
        timeout: Duration::from_secs(1),
        refresh_delays: Arc::new([]),
        clock: Arc::new(crate::clock::SystemClock),
        reserve: Arc::new(|_: &Path, _| true),
        file_work: FileWork::new(),
        taken: Taken::KeepBoth,
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

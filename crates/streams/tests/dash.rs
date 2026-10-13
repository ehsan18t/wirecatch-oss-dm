//! DASH downloads end to end, from a local server.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use wirecatch_engine::{CancellationToken, DownloadOptions};
use wirecatch_streams::{
    Choice, DrmSystem, Error, Feature, ManifestError, Outcome, PrivateNetwork, StreamKind,
    StreamProgress, StreamRequest, download, inspect,
};
use wirecatch_test_support::files::FileServer;
use wirecatch_test_support::{TestResult, private_dir, ts_segment};

const DASH: &str = "application/dash+xml";

fn options() -> DownloadOptions {
    let mut options = DownloadOptions::default();
    options.stall_timeout = Duration::from_secs(5);
    options.max_retries = 2;
    options
}

fn request(server: &FileServer, manifest: &str, output: &Path) -> StreamRequest {
    let mut request = StreamRequest::new(server.url(manifest), output);
    request.options = options();
    request
}

async fn run(request: StreamRequest) -> Result<Outcome, Error> {
    download(
        request,
        Arc::new(StreamProgress::default()),
        CancellationToken::new(),
    )
    .await
}

/// The bytes of the file a completed download saved.
fn saved(outcome: Outcome) -> TestResult<Vec<u8>> {
    match outcome {
        Outcome::Completed { path, .. } => Ok(std::fs::read(path)?),
        other => Err(format!("expected completion, got {other:?}").into()),
    }
}

/// An fMP4-looking init section, so the joined track is saved as `.mp4`.
fn init(seed: u64) -> Vec<u8> {
    let mut data = b"\0\0\0\x10ftypiso6\0\0\0\0".to_vec();
    data.extend(ts_segment(seed, 300));
    data
}

#[tokio::test]
async fn number_templates_download_the_chosen_representation() {
    let server = FileServer::start().await;
    let mut expected = std::collections::HashMap::new();
    for rep in ["v1", "v2", "a1"] {
        let mut bytes = init(rep.len() as u64 + u64::from(rep.as_bytes()[1]));
        server.put(&format!("dash/{rep}/init.mp4"), bytes.clone());
        // 10 s in 4 s segments: three, numbered from 1, with a width of 3.
        for n in 1..=3u64 {
            let data = ts_segment(n * 7 + u64::from(rep.as_bytes()[1]), 2000);
            server.put(&format!("dash/{rep}/seg-{n:03}.m4s"), data.clone());
            bytes.extend(data);
        }
        expected.insert(rep, bytes);
    }
    server.put_typed(
        "dash/manifest.mpd",
        r#"<?xml version="1.0" encoding="UTF-8"?>
<MPD xmlns="urn:mpeg:dash:schema:mpd:2011" type="static" mediaPresentationDuration="PT10S" profiles="urn:mpeg:dash:profile:isoff-live:2011">
  <Period>
    <AdaptationSet contentType="video" mimeType="video/mp4">
      <SegmentTemplate timescale="90000" duration="360000" initialization="$RepresentationID$/init.mp4" media="$RepresentationID$/seg-$Number%03d$.m4s"/>
      <Representation id="v1" bandwidth="400000" width="640" height="360" codecs="avc1.4d401e" frameRate="25"/>
      <Representation id="v2" bandwidth="2500000" width="1280" height="720" codecs="avc1.4d401f" frameRate="25"/>
    </AdaptationSet>
    <AdaptationSet contentType="audio" mimeType="audio/mp4" lang="en">
      <Representation id="a1" bandwidth="96000" codecs="mp4a.40.2">
        <SegmentTemplate timescale="48000" duration="192000" initialization="$RepresentationID$/init.mp4" media="$RepresentationID$/seg-$Number%03d$.m4s"/>
      </Representation>
    </AdaptationSet>
    <AdaptationSet contentType="text" mimeType="text/vtt" lang="es">
      <Representation id="t1" bandwidth="100"><BaseURL>subs.vtt</BaseURL></Representation>
    </AdaptationSet>
  </Period>
</MPD>"#,
        DASH,
    );

    let info = inspect(
        &server.url("dash/manifest.mpd"),
        &options(),
        CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(info.kind, StreamKind::Dash);
    assert!(!info.live);
    assert_eq!(info.duration, Some(Duration::from_secs(10)));
    assert_eq!(info.video.len(), 2);
    assert_eq!(info.video[1].height, Some(720));
    assert_eq!(info.audio.len(), 1);
    assert_eq!(info.audio[0].language.as_deref(), Some("en"));
    assert_eq!(info.subtitles.len(), 1);
    assert_eq!(info.extension(&Choice::Auto, &Choice::Auto, true), "mp4");
    assert_eq!(info.extension(&Choice::Skip, &Choice::Auto, true), "m4a");
    assert_eq!(info.extension(&Choice::Auto, &Choice::Skip, false), "mp4");

    let dir = private_dir("dash-number").unwrap();
    // Separate audio needs ffmpeg; video alone does not.
    assert!(matches!(
        run(request(&server, "dash/manifest.mpd", &dir.join("x.mp4"))).await,
        Err(Error::NeedsFfmpeg)
    ));
    let mut req = request(&server, "dash/manifest.mpd", &dir.join("best.mp4"));
    req.audio = Choice::Skip;
    assert_eq!(
        saved(run(req).await.unwrap()).unwrap(),
        expected["v2"],
        "the best quality by default"
    );

    let mut req = request(&server, "dash/manifest.mpd", &dir.join("low.mp4"));
    req.video = Choice::Track(info.video[0].id.clone());
    req.audio = Choice::Skip;
    assert_eq!(saved(run(req).await.unwrap()).unwrap(), expected["v1"]);

    let mut req = request(&server, "dash/manifest.mpd", &dir.join("audio.mp4"));
    req.video = Choice::Skip;
    assert_eq!(saved(run(req).await.unwrap()).unwrap(), expected["a1"]);
    dir.remove().unwrap();
}

#[tokio::test]
async fn timelines_use_times_in_addresses() {
    let server = FileServer::start().await;
    let mut expected = init(1);
    server.put("tl/init.mp4", expected.clone());
    // Times 0, 2000, 4000 (r=2), then 6000 and 7500.
    for (time, len) in [
        (0u64, 900),
        (2000, 900),
        (4000, 900),
        (6000, 700),
        (7500, 400),
    ] {
        let data = ts_segment(time + 1, len);
        server.put(&format!("tl/chunk-{time}.m4s"), data.clone());
        expected.extend(data);
    }
    server.put_typed(
        "tl/manifest.mpd",
        r#"<MPD type="static" mediaPresentationDuration="PT9S"><Period>
  <AdaptationSet mimeType="video/mp4" codecs="avc1.64001f">
    <SegmentTemplate timescale="1000" initialization="init.mp4" media="chunk-$Time$.m4s">
      <SegmentTimeline><S t="0" d="2000" r="2"/><S d="1500"/><S d="1500" r="-1"/></SegmentTimeline>
    </SegmentTemplate>
    <Representation id="v" bandwidth="1000"/>
  </AdaptationSet></Period></MPD>"#,
        DASH,
    );
    let dir = private_dir("dash-timeline").unwrap();
    assert_eq!(
        saved(
            run(request(&server, "tl/manifest.mpd", &dir.join("v.mp4")))
                .await
                .unwrap()
        )
        .unwrap(),
        expected
    );
    dir.remove().unwrap();
}

#[tokio::test]
async fn segment_base_files_are_split_through_their_index() {
    let server = FileServer::start().await;
    // init (ftyp), then a sidx box listing three subsegments, then the media.
    let mut file = init(5);
    let index_start = file.len() as u64;
    let sizes = [3000u32, 2500, 1800];
    let mut sidx = Vec::new();
    let count = u16::try_from(sizes.len()).unwrap();
    let body = 4 + 4 + 4 + 4 + 4 + 2 + 2 + u32::from(count) * 12;
    sidx.extend_from_slice(&(8 + body).to_be_bytes());
    sidx.extend_from_slice(b"sidx");
    sidx.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0x03, 0xE8]);
    sidx.extend_from_slice(&[0; 4]); // earliest presentation time
    sidx.extend_from_slice(&[0; 4]); // first_offset
    sidx.extend_from_slice(&[0, 0]);
    sidx.extend_from_slice(&count.to_be_bytes());
    for size in sizes {
        sidx.extend_from_slice(&size.to_be_bytes());
        sidx.extend_from_slice(&[0; 8]);
    }
    file.extend(&sidx);
    for (i, size) in sizes.iter().enumerate() {
        file.extend(ts_segment(70 + i as u64, *size as usize));
    }
    server.put("sb/video.mp4", file.clone());
    let index_end = index_start + sidx.len() as u64 - 1;
    server.put_typed(
        "sb/manifest.mpd",
        format!(
            r#"<MPD type="static" mediaPresentationDuration="PT6S"><Period><AdaptationSet mimeType="video/mp4">
  <Representation id="v" bandwidth="1"><BaseURL>video.mp4</BaseURL>
    <SegmentBase indexRange="{index_start}-{index_end}"><Initialization range="0-{}"/></SegmentBase>
  </Representation></AdaptationSet></Period></MPD>"#,
            index_start - 1
        ),
        DASH,
    );
    let dir = private_dir("dash-sidx").unwrap();
    let mut req = request(&server, "sb/manifest.mpd", &dir.join("v.mp4"));
    req.options.connections = 4;
    assert_eq!(saved(run(req).await.unwrap()).unwrap(), file);
    // The index read, then the head and three subsegments, each by range.
    assert_eq!(server.requests("sb/video.mp4"), 5);
    assert!(
        server
            .seen()
            .iter()
            .filter(|s| s.path == "/sb/video.mp4")
            .all(|s| s.header("range").is_some())
    );
    dir.remove().unwrap();
}

#[tokio::test]
async fn live_drm_and_several_periods_are_refused() {
    let server = FileServer::start().await;
    let rep = r#"<AdaptationSet mimeType="video/mp4"><SegmentTemplate media="$Number$.m4s" duration="4"/><Representation id="v" bandwidth="1"/></AdaptationSet>"#;
    server.put_typed(
        "live.mpd",
        format!(r#"<MPD type="dynamic" availabilityStartTime="2026-01-01T00:00:00Z"><Period start="PT0S">{rep}</Period></MPD>"#),
        DASH,
    );
    server.put_typed(
        "drm.mpd",
        r#"<MPD type="static" mediaPresentationDuration="PT8S"><Period><AdaptationSet mimeType="video/mp4">
          <ContentProtection schemeIdUri="urn:mpeg:dash:mp4protection:2011" value="cenc"/>
          <ContentProtection schemeIdUri="urn:uuid:9a04f079-9840-4286-ab92-e65be0885f95"/>
          <SegmentTemplate media="$Number$.m4s" duration="4"/><Representation id="v" bandwidth="1"/>
        </AdaptationSet></Period></MPD>"#,
        DASH,
    );
    server.put_typed(
        "periods.mpd",
        format!(r#"<MPD type="static" mediaPresentationDuration="PT8S"><Period duration="PT4S">{rep}</Period><Period duration="PT4S">{rep}</Period></MPD>"#),
        DASH,
    );
    let info = inspect(
        &server.url("live.mpd"),
        &options(),
        CancellationToken::new(),
    )
    .await
    .unwrap();
    assert!(info.live);
    let dir = private_dir("dash-refused").unwrap();
    let error = run(request(&server, "live.mpd", &dir.join("a.mp4")))
        .await
        .unwrap_err();
    assert!(matches!(error, Error::Live), "{error}");
    let error = run(request(&server, "drm.mpd", &dir.join("b.mp4")))
        .await
        .unwrap_err();
    assert!(
        matches!(&error, Error::Drm(DrmSystem::PlayReady)),
        "{error}"
    );
    let error = run(request(&server, "periods.mpd", &dir.join("c.mp4")))
        .await
        .unwrap_err();
    assert!(
        matches!(error, Error::Unsupported(Feature::SeveralPeriods)),
        "{error}"
    );
    assert!(
        !server.seen().iter().any(|s| s.has_extension("m4s")),
        "nothing fetched"
    );
    dir.remove().unwrap();
}

#[tokio::test]
async fn a_base_url_into_the_local_network_is_refused() {
    let server = FileServer::start().await;
    server.put_typed(
        "lan.mpd",
        r#"<MPD type="static" mediaPresentationDuration="PT4S"><BaseURL>http://10.0.0.5/media/</BaseURL><Period>
          <AdaptationSet mimeType="video/mp4"><SegmentTemplate media="$Number$.m4s" duration="2"/>
          <Representation id="v" bandwidth="1"/></AdaptationSet></Period></MPD>"#,
        DASH,
    );
    let dir = private_dir("dash-private").unwrap();
    let mut req = request(&server, "lan.mpd", &dir.join("v.mp4"));
    req.private_network = PrivateNetwork::Block;
    let error = run(req).await.unwrap_err();
    assert!(matches!(error, Error::PrivateNetwork), "{error}");
    dir.remove().unwrap();
}

/// A presentation duration too large for `Duration` once panicked while inspecting, and release builds abort on panic, which took down the whole app.
#[tokio::test]
async fn an_absurd_presentation_duration_is_not_a_crash() {
    let server = FileServer::start().await;
    server.put_typed(
        "huge.mpd",
        r#"<MPD type="static" mediaPresentationDuration="PT99999999999999999999S"><Period><AdaptationSet mimeType="video/mp4">
          <SegmentList><SegmentURL media="0.m4s"/></SegmentList><Representation id="v" bandwidth="1"/>
        </AdaptationSet></Period></MPD>"#,
        DASH,
    );
    let info = inspect(
        &server.url("huge.mpd"),
        &options(),
        CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(info.duration, None, "too long to be real, so unknown");
}

/// A track with no media segments must fail like an HLS playlist without segments, not "complete" as an empty file.
#[tokio::test]
async fn a_track_without_segments_is_refused_not_saved_empty() -> TestResult {
    let server = FileServer::start().await;
    server.put("dash/init.mp4", init(1));
    let manifest = |duration: &str, segments: &str| {
        format!(
            r#"<MPD type="static" mediaPresentationDuration="{duration}"><Period><AdaptationSet mimeType="video/mp4" codecs="avc1.64001f"><Representation id="v" bandwidth="1">{segments}</Representation></AdaptationSet></Period></MPD>"#
        )
    };
    server.put_typed(
        "empty-list.mpd",
        manifest(
            "PT4S",
            r#"<SegmentList><Initialization sourceURL="dash/init.mp4"/></SegmentList>"#,
        ),
        DASH,
    );
    server.put_typed(
        "no-length.mpd",
        manifest(
            "PT0S",
            r#"<SegmentTemplate initialization="dash/init.mp4" media="dash/$Number$.m4s" duration="1"/>"#,
        ),
        DASH,
    );
    let dir = private_dir("dash-empty")?;
    for (manifest, name) in [("empty-list.mpd", "a.mp4"), ("no-length.mpd", "b.mp4")] {
        let output = dir.join(name);
        let error = run(request(&server, manifest, &output)).await.unwrap_err();
        assert!(
            matches!(
                &error,
                Error::Manifest {
                    host: Some(_),
                    line: None,
                    cause: ManifestError::NoSegments
                }
            ),
            "{manifest}: {error}"
        );
        assert!(!output.exists(), "{manifest}");
    }
    dir.remove()?;
    Ok(())
}

/// The file name forecast before the download follows the picks the download makes: here the main representation, not the one with the highest bandwidth.
#[tokio::test]
async fn the_forecast_extension_follows_the_automatic_pick() -> TestResult {
    let server = FileServer::start().await;
    server.put_typed(
        "main.mpd",
        r#"<MPD type="static" mediaPresentationDuration="PT4S"><Period>
          <AdaptationSet mimeType="video/mp4" codecs="avc1.64001f"><Role schemeIdUri="urn:mpeg:dash:role:2011" value="main"/>
            <Representation id="h264" bandwidth="1000"><SegmentTemplate media="v/$Number$.m4s" duration="2"/></Representation>
          </AdaptationSet>
          <AdaptationSet mimeType="video/mp4" codecs="vp09.00.40.08">
            <Representation id="vp9" bandwidth="9000"><SegmentTemplate media="w/$Number$.m4s" duration="2"/></Representation>
          </AdaptationSet></Period></MPD>"#,
        DASH,
    );
    let info = inspect(
        &server.url("main.mpd"),
        &options(),
        CancellationToken::new(),
    )
    .await?;
    assert_eq!(info.extension(&Choice::Auto, &Choice::Auto, true), "mp4");
    let vp9 = info
        .video
        .iter()
        .find(|v| v.codecs.as_deref() == Some("vp09.00.40.08"))
        .ok_or("no VP9 track")?;
    assert_eq!(
        info.extension(&Choice::Track(vp9.id.clone()), &Choice::Auto, true),
        "mkv"
    );
    Ok(())
}

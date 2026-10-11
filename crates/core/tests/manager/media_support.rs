//! Helpers for the add-on and media tests: a local server for many files, fake
//! add-on releases built from the `fake_media_tool` example, and a harness with
//! the media add-on pointed at them.

#![allow(dead_code)]

use std::collections::HashMap;
use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode, Uri, header};
use axum::response::Response;
use axum::routing::get;
use bytes::Bytes;
use pgp::composed::{
    DetachedSignature, KeyType, SecretKeyParamsBuilder, SignedPublicKey, SignedSecretKey,
};
use pgp::crypto::hash::HashAlgorithm;
use pgp::ser::Serialize as _;
use pgp::types::{KeyDetails as _, Password};
use rand::SeedableRng;
use rand::rngs::StdRng;
use sha2::{Digest, Sha256};
use tokio::sync::broadcast;
use wirecatch_core::{AddonComponent, AddonFile, ArchiveKind, MediaAddonSources, PinnedKey};
use wirecatch_protocol::app::{AddonInfo, AddonState, UiEvent};
use wirecatch_protocol::extension::{CaptureSource, MediaHint, MediaKind, NewDownload};

use crate::common::{Harness, PATIENCE};

#[derive(Clone, Default)]
struct File {
    body: Bytes,
    /// Bytes per second, if slowed down.
    bps: Option<u64>,
    content_type: Option<String>,
}

/// One request as the server saw it.
#[derive(Clone, Debug)]
pub struct Seen {
    pub path: String,
    pub headers: HeaderMap,
}

#[derive(Default)]
struct Files {
    files: Mutex<HashMap<String, File>>,
    seen: Mutex<Vec<Seen>>,
}

/// Serves files by path, with byte ranges.
pub struct FileServer {
    pub base: String,
    inner: Arc<Files>,
}

impl FileServer {
    pub async fn start() -> Self {
        let inner = Arc::new(Files::default());
        let app = Router::new()
            .route("/{*path}", get(serve))
            .with_state(inner.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        Self {
            base: format!("http://{addr}"),
            inner,
        }
    }

    pub fn url(&self, path: &str) -> String {
        format!("{}/{}", self.base, path.trim_start_matches('/'))
    }

    pub fn put(&self, path: &str, body: impl Into<Bytes>) {
        self.inner.files.lock().unwrap().insert(
            format!("/{}", path.trim_start_matches('/')),
            File {
                body: body.into(),
                ..File::default()
            },
        );
    }

    pub fn put_typed(&self, path: &str, body: impl Into<Bytes>, content_type: &str) {
        self.put(path, body);
        self.change(path, |f| f.content_type = Some(content_type.into()));
    }

    /// Sends `path` at `bps` bytes per second.
    pub fn throttle(&self, path: &str, bps: Option<u64>) {
        self.change(path, |f| f.bps = bps);
    }

    fn change(&self, path: &str, change: impl FnOnce(&mut File)) {
        let key = format!("/{}", path.trim_start_matches('/'));
        if let Some(file) = self.inner.files.lock().unwrap().get_mut(&key) {
            change(file);
        }
    }

    /// Requests made for `path`.
    pub fn requests(&self, path: &str) -> Vec<Seen> {
        let key = format!("/{}", path.trim_start_matches('/'));
        self.inner
            .seen
            .lock()
            .unwrap()
            .iter()
            .filter(|s| s.path == key)
            .cloned()
            .collect()
    }
}

async fn serve(State(inner): State<Arc<Files>>, uri: Uri, headers: HeaderMap) -> Response {
    inner.seen.lock().unwrap().push(Seen {
        path: uri.path().to_string(),
        headers: headers.clone(),
    });
    let Some(file) = inner.files.lock().unwrap().get(uri.path()).cloned() else {
        return Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(Body::empty())
            .unwrap();
    };
    let len = file.body.len() as u64;
    let range = headers
        .get(header::RANGE)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("bytes="))
        .and_then(|v| v.split_once('-'))
        .and_then(|(a, b)| {
            let start: u64 = a.parse().ok()?;
            let end = if b.is_empty() {
                len
            } else {
                b.parse::<u64>().ok()?.saturating_add(1).min(len)
            };
            Some((start, end))
        });
    let mut builder = Response::builder()
        .header(header::ACCEPT_RANGES, "bytes")
        .header(header::ETAG, "\"fixed\"");
    if let Some(t) = &file.content_type {
        builder = builder.header(header::CONTENT_TYPE, t);
    }
    let (start, end) = match range {
        Some((start, _)) if start >= len && len > 0 => {
            return builder
                .status(StatusCode::RANGE_NOT_SATISFIABLE)
                .header(header::CONTENT_RANGE, format!("bytes */{len}"))
                .body(Body::empty())
                .unwrap();
        }
        Some((start, end)) if len > 0 => {
            builder = builder.status(StatusCode::PARTIAL_CONTENT).header(
                header::CONTENT_RANGE,
                format!("bytes {start}-{}/{len}", end - 1),
            );
            (start, end)
        }
        _ => (0, len),
    };
    builder = builder.header(header::CONTENT_LENGTH, end - start);
    let chunk = file.bps.map_or(64 * 1024, |bps| (bps / 20).max(512)) as usize;
    let pause = file
        .bps
        .map(|bps| Duration::from_secs_f64(chunk as f64 / bps as f64));
    let data = file.body.clone();
    let body = futures_util::stream::unfold(start, move |pos| {
        let data = data.clone();
        async move {
            if pos >= end {
                return None;
            }
            if let Some(pause) = pause {
                tokio::time::sleep(pause).await;
            }
            let next = (pos + chunk as u64).min(end);
            Some((
                Ok::<_, std::io::Error>(data.slice(pos as usize..next as usize)),
                next,
            ))
        }
    });
    builder.body(Body::from_stream(body)).unwrap()
}

/// The `fake_media_tool` example, built by `cargo test`.
pub fn fake_tool() -> Vec<u8> {
    let exe = std::env::current_exe().unwrap();
    let debug = exe.parent().unwrap().parent().unwrap();
    let path = debug
        .join("examples")
        .join(format!("fake_media_tool{}", std::env::consts::EXE_SUFFIX));
    std::fs::read(&path).unwrap_or_else(|e| {
        panic!(
            "{} is missing ({e}): run `cargo test -p wirecatch-core`, which builds the examples",
            path.display()
        )
    })
}

/// A real ffmpeg for the join test, from `WIRECATCH_TEST_FFMPEG`.
pub fn real_ffmpeg() -> Option<PathBuf> {
    std::env::var_os("WIRECATCH_TEST_FFMPEG")
        .map(PathBuf::from)
        .filter(|p| p.is_file())
}

pub fn sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// A zip archive of `entries` (name, content); a `None` content makes a symbolic
/// link to `/etc/passwd`.
pub fn zip(entries: &[(&str, Option<&[u8]>)]) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .unix_permissions(0o755);
    for (name, content) in entries {
        match content {
            Some(content) => {
                zip.start_file(*name, options).unwrap();
                zip.write_all(content).unwrap();
            }
            None => zip.add_symlink(*name, "/etc/passwd", options).unwrap(),
        }
    }
    zip.finish().unwrap().into_inner()
}

/// A `.tar.xz` archive of `entries`; names are written raw (even with `..`),
/// and a `None` content makes a symbolic link to `/etc/passwd`.
pub fn tar_xz(entries: &[(&str, Option<&[u8]>)]) -> Vec<u8> {
    let mut tar = tar::Builder::new(Vec::new());
    for (name, content) in entries {
        let mut header = tar::Header::new_old();
        let raw = &mut header.as_old_mut().name;
        raw[..name.len()].copy_from_slice(name.as_bytes());
        header.set_mode(0o755);
        match content {
            Some(content) => {
                header.set_entry_type(tar::EntryType::Regular);
                header.set_size(content.len() as u64);
                header.set_cksum();
                tar.append(&header, *content).unwrap();
            }
            None => {
                header.set_entry_type(tar::EntryType::Symlink);
                header.set_size(0);
                header.set_link_name("/etc/passwd").unwrap();
                header.set_cksum();
                tar.append(&header, std::io::empty()).unwrap();
            }
        }
    }
    let tar = tar.into_inner().unwrap();
    let mut xz =
        lzma_rust2::XzWriter::new(Vec::new(), lzma_rust2::XzOptions::with_preset(1)).unwrap();
    xz.write_all(&tar).unwrap();
    xz.finish().unwrap()
}

pub fn exe(stem: &str) -> String {
    format!("{stem}{}", std::env::consts::EXE_SUFFIX)
}

/// What a test add-on release is made of.
pub struct Releases {
    pub ytdlp: Vec<u8>,
    pub ffmpeg: Vec<u8>,
    pub deno: Vec<u8>,
}

impl Releases {
    /// Fake tools: yt-dlp as a plain program, ffmpeg in a zip, Deno in a
    /// `.tar.xz`. Both archives carry entries that must never be installed: a
    /// path that climbs out, an unlisted program and a symbolic link.
    pub fn fake() -> Self {
        Self::with_ffmpeg(&fake_tool())
    }

    /// The same with `ffmpeg_exe` as ffmpeg.
    pub fn with_ffmpeg(ffmpeg_exe: &[u8]) -> Self {
        let tool = fake_tool();
        Self {
            ytdlp: tool.clone(),
            ffmpeg: zip(&[
                ("../escape.txt", Some(b"escaped".as_slice())),
                (&format!("ff/bin/{}", exe("ffmpeg")), Some(ffmpeg_exe)),
                ("ff/bin/avcodec-63.dll", Some(b"library".as_slice())),
                ("ff/bin/extra.exe", Some(b"not listed".as_slice())),
                ("ff/bin/link", None),
            ]),
            deno: tar_xz(&[
                ("../../evil.txt", Some(b"escaped".as_slice())),
                (&format!("deno/{}", exe("deno")), Some(&tool)),
                ("deno/link", None),
            ]),
        }
    }
}

/// Serves `releases` and describes them as add-on sources.
pub fn publish(server: &FileServer, releases: &Releases, prefix: &str) -> MediaAddonSources {
    server.put(&format!("{prefix}/yt-dlp"), releases.ytdlp.clone());
    server.put(&format!("{prefix}/ffmpeg.zip"), releases.ffmpeg.clone());
    server.put(&format!("{prefix}/deno.tar.xz"), releases.deno.clone());
    let ytdlp = exe("yt-dlp");
    MediaAddonSources {
        ytdlp: AddonComponent {
            name: "yt-dlp".into(),
            version: "2026.08.19".into(),
            url: server.url(&format!("{prefix}/yt-dlp")),
            sha256: sha256(&releases.ytdlp),
            size: releases.ytdlp.len() as u64,
            archive: ArchiveKind::Plain,
            files: vec![AddonFile {
                from: String::new(),
                to: ytdlp.clone(),
                executable: true,
            }],
            program: ytdlp,
        },
        ffmpeg: AddonComponent {
            name: "ffmpeg".into(),
            version: "n9.0.2-25-g67b60c310b".into(),
            url: server.url(&format!("{prefix}/ffmpeg.zip")),
            sha256: sha256(&releases.ffmpeg),
            size: releases.ffmpeg.len() as u64,
            archive: ArchiveKind::Zip,
            files: vec![
                AddonFile {
                    from: format!("ff/bin/{}", exe("ffmpeg")),
                    to: format!("bin/{}", exe("ffmpeg")),
                    executable: true,
                },
                AddonFile {
                    from: "ff/bin/avcodec-63.dll".into(),
                    to: "bin/avcodec-63.dll".into(),
                    executable: false,
                },
            ],
            program: format!("bin/{}", exe("ffmpeg")),
        },
        deno: AddonComponent {
            name: "deno".into(),
            version: "2.9.7".into(),
            url: server.url(&format!("{prefix}/deno.tar.xz")),
            sha256: sha256(&releases.deno),
            size: releases.deno.len() as u64,
            archive: ArchiveKind::TarXz,
            files: vec![AddonFile {
                from: format!("deno/{}", exe("deno")),
                to: exe("deno"),
                executable: true,
            }],
            program: exe("deno"),
        },
        ytdlp_releases: None,
        ytdlp_asset: ytdlp_asset(),
        ytdlp_checksums_asset: "SHA2-256SUMS".into(),
        ytdlp_signing_key: test_signer().1.clone(),
        ytdlp_release_downloads: server.url("rel/"),
    }
}

/// A throwaway OpenPGP key made for the tests (seeded, so every run makes the
/// same one), standing in for yt-dlp's release key; and the same key pinned.
fn signer(seed: u64) -> (SignedSecretKey, PinnedKey) {
    let mut params = SecretKeyParamsBuilder::default();
    params
        .key_type(KeyType::Ed25519Legacy)
        .can_certify(true)
        .can_sign(true)
        .primary_user_id(format!("Test signer {seed} <test@example.invalid>"));
    let secret = params
        .build()
        .unwrap()
        .generate(StdRng::seed_from_u64(seed))
        .unwrap();
    let public = SignedPublicKey::from(secret.clone());
    let pinned = PinnedKey {
        armored: public.to_armored_string(Default::default()).unwrap(),
        fingerprint: format!("{:X}", public.fingerprint()),
    };
    (secret, pinned)
}

/// The key the test add-on sources trust for yt-dlp updates.
pub fn test_signer() -> &'static (SignedSecretKey, PinnedKey) {
    static SIGNER: OnceLock<(SignedSecretKey, PinnedKey)> = OnceLock::new();
    SIGNER.get_or_init(|| signer(1))
}

/// A detached binary signature over `data` by the trusted test key, or with
/// `trusted` false by another key.
pub fn sign(data: &[u8], trusted: bool) -> Vec<u8> {
    static OTHER: OnceLock<(SignedSecretKey, PinnedKey)> = OnceLock::new();
    let key = if trusted {
        &test_signer().0
    } else {
        &OTHER.get_or_init(|| signer(2)).0
    };
    DetachedSignature::sign_binary_data(
        StdRng::seed_from_u64(3),
        &key.primary_key,
        &Password::empty(),
        HashAlgorithm::Sha256,
        data,
    )
    .unwrap()
    .to_bytes()
    .unwrap()
}

pub fn ytdlp_asset() -> String {
    exe("yt-dlp")
}

/// A harness whose media add-on comes from `sources`.
pub async fn harness(sources: MediaAddonSources) -> Harness {
    let sources = Arc::new(sources);
    Harness::configured(Arc::new(move |config| {
        config.with_addon_sources((*sources).clone())
    }))
    .await
}

/// The media add-on folder of a harness.
pub fn addon_dir(h: &Harness) -> PathBuf {
    h.root.path().join("data").join("addons").join("media")
}

/// Waits for an `AddonsChanged` whose add-on satisfies `check`.
pub async fn wait_addon(
    ui: &mut broadcast::Receiver<UiEvent>,
    what: &str,
    check: impl Fn(&AddonInfo) -> bool,
) -> AddonInfo {
    next_event(ui, what, |event| match event {
        UiEvent::AddonsChanged(list) => list.iter().find(|a| check(a)).cloned(),
        _ => None,
    })
    .await
}

/// Waits for the first event that `pick` turns into a value.
pub async fn next_event<T>(
    ui: &mut broadcast::Receiver<UiEvent>,
    what: &str,
    mut pick: impl FnMut(&UiEvent) -> Option<T>,
) -> T {
    let wait = async {
        loop {
            match ui.recv().await {
                Ok(event) => {
                    if let Some(found) = pick(&event) {
                        return found;
                    }
                }
                Err(broadcast::error::RecvError::Lagged(_)) => {}
                Err(e) => panic!("events ended: {e}"),
            }
        }
    };
    tokio::time::timeout(PATIENCE, wait)
        .await
        .unwrap_or_else(|_| panic!("timed out waiting for: {what}"))
}

/// Installs the add-on and waits until it is installed.
pub async fn install(h: &Harness) -> AddonInfo {
    let mut ui = h.manager.subscribe();
    h.manager.install_addon("media").await.unwrap();
    wait_addon(&mut ui, "the add-on installed", |a| {
        matches!(a.state, AddonState::Installed | AddonState::Failed { .. })
    })
    .await
}

/// A media capture from the browser extension.
pub fn media_capture(url: &str, kind: MediaKind, title: Option<&str>) -> NewDownload {
    NewDownload {
        handover_id: format!("media-{url}"),
        url: url.to_string(),
        referrer: Some("https://videos.example/watch".into()),
        page_url: Some("https://videos.example/watch".into()),
        suggested_name: None,
        mime: None,
        size_hint: None,
        cookies: Some("session=abc123; theme=dark".into()),
        user_agent: Some("TestBrowser/2".into()),
        extra_headers: vec![],
        source: CaptureSource::MediaButton,
        queue: None,
        media: Some(MediaHint {
            kind,
            title: title.map(str::to_string),
        }),
    }
}

/// Where the fake tools read their instructions and log their calls.
pub fn tool_dir(h: &Harness) -> PathBuf {
    let dir = h.root.path().join("data").join("fake-tools");
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Every call the fake yt-dlp recorded, as JSON.
pub fn ytdlp_calls(h: &Harness) -> Vec<serde_json::Value> {
    let log = tool_dir(h).join("calls.jsonl");
    std::fs::read_to_string(log)
        .unwrap_or_default()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

/// The arguments of a recorded call.
pub fn args(call: &serde_json::Value) -> Vec<String> {
    call["args"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a.as_str().unwrap().to_string())
        .collect()
}

/// Leaves an instruction for the fake tools.
pub fn tell_tool(h: &Harness, file: &str, text: &str) {
    std::fs::write(tool_dir(h).join(file), text).unwrap();
}

pub fn path_exists(path: impl AsRef<Path>) -> bool {
    path.as_ref().exists()
}

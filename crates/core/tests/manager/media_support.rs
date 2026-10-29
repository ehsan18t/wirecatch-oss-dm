//! Helpers for the add-on and media tests: fake add-on releases built from the
//! `fake_media_tool` program, served by the test-support crate's file server,
//! and a harness with the media add-on pointed at them.

use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use pgp::composed::{
    ArmorOptions, DetachedSignature, KeyType, SecretKeyParamsBuilder, SignedPublicKey,
    SignedSecretKey,
};
use pgp::crypto::hash::HashAlgorithm;
use pgp::ser::Serialize as _;
use pgp::types::{KeyDetails as _, Password};
use rand::SeedableRng;
use rand::rngs::StdRng;
use sha2::{Digest, Sha256};
use tokio::sync::broadcast;
use wirecatch_core::{
    AddonComponent, AddonFile, ArchiveKind, ComponentId, Fingerprint, MediaAddonSources, PinnedKey,
    ReleaseFile, Sha256Digest,
};
use wirecatch_protocol::app::{AddonId, AddonInfo, AddonState, UiEvent};
use wirecatch_protocol::extension::{CaptureSource, MediaHint, MediaKind, NewDownload};

pub use wirecatch_test_support::files::FileServer;

use crate::common::{Harness, PATIENCE, TestResult};

/// The `fake_media_tool` program, which cargo builds for this crate's tests.
pub fn fake_tool() -> TestResult<Vec<u8>> {
    Ok(std::fs::read(env!("CARGO_BIN_EXE_fake_media_tool"))?)
}

/// The fake tool as yt-dlp release `version`: it says it is that version when asked.
pub fn ytdlp_release(version: &str) -> TestResult<Vec<u8>> {
    Ok([
        fake_tool()?,
        format!("\nwirecatch-fake-version={version}").into_bytes(),
    ]
    .concat())
}

pub use wirecatch_test_support::real_ffmpeg;

/// The SHA-256 of `bytes` in lower-case hex, worked out here rather than by
/// the add-on code under test.
pub fn sha256(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let digest = Sha256::digest(bytes);
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        hex.push(char::from(DIGITS[usize::from(byte >> 4)]));
        hex.push(char::from(DIGITS[usize::from(byte & 0xf)]));
    }
    hex
}

/// A zip archive of `entries` (name, content); a `None` content makes a symbolic
/// link to `/etc/passwd`.
pub fn zip(entries: &[(&str, Option<&[u8]>)]) -> TestResult<Vec<u8>> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .unix_permissions(0o755);
    for (name, content) in entries {
        match content {
            Some(content) => {
                zip.start_file(*name, options)?;
                zip.write_all(content)?;
            }
            None => zip.add_symlink(*name, "/etc/passwd", options)?,
        }
    }
    Ok(zip.finish()?.into_inner())
}

/// A `.tar.xz` archive of `entries`; names are written raw (even with `..`),
/// and a `None` content makes a symbolic link to `/etc/passwd`.
pub fn tar_xz(entries: &[(&str, Option<&[u8]>)]) -> TestResult<Vec<u8>> {
    let mut tar = tar::Builder::new(Vec::new());
    for (name, content) in entries {
        let mut header = tar::Header::new_old();
        let raw = &mut header.as_old_mut().name;
        raw[..name.len()].copy_from_slice(name.as_bytes());
        header.set_mode(0o755);
        if let Some(content) = content {
            header.set_entry_type(tar::EntryType::Regular);
            header.set_size(content.len() as u64);
            header.set_cksum();
            tar.append(&header, *content)?;
        } else {
            header.set_entry_type(tar::EntryType::Symlink);
            header.set_size(0);
            header.set_link_name("/etc/passwd")?;
            header.set_cksum();
            tar.append(&header, std::io::empty())?;
        }
    }
    let tar = tar.into_inner()?;
    let mut xz = lzma_rust2::XzWriter::new(Vec::new(), lzma_rust2::XzOptions::with_preset(1))?;
    xz.write_all(&tar)?;
    Ok(xz.finish()?)
}

/// `stem` with the platform's program extension (`.exe` on Windows).
pub fn exe(stem: &str) -> String {
    format!("{stem}{}", std::env::consts::EXE_SUFFIX)
}

/// What a test add-on release is made of.
#[derive(Clone)]
pub struct Releases {
    /// The yt-dlp program, as published.
    pub ytdlp: Vec<u8>,
    /// The ffmpeg zip archive.
    pub ffmpeg: Vec<u8>,
    /// The Deno `.tar.xz` archive.
    pub deno: Vec<u8>,
}

impl Releases {
    /// Fake tools: yt-dlp as a plain program, ffmpeg in a zip, Deno in a
    /// `.tar.xz`. Both archives carry entries that must never be installed: a
    /// path that climbs out, an unlisted program and a symbolic link.
    ///
    /// Packing them takes seconds in a debug build, so they are packed once per
    /// test binary.
    pub fn fake() -> TestResult<Self> {
        static PACKED: OnceLock<Releases> = OnceLock::new();
        if let Some(packed) = PACKED.get() {
            return Ok(packed.clone());
        }
        // Two tests may both pack them; the archives are the same either way.
        let packed = Self::pack()?;
        Ok(PACKED.get_or_init(|| packed).clone())
    }

    fn pack() -> TestResult<Self> {
        let tool = fake_tool()?;
        Ok(Self {
            ytdlp: tool.clone(),
            ffmpeg: zip(&[
                ("../escape.txt", Some(b"escaped".as_slice())),
                (&format!("ff/bin/{}", exe("ffmpeg")), Some(&tool)),
                ("ff/bin/avcodec-63.dll", Some(b"library".as_slice())),
                ("ff/bin/extra.exe", Some(b"not listed".as_slice())),
                ("ff/bin/link", None),
            ])?,
            deno: tar_xz(&[
                ("../../evil.txt", Some(b"escaped".as_slice())),
                (&format!("deno/{}", exe("deno")), Some(&tool)),
                ("deno/link", None),
            ])?,
        })
    }
}

/// Serves `releases` and describes them as add-on sources.
pub fn publish(
    server: &FileServer,
    releases: &Releases,
    prefix: &str,
) -> TestResult<MediaAddonSources> {
    server.put(&format!("{prefix}/yt-dlp"), releases.ytdlp.clone());
    server.put(&format!("{prefix}/ffmpeg.zip"), releases.ffmpeg.clone());
    server.put(&format!("{prefix}/deno.tar.xz"), releases.deno.clone());
    let ytdlp = exe("yt-dlp");
    let file = |from: String, to: String, executable| AddonFile {
        from,
        to,
        executable,
    };
    Ok(MediaAddonSources {
        ytdlp: AddonComponent::new(
            ComponentId::YtDlp,
            "2026.08.19",
            release_file(server, &format!("{prefix}/yt-dlp"), &releases.ytdlp)?,
            ArchiveKind::Plain,
            vec![file(String::new(), ytdlp.clone(), true)],
            ytdlp,
        )?,
        ffmpeg: AddonComponent::new(
            ComponentId::Ffmpeg,
            "n9.0.2-25-g67b60c310b",
            release_file(server, &format!("{prefix}/ffmpeg.zip"), &releases.ffmpeg)?,
            ArchiveKind::Zip,
            vec![
                file(
                    format!("ff/bin/{}", exe("ffmpeg")),
                    format!("bin/{}", exe("ffmpeg")),
                    true,
                ),
                file(
                    "ff/bin/avcodec-63.dll".into(),
                    "bin/avcodec-63.dll".into(),
                    false,
                ),
            ],
            format!("bin/{}", exe("ffmpeg")),
        )?,
        deno: AddonComponent::new(
            ComponentId::Deno,
            "2.9.7",
            release_file(server, &format!("{prefix}/deno.tar.xz"), &releases.deno)?,
            ArchiveKind::TarXz,
            vec![file(format!("deno/{}", exe("deno")), exe("deno"), true)],
            exe("deno"),
        )?,
        ytdlp_releases: None,
        ytdlp_asset: ytdlp_asset(),
        ytdlp_checksums_asset: "SHA2-256SUMS".into(),
        ytdlp_signing_key: test_signer()?.1.clone(),
        ytdlp_release_downloads: server.url("rel/"),
    })
}

/// The release file `bytes`, served at `path`.
pub fn release_file(server: &FileServer, path: &str, bytes: &[u8]) -> TestResult<ReleaseFile> {
    Ok(ReleaseFile::new(
        server.url(path),
        digest(bytes),
        bytes.len() as u64,
    )?)
}

/// The SHA-256 of `bytes`, worked out here rather than by the add-on code under test.
pub fn digest(bytes: &[u8]) -> Sha256Digest {
    Sha256Digest::from_bytes(Sha256::digest(bytes).into())
}

/// A throwaway OpenPGP key made for the tests (seeded, so every run makes the same one), standing in for yt-dlp's release key; and the same key pinned.
fn signer(seed: u64) -> TestResult<(SignedSecretKey, PinnedKey)> {
    let mut params = SecretKeyParamsBuilder::default();
    params
        .key_type(KeyType::Ed25519Legacy)
        .can_certify(true)
        .can_sign(true)
        .primary_user_id(format!("Test signer {seed} <test@example.invalid>"));
    let secret = params.build()?.generate(StdRng::seed_from_u64(seed))?;
    let public = SignedPublicKey::from(secret.clone());
    let pinned = PinnedKey {
        armored: public.to_armored_string(ArmorOptions::default())?,
        fingerprint: Fingerprint::parse(&format!("{:X}", public.fingerprint()))?,
    };
    Ok((secret, pinned))
}

/// The key made from `seed` by [`signer`], made once per test binary.
fn cached_signer(
    cache: &'static OnceLock<(SignedSecretKey, PinnedKey)>,
    seed: u64,
) -> TestResult<&'static (SignedSecretKey, PinnedKey)> {
    if let Some(made) = cache.get() {
        return Ok(made);
    }
    // Two tests may both make it; the key is the same either way.
    let made = signer(seed)?;
    Ok(cache.get_or_init(|| made))
}

/// The key the test add-on sources trust for yt-dlp updates.
pub fn test_signer() -> TestResult<&'static (SignedSecretKey, PinnedKey)> {
    static SIGNER: OnceLock<(SignedSecretKey, PinnedKey)> = OnceLock::new();
    cached_signer(&SIGNER, 1)
}

/// A detached binary signature over `data` by the trusted test key, or with
/// `trusted` false by another key.
pub fn sign(data: &[u8], trusted: bool) -> TestResult<Vec<u8>> {
    static OTHER: OnceLock<(SignedSecretKey, PinnedKey)> = OnceLock::new();
    let key = if trusted {
        &test_signer()?.0
    } else {
        &cached_signer(&OTHER, 2)?.0
    };
    let signature = DetachedSignature::sign_binary_data(
        StdRng::seed_from_u64(3),
        &key.primary_key,
        &Password::empty(),
        HashAlgorithm::Sha256,
        data,
    )?;
    Ok(signature.to_bytes()?)
}

/// The name of the yt-dlp program among a release's assets.
pub fn ytdlp_asset() -> String {
    exe("yt-dlp")
}

/// A harness whose media add-on comes from `sources`.
pub async fn harness(sources: MediaAddonSources) -> TestResult<Harness> {
    let sources = Arc::new(sources);
    Harness::configured(Arc::new(move |config| {
        config.with_addon_sources((*sources).clone())
    }))
    .await
}

/// A harness whose media add-on comes from `sources`, and whose page videos
/// read the page again after `delays` when a format's address is refused.
pub async fn harness_refreshing(
    sources: MediaAddonSources,
    delays: Vec<Duration>,
) -> TestResult<Harness> {
    let sources = Arc::new(sources);
    Harness::configured(Arc::new(move |config| {
        config
            .with_addon_sources((*sources).clone())
            .with_media_refresh_delays(delays.clone())
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
pub async fn install(h: &Harness) -> TestResult<AddonInfo> {
    let mut ui = h.manager.subscribe();
    h.manager.install_addon(AddonId::Media).await?;
    Ok(wait_addon(&mut ui, "the add-on installed", |a| {
        matches!(a.state, AddonState::Installed | AddonState::Failed { .. })
    })
    .await)
}

/// A media capture from the browser extension.
pub fn media_capture(url: impl AsRef<str>, kind: MediaKind, title: Option<&str>) -> NewDownload {
    let url = url.as_ref();
    NewDownload {
        handover_id: format!("media-{url}"),
        url: url.to_string(),
        referrer: Some("https://videos.test/watch".into()),
        page_url: Some("https://videos.test/watch".into()),
        frame_url: None,
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
pub fn tool_dir(h: &Harness) -> TestResult<PathBuf> {
    let dir = h.root.path().join("data").join("fake-tools");
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// Every call the fake yt-dlp recorded, as JSON; none before the first call.
pub fn ytdlp_calls(h: &Harness) -> TestResult<Vec<serde_json::Value>> {
    let log = tool_dir(h)?.join("calls.jsonl");
    let text = match std::fs::read_to_string(log) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(e.into()),
    };
    Ok(text
        .lines()
        .map(serde_json::from_str)
        .collect::<Result<_, _>>()?)
}

/// The arguments of a recorded call.
pub fn args(call: &serde_json::Value) -> TestResult<Vec<String>> {
    call["args"]
        .as_array()
        .ok_or("the call has no argument list")?
        .iter()
        .map(|a| {
            a.as_str()
                .map(str::to_string)
                .ok_or_else(|| format!("{a} is not a text argument").into())
        })
        .collect()
}

/// Leaves an instruction for the fake tools.
pub fn tell_tool(h: &Harness, file: &str, text: &str) -> TestResult {
    std::fs::write(tool_dir(h)?.join(file), text)?;
    Ok(())
}

/// Has the installed fake ffmpeg hand every call to the real `ffmpeg`: a real
/// one is hundreds of megabytes, too large to install in every test that uses
/// it within [`PATIENCE`] on a loaded machine. The install itself is still
/// tested with the fake; what no test does any more is extract a full-size
/// binary and run that installed copy (the archive's size cap has unit tests
/// in `addons/archive.rs`).
pub fn use_real_ffmpeg(h: &Harness, ffmpeg: &Path) -> TestResult {
    tell_tool(h, "real-ffmpeg", &ffmpeg.to_string_lossy())
}

/// Whether `path` exists.
pub fn path_exists(path: impl AsRef<Path>) -> bool {
    path.as_ref().exists()
}

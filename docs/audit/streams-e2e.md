# Audit: crates/streams and crates/e2e

## STR-1 A hostile duration in a playlist or manifest panics, which aborts the whole app
- severity: critical
- category: unsafe
- location: crates/streams/src/plan.rs:364-366, crates/streams/src/plan.rs:200, crates/streams/src/plan.rs:234, crates/streams/src/plan.rs:251, crates/streams/src/hls.rs:199-205, crates/streams/src/dash.rs:534-571, Cargo.toml:60
- problem: `seconds()` only checks `is_finite() && > 0.0` before `Duration::from_secs_f64`, which panics when the value overflows `Duration` (above about 1.8e19 s). Both parsers let such values through: HLS `#EXTINF:1e300,` parses as a finite f64 and is summed into `Media::duration`; DASH `mediaPresentationDuration="PT99999999999999999999S"` is finite in `iso_duration`. `inspect()` (called by core in `crates/core/src/manager/media.rs:579` inside the app process) reaches `seconds()` for every HLS master, HLS media and DASH source. The release profile sets `panic = "abort"`, so one crafted or broken link kills the desktop app and every running download. The existing "hostile input is an error, not a panic" tests do not cover this.
- fix: Replace the body of `seconds()` with `Duration::try_from_secs_f64(value).ok().filter(|d| !d.is_zero())`. Additionally clamp per-segment `EXTINF` values in `hls::parse_media` (reject or drop values above a named `MAX_SEGMENT_SECONDS`), and add `#EXTINF:1e300,` and `PT99999999999999999999S` cases to `broken_playlists_are_errors_not_panics` and `hostile_manifests_are_errors_not_panics` that go through `Source::info`.
- cross-package: no

## STR-2 Resume is wiped whenever the link is refreshed, because query tokens are in the plan fingerprint
- severity: high
- category: design
- location: crates/streams/src/plan.rs:116-137, crates/streams/src/workdir.rs:42-56, crates/streams/src/error.rs:32-34, crates/streams/src/error.rs:98-100
- problem: `Plan::fingerprint` hashes every piece's query string, and `WorkDir::open` deletes the whole `.wcstream` folder when the fingerprint differs. Signed CDN links (CloudFront, Akamai `hdnts`, most video sites) carry a per-session token in every segment query. The crate's own errors for an expired link (`Manifest`, `Key`, `Decrypt`) tell the user to "open the video page again to get a fresh one", which yields new tokens, a new fingerprint, and silent deletion of every downloaded segment. The test `fingerprints_follow_paths_and_queries_but_not_hosts` asserts this (a vs b differ only by `token=`), so the headline "resume" feature fails exactly in the scenario the error messages steer users into.
- fix: Hash the query only where it is needed to tell pieces apart: per track, include `query()` for a piece only if its path is not unique within that track (compute a path-count map first). Keep ranges, IVs, init flags and counts in the hash. Add a test where segment paths are unique and only a `token=` parameter changes, asserting the fingerprint is stable, next to the existing `seg?n=1` case.
- cross-package: no

## STR-3 Track ids are positional strings that core persists across restarts
- severity: high
- category: design
- location: crates/streams/src/plan.rs:208, crates/streams/src/plan.rs:224, crates/streams/src/plan.rs:237, crates/streams/src/plan.rs:259, crates/streams/src/plan.rs:274, crates/streams/src/plan.rs:290, crates/streams/src/plan.rs:308, crates/streams/src/plan.rs:395, crates/streams/src/plan.rs:410, crates/streams/src/plan.rs:438-441, crates/streams/src/plan.rs:466-486, crates/streams/src/plan.rs:609, crates/streams/src/model.rs:156-165, crates/core/src/media/mod.rs:30-38, crates/core/src/manager/media_run.rs:176-182
- problem: Ids are `format!("v{i}")`, `"r{i}"`, `"d{i}"` (index into the parsed list) and are parsed back with `index_of(id, prefix)`. Core stores them in `Pick::Track(String)` "so it can continue after a restart" and re-plans the manifest on resume. If the playlist lists variants or renditions in another order on the next fetch (common with refreshed links, A/B CDNs, or added renditions), `r2` silently selects a different audio language or quality; `NoSuchTrack` only fires when the index is out of range. DASH even has a spec-required, stable `Representation@id` that is read (dash.rs:358) but not used as the id. The `v`/`r`/`d` prefix grammar is also an undocumented stringly-typed protocol between modules.
- fix: Make ids content-derived and stable: DASH uses `Representation@id` (escaped, plus adaptation-set index only for disambiguation); HLS uses a short hash of the variant URI path plus BANDWIDTH/RESOLUTION/CODECS, and of GROUP-ID+NAME+LANGUAGE for renditions. Introduce a `TrackId` newtype with private constructor and a `TrackRef` enum (`Variant(usize)`, `Rendition(usize)`, `Representation(usize)`) resolved once in `plan.rs`, replacing `index_of` and the string prefixes. Add a test that reorders variants between inspect and download and asserts the same track is chosen.
- cross-package: yes (core)

## STR-4 Two codec classifiers with contradictory MP4 policies
- severity: medium
- category: duplication
- location: crates/streams/src/plan.rs:656-696, crates/streams/src/download.rs:459-473, crates/streams/src/model.rs:80-93, crates/streams/src/ffmpeg.rs:31-63, crates/core/src/media/output.rs:30-120, crates/core/src/media/output.rs:245-275
- problem: streams classifies codecs with three string lists (`is_video_codec`, `is_audio_codec`, `mp4_compatible`, each re-implementing the "split on '.', lowercase" step) and has its own `Container` enum. Core has typed `VideoCodec`/`AudioCodec` and `Container::holds_video`, whose documented policy is that MP4 takes H.264 and H.265 only because players refuse VP9/AV1 in MP4. streams' `mp4_compatible` includes `av01` and `vp09`, so an HLS/DASH stream with VP9 or AV1 is written to MP4, while the same codec from the page path (yt-dlp) goes to MKV/WebM. The product gives different containers for the same video depending on which pipeline found it, and fixes to codec handling must be made twice.
- fix: Move `VideoCodec`, `AudioCodec` and the container capability table into one place both crates depend on (streams is the lower layer, so `wirecatch_streams::codec`, re-exported for core), parse codec strings once into those enums when building `Track`, and have `container_for` and `StreamInfo::extension` use `Container::holds_video/holds_audio`. Decide the VP9/AV1-in-MP4 policy once and test it in that module.
- cross-package: yes (core)

## STR-5 ffmpeg invocation is implemented twice, and the two have drifted
- severity: medium
- category: duplication
- location: crates/streams/src/ffmpeg.rs:265-304, crates/streams/src/ffmpeg.rs:308-369, crates/core/src/media/merge.rs:27-52, crates/core/src/media/merge.rs:92-97, crates/core/src/media/process.rs:72-100
- problem: `ffmpeg::arguments` (streams) and `merge::arguments` (core) build the same `-hide_banner -nostdin -loglevel error -y` prefix and stream-copy tail; `file_argument` and core's `file_arg` are the same function; `ffmpeg::run` and `process::run` both spawn with `CREATE_NO_WINDOW` (the `0x0800_0000` literal appears in both). They have already drifted: core adds `-movflags +faststart` for MP4/M4A, streams does not, so stream downloads produce MP4s that cannot start playing until fully read; core runs ffmpeg in a job object / process group with an explicit working folder and timeout support, streams uses only `kill_on_drop` and inherits the app's current directory.
- fix: Expose one argument builder from streams (for example `ffmpeg::copy_arguments(inputs, maps, output, container)` that adds `+faststart` for MP4-family outputs) and make `merge::arguments` call it. Have streams run ffmpeg through an injected runner, or move core's `process::run` (job object, process group, working folder) down into a shared module that both use.
- cross-package: yes (core)

## STR-6 Parsers return `Result<_, String>`, and the public errors are stringly typed
- severity: medium
- category: error-handling
- location: crates/streams/src/hls.rs:89, crates/streams/src/hls.rs:105, crates/streams/src/hls.rs:180, crates/streams/src/dash.rs:140, crates/streams/src/dash.rs:313, crates/streams/src/plan.rs:24-28, crates/streams/src/error.rs:34-46, crates/streams/src/error.rs:50-56, crates/streams/src/error.rs:84-88, crates/streams/src/hls.rs:683, crates/streams/src/hls.rs:695, crates/streams/src/dash.rs:947, crates/streams/src/dash.rs:953
- problem: Every parser and `Segments::pieces` returns `String` errors, so `Error::Manifest(String)`, `Error::Drm(String)` and `Error::Unsupported(String)` carry free text. Callers cannot branch on the cause (core maps all of `Manifest` to "refresh link", including "more than 200000 segments", where refreshing cannot help), message wording can only be tested by substring (`error.contains("past any possible file size")`, `contains("too large")`, `contains("more than 200000 segments")`), and `NoSuchTrack::kind` / `UnknownFormat::role` are `&'static str` although a `Role` enum exists. Rewording a message breaks tests and any future localization.
- fix: Add a `pub(crate) enum ManifestError` (`NotM3u`, `NoSegments`, `TooManySegments`, `BadByteRange { value }`, `ByteRangeOverflow`, `BadIv`, `UnclosedTemplate`, `NumberTooLarge`, ...) with `thiserror` messages, return it from the parsers, and wrap it as `Error::Manifest(#[source] ManifestError)`. Replace `Drm(String)` with a `DrmSystem` enum, and make `Role` public (`#[non_exhaustive]`) for `NoSuchTrack` and `UnknownFormat`. Tests then use `matches!`.
- cross-package: yes (core, for matching on the new variants)

## STR-7 A DASH track with no media segments "completes" as an empty file
- severity: medium
- category: error-handling
- location: crates/streams/src/dash.rs:177-247, crates/streams/src/dash.rs:425-456, crates/streams/src/plan.rs:632-643, crates/streams/src/download.rs:399-409, crates/streams/src/hls.rs:293-295
- problem: HLS refuses a playlist without segments ("it lists no segments"), but DASH has no equivalent check. A `SegmentList` without `SegmentURL`, or a duration-based template whose count is 0 (for example `mediaPresentationDuration="PT0S"`, or `Period@start` past the end), produces a track with zero or only an init piece. `download` then concatenates nothing, `InputFormat::detect` returns `None` for the empty file, the no-ffmpeg path falls back to `.ts`, and the result is `Outcome::Completed { bytes: 0 }`: the user sees a successful download of an empty video. `plan_dash` also turns `into_pieces()`'s `None` into an empty list with `unwrap_or_default()`, hiding the same condition if the `Indexed` arm ever changes.
- fix: In `plan_dash` (or `expand` and the `SegmentList` branch), return `ManifestError::NoSegments` when a track has no non-init piece, matching HLS; replace `unwrap_or_default()` with an explicit error. Add a DASH test for an empty `SegmentList` and for `PT0S`.
- cross-package: no

## STR-8 `download::run` and `download::join` are god functions
- severity: medium
- category: design
- location: crates/streams/src/download.rs:58-159, crates/streams/src/download.rs:312-423
- problem: `run` does path resolution, fetcher setup, planning, the ffmpeg precondition, work-folder opening, resume bookkeeping (three counters updated in two branches), job queue building, worker pool management, error aggregation and pause handling in one 100-line function. `join` (110 lines, four levels of nesting) mixes per-track concat, container choice, format detection, the MP4-then-Matroska retry loop, renaming and cleanup, with the ffmpeg and no-ffmpeg paths as two large match arms. The resume accounting (`done`, `bytes`, `number`) is the logic most likely to regress and is not unit-testable in this shape.
- fix: Split into `fn resume_state(plan, &WorkDir) -> (VecDeque<Job>, done, bytes)` (pure, unit-tested), `async fn download_pieces(shared, jobs) -> Result<Option<Outcome>>`, `async fn mux_with_ffmpeg(...) -> Result<MuxResult>` and `fn save_single_track(...)`. Keep `run` and `join` as short orchestration.
- cross-package: no

## STR-9 `StreamInfo::extension` re-implements the track choice and container choice, and can disagree with `download`
- severity: medium
- category: duplication
- location: crates/streams/src/model.rs:54-94, crates/streams/src/plan.rs:423-436, crates/streams/src/plan.rs:493-524, crates/streams/src/plan.rs:591-604, crates/streams/src/download.rs:459-473, crates/streams/src/ffmpeg.rs:40-47
- problem: The forecast picks "Auto" video by `(bandwidth, area)` over all listed video and audio by `(default, bandwidth)`, while the real plan picks DASH by `(main, bandwidth, area)`, HLS audio by the chosen variant's `AUDIO` group then `default`/`autoselect`, and falls back to audio-only variants. The `(fits, audio_only) -> "mp4" | "m4a" | "mkv" | "mka"` table is a copy of `Container::extension`, and the codec fit test is a copy of `container_for`. Core names the output from this forecast (crates/core/src/media/choice.rs:220-270), so any divergence gives a file whose name core did not expect and a `video (1).mkv` style rename.
- fix: Have `StreamInfo` keep enough to resolve choices (or keep the parsed `Source`), and implement both `extension()` and `Source::plan()` on top of one `resolve_choice(video, audio) -> Selected` function and one `Container::for_codecs(...)`; `extension()` then returns `container.extension(audio_only)`.
- cross-package: no

## STR-10 The local HTTP test server exists in five copies
- severity: medium
- category: testing
- location: crates/streams/tests/common/mod.rs:1-247, crates/e2e/src/server.rs:1-203, crates/engine/tests/common/mod.rs, crates/core/tests/manager/common.rs, apps/desktop/src-tauri/tests/common/mod.rs
- problem: CLAUDE.md says tests that need a server use the one in `crates/engine/tests/common`. Instead streams ("in the style of the engine's test server") and e2e ("after the one in crates/engine/tests/common") each carry their own axum server with the same `Seen` struct, the same `bytes=` range parser, and the same xorshift data generator (seed `0x9E37_79B9_7F4A_7C15` in streams `segment()` and e2e `test_data()`). The copies already differ: the e2e parser panics on `bytes=10-5` (`slice(10..6)`), streams answers it with 206 and an inverted range, and only some copies support delays, failures, holds or ETags. A fix to range semantics must be made five times.
- fix: Create an unpublished `crates/testkit` (dev-dependency only) with one configurable server (files by path, content types, failure counts, delays, hold/release gate, request log with a `watch` for waiting) and the data generator; migrate streams and e2e first, then the others.
- cross-package: yes (engine, core, desktop)

## STR-11 Stream tests poll without a bound and assert on wall-clock time
- severity: medium
- category: testing
- location: crates/streams/tests/hls.rs:316-318, crates/streams/tests/hls.rs:361-363, crates/streams/tests/hls.rs:323, crates/streams/tests/hls.rs:364-371, crates/streams/tests/hls.rs:787-797, crates/streams/src/hls.rs:693-696, crates/e2e/tests/e2e.rs:355-360, crates/e2e/tests/e2e.rs:414-418
- problem: `while progress.segments_done() < 10 { sleep(5ms) }` has no deadline: if the download errors before ten segments, the spawned task ends and the test spins forever instead of failing, hanging CI. `resuming_checks_segment_sizes` asserts `segments_left: 1` only because seg3 is delayed 400 ms, and `resuming_after_a_cancel` accepts a range `1..=20` because the result is timing-dependent. `speed_limits_apply_to_streams` asserts the unlimited run takes under 750 ms, which includes ten per-segment `fsync` calls (slow on Windows runners). e2e asserts `list_queues` answers within 1 s and an app cold start within 5 s on a loaded runner. These are the classic flaky patterns.
- fix: Expose a progress notification (a `watch` or `Notify` on `StreamProgress`, which also lets core stop polling) and wait with `tokio::time::timeout(PATIENCE, ...)`. Make the cancel point deterministic with the server's hold/release gate (as e2e does) instead of delays. Replace the unlimited-speed upper bound with a ratio assertion (`limited > unlimited * 2`) and keep only the lower bound against the rate. Put the e2e latency bounds behind a named constant with generous headroom, or assert ordering (answered before the capture is released) rather than a wall-clock limit.
- cross-package: no

## STR-12 Engine error mapping is copied three times, and `Error::Request` drops its source
- severity: low
- category: error-handling
- location: crates/streams/src/error.rs:150-158, crates/streams/src/download.rs:218-226, crates/streams/src/download.rs:292-299, crates/streams/src/error.rs:17-18
- problem: The `Cancelled -> Cancelled`, `PrivateNetwork -> PrivateNetwork`, else wrap mapping is written out in `From<wirecatch_engine::Error>` and again in `fetch_piece` and `Keys::get` (with a different wrapper each time). Adding an engine variant that needs special handling means finding all three. `Request(wirecatch_engine::Error)` uses `#[error("{0}")]` without `#[source]` or `transparent`, so `Error::source()` returns `None` and error-chain logging loses the engine's cause, unlike `Segment`, which has `#[source]`.
- fix: Add `fn map_engine(e, wrap: impl FnOnce(wirecatch_engine::Error) -> Error) -> Error` handling the two shared cases, use it in all three places, and mark `Request` as `#[error(transparent)]`.
- cross-package: no

## STR-13 Stringly-typed HLS key methods and DRM names, with the DRM id table duplicated
- severity: low
- category: design
- location: crates/streams/src/hls.rs:164-170, crates/streams/src/hls.rs:299-358, crates/streams/src/dash.rs:639-665, crates/streams/src/plan.rs:62-67, crates/streams/src/model.rs:115, crates/streams/src/model.rs:138, crates/streams/src/plan.rs:370-385
- problem: `Key { method: String, format: String }` is compared against `"AES-128"`, `"identity"` and `"NONE"` in four places. The Widevine and PlayReady system UUIDs are written in both `hls::refusal` and `dash::protection` (FairPlay and Clear Key only in one of them), and both return display strings that flow into `Error::Drm(String)`. `VideoTrack::format` and `AudioTrack::format` are `&'static str` ("mp4", "ts", "webm") although `InputFormat` already models these formats.
- fix: Add `enum KeyMethod { None, Aes128, SampleAes, Other(String) }` and `enum KeyFormat { Identity, Other(String) }` parsed once in `update_keys`; add one `drm::system_for(id: &str) -> DrmSystem` with a single UUID table used by both parsers; type the track `format` as `InputFormat` (or a small `SegmentFormat` enum) and call `.extension()` where a string is needed.
- cross-package: no

## STR-14 Per-segment allocations and copies of the whole piece list
- severity: low
- category: performance
- location: crates/streams/src/hls.rs:243-244, crates/streams/src/hls.rs:248-252, crates/streams/src/hls.rs:271, crates/streams/src/hls.rs:326-335, crates/streams/src/download.rs:106-111, crates/streams/src/dash.rs:290-303, crates/streams/src/crypto.rs:255, crates/streams/src/download.rs:249
- problem: With up to 200 000 segments per track: `parse_media` clones the map `Url` and range on every segment just to compare with `emitted_map`, clones the segment `Url` into `previous_end` whenever ranges are used, and clones the key `Url` into every `Encryption`; `run` clones every `Piece` (one or two heap `Url`s each) into a `Job` while the `Plan` is still held, doubling plan memory; `substitute` allocates via `value.to_string()` / `format!` per segment. In the hot download path, each encrypted segment allocates a fresh 256 KiB decrypt buffer plus a 256 KiB `BufWriter`. CLAUDE.md asks for no per-chunk allocations in hot paths.
- fix: Compare the map by reference (store an index or `Arc<Map>` and compare pointers); keep keys in a `Vec<Url>` and store a `key: u32` index in `Encryption`; move pieces out of the plan into jobs (keep only per-track counts, roles and codecs for joining); use `write!(out, "{value:0width$}")` in `substitute`; reuse one decrypt buffer per worker (pass it into `decrypt`, owned by the worker loop).
- cross-package: no

## STR-15 Zero-length byte ranges are accepted and become invalid HTTP ranges
- severity: low
- category: error-handling
- location: crates/streams/src/hls.rs:385-401, crates/streams/src/dash.rs:517-527, crates/engine/src/fetch.rs:486-490
- problem: HLS `#EXT-X-BYTERANGE:0@5` and a `sidx` reference with `referenced_size` 0 produce an empty `5..5` piece. The engine formats that as `bytes=5-4` (and `0..0` as `bytes=0-0`, which fetches one byte), so the request either fails after retries with an unclear "Part N could not be downloaded" or stores a byte that was never part of the media.
- fix: Reject `len == 0` in `hls::span` with a manifest error, and return `None` from `sidx_pieces` (falling back to the whole file) when a reference size is 0. Optionally assert non-empty ranges in `Piece::plain`.
- cross-package: no

## STR-16 The segment cap is enforced inconsistently
- severity: low
- category: error-handling
- location: crates/streams/src/hls.rs:237, crates/streams/src/dash.rs:187, crates/streams/src/dash.rs:216, crates/streams/src/dash.rs:235, crates/streams/src/dash.rs:442
- problem: HLS checks `pieces.len() >= MAX_SEGMENTS` before pushing, so a track holds at most 200 000 pieces including init sections; DASH checks `pieces.len() > MAX_SEGMENTS`, allowing 200 001 media pieces plus init. The per-entry `count > MAX_SEGMENTS` check in a timeline is redundant with the push check. The documented limit ("the most pieces one track may have") is not what DASH enforces.
- fix: Add one `fn check_room(pieces: &[Piece]) -> Result<(), ManifestError>` in `plan.rs` with the `>=` semantics and call it before every push in both parsers; drop the redundant per-entry check or keep it as an early exit using the same helper.
- cross-package: no

## STR-17 Peak disk use is twice the video size during joining
- severity: low
- category: performance
- location: crates/streams/src/download.rs:477-500, crates/streams/src/download.rs:367-381
- problem: `concat` copies every segment into `track<n>` and only removes the segments after the whole track is written, so a 10 GB video needs 20 GB free; the ffmpeg step then writes a full `joined.<ext>` next to the track files, again doubling. Users with nearly full disks get a "check that the folder has free space" error at 99 percent, after the whole download.
- fix: Remove each segment right after it has been appended, and record the appended index in the log (`A <track> <index> <offset>`) so an interrupted concat resumes by truncating the track file to the logged offset. Before joining, check free space against the known total and fail early with a clear message.
- cross-package: no

## STR-18 Magic values: about 40 inline literals that should be named
- severity: low
- category: magic-value
- location: crates/streams/src/plan.rs:208, crates/streams/src/plan.rs:224, crates/streams/src/plan.rs:311, crates/streams/src/plan.rs:318, crates/streams/src/plan.rs:342, crates/streams/src/plan.rs:374, crates/streams/src/download.rs:249, crates/streams/src/download.rs:403, crates/streams/src/download.rs:484, crates/streams/src/model.rs:78, crates/streams/src/model.rs:89-92, crates/streams/src/workdir.rs:43, crates/streams/src/workdir.rs:52, crates/streams/src/workdir.rs:108, crates/streams/src/workdir.rs:114, crates/streams/src/workdir.rs:149, crates/streams/src/ffmpeg.rs:102, crates/streams/src/ffmpeg.rs:338, crates/streams/src/hls.rs:300-323, crates/streams/src/hls.rs:343-346, crates/streams/src/dash.rs:652-654, crates/e2e/tests/e2e.rs:24, crates/e2e/src/app.rs:264, crates/e2e/tests/e2e.rs:355, crates/e2e/tests/e2e.rs:418
- problem: Pattern, roughly 40 occurrences. Worst examples: track-id prefixes `'v'`, `'r'`, `'d'` in about 13 `format!`/`index_of` sites (see STR-3); `"video"`/`"audio"` literals for `NoSuchTrack` in six places although `Role::name()` exists; the `"ts"` fallback extension in three files; buffer sizes `256 * 1024` (also `crypto::CHUNK`), `1024 * 1024`, `4096` (twice, meaning different things) and `512`; the work-folder layout `format!("t{track}")` in four places and the header version `"wirecatch-stream 1"` inline; DRM UUIDs written twice; HLS key method strings four times. In e2e the cookie `"session=e2e-cookie"` is both a constant in `tests/e2e.rs` and a literal in `app::capture`, so changing one silently breaks the cookie assertions.
- fix: Name them: `const FALLBACK_EXTENSION`, `const DECRYPT_BUFFER`, `const CONCAT_BUFFER`, `const DETECT_HEAD_BYTES`, `const DASH_SNIFF_CHARS`, `const WORKDIR_FORMAT_VERSION`; add `WorkDir::track_dir(track)` used by the four path builders; use `Role::name()` for the error kind; export the e2e cookie from `app` and use it in the tests.
- cross-package: no

## STR-19 Stream test helpers are copied into each test file with divergent settings
- severity: low
- category: testing
- location: crates/streams/tests/hls.rs:20-49, crates/streams/tests/dash.rs:16-43, crates/streams/tests/ffmpeg.rs:66-79, crates/streams/tests/ffmpeg.rs:18-30, crates/streams/tests/common/mod.rs:242-247
- problem: `options()`, `request()`, `run()` and `completed()`/`saved()` are redefined in each of the three test files, with `max_retries` 3, 2 and the default respectively, for no stated reason. `fake_ffmpeg()` returns `Option<PathBuf>` but can only return `Some` (it asserts otherwise). `temp_dir` creates pid-named folders in the system temp folder and never removes them, unlike e2e, which uses `tempfile`.
- fix: Move the shared helpers into `tests/common/mod.rs` with one documented retry setting; make `fake_ffmpeg()` return `PathBuf`; return a `tempfile::TempDir` from `temp_dir` so folders are cleaned up.
- cross-package: no

## STR-20 A panic in the fake extension's reader task is swallowed and misreported
- severity: low
- category: testing
- location: crates/e2e/src/extension.rs:131-143, crates/e2e/src/extension.rs:242-246
- problem: The stdout reader runs in a detached `tokio::spawn` and calls `panic!` when the bridge writes an invalid frame. A panic in a detached task does not fail the test; it only drops the sender, so the test then fails with "the bridge closed stdout without answering request N", pointing the developer at the wrong problem (a protocol framing bug looks like a crash or early exit).
- fix: Send the decode error through the channel (`mpsc::UnboundedSender<Result<Message, String>>`) and have `pump` panic with the real error on the test's own task, or keep the `JoinHandle` and check it when the inbox closes.
- cross-package: no

## STR-21 The e2e README describes ignored tests that no longer exist
- severity: low
- category: convention
- location: crates/e2e/README.md:19-23, crates/e2e/tests/e2e.rs
- problem: The README says two tests are `#[ignore]`d until the protocol server handles each request in its own task and gives the `--include-ignored` command; `tests/e2e.rs` has no `#[ignore]` (scenario 5, `a_capture_waiting_on_a_silent_server_does_not_delay_other_calls`, now runs). Stale docs in a test crate mislead whoever debugs a CI failure.
- fix: Delete the paragraph (or replace it with a note that scenario 5 covers per-request tasks).
- cross-package: no

## STR-22 Two diverging lists of media extensions decide output names
- severity: low
- category: duplication
- location: crates/streams/src/model.rs:321-340, crates/core/src/media/mod.rs:306-313
- problem: streams replaces an output extension only if it is in its 14-entry list (includes `m3u8`, `mpd`, `m4s`, `m2ts`; lacks `opus`, `ogg`, `flac`, `wav`, `3gp`, `flv`); core's 16-entry list has the opposite gaps. A stream saved to a name core produced as `clip.flac` or `clip.opus` becomes `clip.flac.m4a` instead of `clip.m4a`, while the page path would have replaced it.
- fix: Keep one list (in streams, exported, or in the shared codec/container module from STR-4) and use it from both crates; add the manifest/segment extensions there as "replaceable" entries.
- cross-package: yes (core)

Summary: 22 findings in crates/streams and crates/e2e: 1 critical, 2 high, 8 medium, 11 low.
The critical one is a reachable panic (Duration overflow from a crafted EXTINF or DASH duration) that aborts the app under `panic = "abort"`; the highs are resume being wiped by refreshed signed links and positional track ids that core persists.
The rest is cross-package duplication with core (codecs, containers, ffmpeg runner, test servers), stringly-typed errors and ids, and timing-dependent or hanging tests; no e2e or stream test reaches beyond the local test servers.

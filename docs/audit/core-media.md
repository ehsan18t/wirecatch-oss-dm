# Audit: crates/core/src/media and crates/core/src/addons

Scope: `crates/core/src/media/{mod,choice,convert,merge,output,process,ytdlp}.rs` and `crates/core/src/addons/{mod,archive,manifest,signature}.rs`. Every finding below was checked against the code at commit 9e1580d. No critical or high findings: the security-sensitive paths (archive extraction, signature pinning, ffmpeg input whitelisting, extension whitelisting, cookie file handling) are carefully built. The problems are in process lifecycle edges, encoder fallback policy, error modelling, module shape and duplication with `wirecatch-streams`.

## CMD-1 Windows child is assigned to its kill-job only after it is already running
- severity: medium
- category: concurrency
- location: crates/core/src/media/process.rs:101, crates/core/src/media/process.rs:102, crates/core/src/media/process.rs:207-226, crates/core/src/media/process.rs:277-304
- problem: `command.spawn()` starts the program normally and `Tree::attach` calls `AssignProcessToJobObject` afterwards. Any process the program creates before that call is not in the job, so `TerminateJobObject` on cancel, timeout or drop does not kill it. yt-dlp.exe is a PyInstaller one-file build whose bootloader starts a second process; if that happens inside the window (slow machine, AV hooks delaying the parent thread), Cancel kills only the bootloader and the real yt-dlp keeps downloading and holding files in the `.wcmedia` folder, which then cannot be deleted on Windows. The module doc promises "killed, with everything they started", which is not guaranteed. If `Job::for_process` fails, the program runs with no grouping at all and only a warning is logged.
- fix: spawn with `CREATE_SUSPENDED | CREATE_NO_WINDOW`, assign to the job, then resume (`NtResumeProcess` on the process handle, or enumerate the primary thread with Toolhelp32 and `ResumeThread`); alternatively create the process with `PROC_THREAD_ATTRIBUTE_JOB_LIST`. If the job cannot be created or assigned, kill the child and return `RunError::Start` instead of running ungrouped. Add a Windows test where the fake tool spawns its child as its very first action.
- cross-package: no

## CMD-2 `killpg` is sent to a process group id after its leader was reaped, and up to three times
- severity: medium
- category: unsafe
- location: crates/core/src/media/process.rs:152-157, crates/core/src/media/process.rs:178, crates/core/src/media/process.rs:235-254
- problem: `child.wait()` reaps the leader, then `tree.kill()` runs `killpg(group, SIGKILL)` immediately, again at line 178, and again in `Drop`. Once the leader is reaped and no other member is alive, the PGID is free and can be reused by an unrelated process group of the same user, which would then be SIGKILLed. The window is short and PID wrap-around makes it unlikely, but the SAFETY comment ("a group that is gone gives ESRCH") states the wrong invariant: the hazard is reuse, not absence. Calling kill three times widens the window for no benefit.
- fix: make `Tree` hold `Cell<Option<i32>>` and take the group on the first kill so later calls are no-ops; after the leader's exit kill exactly once. For a race-free version, observe exit with `waitid(P_PID, pid, WEXITED | WNOWAIT)` in `spawn_blocking`, `killpg` while the leader is still a zombie (the PGID cannot be reused then), and only then reap. Update the SAFETY comment to state the real invariant.
- cross-package: no

## CMD-3 Long-running yt-dlp and ffmpeg runs have no stall detection
- severity: medium
- category: concurrency
- location: crates/core/src/media/ytdlp.rs:443-449, crates/core/src/media/ytdlp.rs:478-480, crates/core/src/media/convert.rs:281-287, crates/core/src/media/convert.rs:306, crates/core/src/media/convert.rs:321, crates/core/src/media/convert.rs:336, crates/core/src/media/merge.rs:110-118, crates/core/src/media/merge.rs:132-135
- problem: yt-dlp downloads, conversions, subtitle conversions and joins all pass `timeout: None`. A hung hardware encoder driver, a yt-dlp fragment downloader stuck on a dead socket, or an ffmpeg blocked on a bad input leaves the download showing "Converting" or "Downloading" forever with no error and no Try again; only a manual Cancel recovers. As a consequence every `RunError::TimedOut` arm on these paths (with messages such as "The media add-on stopped answering") is dead code that suggests a protection that does not exist. A total timeout is wrong for multi-hour downloads, which is presumably why it was left out.
- fix: add an inactivity watchdog to `process::run` (for example `idle_timeout: Option<Duration>` that resets on every stdout or stderr line). yt-dlp is run with `--newline --progress` and ffmpeg with `-progress pipe:1`, so both emit a line at least every second while healthy; 2 to 5 minutes of silence is a safe stall signal. Delete the unreachable `TimedOut` arms or make them reachable through the new option.
- cross-package: no

## CMD-4 Any failed conversion permanently disables the hardware encoder and re-runs the whole conversion with every other encoder
- severity: medium
- category: error-handling
- location: crates/core/src/media/convert.rs:233-267
- problem: every `ConvertError::Failed` while a hardware encoder is in use is treated as "this encoder cannot handle this video": the encoder is inserted into the process-wide `failed` set and the conversion restarts with the next encoder, down to software. But the same ffmpeg run also reads subtitles, a metadata file and a cover, and maps audio; a damaged subtitle file, a bad cover, or a full disk makes the run fail for reasons unrelated to the encoder. The result is N complete re-encodes that all fail the same way (slow, with only the last reason reported), and NVENC/QSV disabled for the rest of the app session because of one bad input.
- fix: separate encoder failures from input failures before blacklisting: either run the probe clip again with the failing encoder and only blacklist if the probe now fails, or retry the same run once with video `-c:v` set to software and only blacklist if that succeeds. Never fall through encoders for errors whose stderr names an input file, and stop on the first non-encoder failure. Add a unit test with a fake ffmpeg that fails on the subtitle input.
- cross-package: no

## CMD-5 Encoder discovery is hidden global state that caches transient failures for the life of the app
- severity: medium
- category: design
- location: crates/core/src/media/convert.rs:37-51, crates/core/src/media/convert.rs:62-90, crates/core/src/media/convert.rs:108-111, crates/core/src/media/output.rs:1301-1329
- problem: (1) `known()` is a `static LazyLock<Mutex<Known>>` inside a library module, so all tests in the process share probe results and the `failed` set, and the manager cannot reset it (for example after the user updates GPU drivers). (2) `listed()` stores `None` in a `OnceCell` when `ffmpeg -encoders` fails or exceeds `PROBE_TIMEOUT` once; on Windows the first run of a freshly installed ffmpeg.exe plus seven DLLs is commonly delayed by Defender scanning, after which the whole session is limited to one software encoder and no hardware encoding. (3) That probe runs with `CancellationToken::new()`, so a user Cancel during the first conversion still waits up to 20 s. (4) With `listed == None`, `encoder_order` picks "the last software encoder in the array", silently coupling correctness to the order of `H264_ENCODERS`. (5) `encoder_order` takes a `failed` set but its only production caller passes `&HashSet::new()` and filters failures separately at line 135, so the parameter is dead.
- fix: move the cache into an `EncoderCatalog` owned by the manager (or stored next to `MediaTools`, keyed by the ffmpeg identity), injected into `convert`. Do not cache a failed `-encoders` call (use `get_or_try_init` and return `Err` on failure). Pass the caller's cancel token to the probe. Mark the fallback encoder explicitly (`Encoder::is_addon_default`) instead of relying on position, and either use `encoder_order`'s `failed` parameter or remove it.
- cross-package: no

## CMD-6 Blocking file system and Win32 calls on async tasks
- severity: medium
- category: performance
- location: crates/core/src/media/ytdlp.rs:334, crates/core/src/media/ytdlp.rs:339, crates/core/src/media/ytdlp.rs:421, crates/core/src/media/ytdlp.rs:597-625, crates/core/src/media/ytdlp.rs:628-634, crates/core/src/media/merge.rs:121, crates/core/src/media/convert.rs:298, crates/core/src/media/convert.rs:322
- problem: the project rule is "no blocking calls on async tasks". `extract_once` and `download_once` are `async fn`s that call `std::fs::create_dir_all`, open and write the cookies file, and run `owner_only::restrict`, which on Windows does token queries and `SetNamedSecurityInfoW` (crates/core/src/owner_only.rs:68-148). `CookiesFile::drop` runs `std::fs::remove_file` on the runtime thread, and `output.is_file()` is a synchronous stat after every ffmpeg run. The same module correctly uses `spawn_blocking` for `largest_result`, so the omission is inconsistent rather than deliberate. On a slow or network drive (or with AV hooks on file creation) these stall a runtime worker.
- fix: create the cookies file and its folder inside `tokio::task::spawn_blocking` (or the crate's `error::blocking` helper) and return the guard; delete it with `tokio::fs::remove_file` before returning, keeping a best-effort synchronous `Drop` only as the panic/cancel fallback. Replace `output.is_file()` with `tokio::fs::metadata(output).await.is_ok_and(|m| m.is_file())`.
- cross-package: no

## CMD-7 ffmpeg argument building and process running are implemented twice, once here and once in wirecatch-streams
- severity: medium
- category: duplication
- location: crates/core/src/media/output.rs:1430-1454, crates/core/src/media/merge.rs:28-53, crates/core/src/media/merge.rs:92-97, crates/core/src/media/output.rs:1460-1470, crates/core/src/media/output.rs:1632-1636, crates/core/src/media/process.rs:73-185, crates/streams/src/ffmpeg.rs:198-215, crates/streams/src/ffmpeg.rs:265-295, crates/streams/src/ffmpeg.rs:299-304, crates/streams/src/ffmpeg.rs:308-361
- problem: `output::local_input` is `streams::ffmpeg_input_args` re-typed with a `&str` demuxer instead of `InputFormat`; `file_arg` exists three times (`merge.rs`, `output.rs`, `streams::file_argument`); the preamble `-hide_banner -nostdin -loglevel error -y` is written out in four places; and there are three builders for "copy video plus audio into a container" (`merge::arguments`, `output::arguments` with Copy/Copy, `streams::arguments`). They already drift: `output::arguments` always names the muxer with `-f` ("so the extension never decides") while `merge::arguments` lets ffmpeg pick the muxer from the output extension. `streams::ffmpeg::run` is a second process runner without the process-group/job kill, Python-variable scrubbing or line handling of `media::process::run`, although both run the add-on's ffmpeg. The security rule D21.3 (protocol and format whitelist on every input) is therefore enforced by two code paths that must be kept in sync by hand.
- fix: make `wirecatch-streams` export one input builder that takes a demuxer name (`ffmpeg_input_args_for(demuxer, path)`, with `ffmpeg_input_args` as a thin wrapper), one `file_argument`, and a `FFMPEG_PREAMBLE` constant; delete `local_input` and both `file_arg`s here. Replace `merge::arguments` with `output::arguments` for an `OutputJob { video: Copy, audio: Copy }` so there is one join path with an explicit muxer. Longer term, let streams accept a runner (trait or closure) so core's `process::run` is the only place that spawns the add-on's programs.
- cross-package: yes (wirecatch-streams)

## CMD-8 Three unrelated error strategies, with String errors in library code and UI wording in the media layer
- severity: medium
- category: error-handling
- location: crates/core/src/addons/archive.rs:79-83, crates/core/src/addons/archive.rs:161-182, crates/core/src/addons/archive.rs:250-254, crates/core/src/addons/signature.rs:53-57, crates/core/src/media/choice.rs:87-91, crates/core/src/media/ytdlp.rs:670-697, crates/core/src/media/merge.rs:58-85, crates/core/src/media/merge.rs:122-145, crates/core/src/media/convert.rs:172-179
- problem: `archive` returns full user sentences as `String`, `signature` returns lower-case phrases as `String` that the caller must splice into a sentence, `choice::Plan::resolve` returns `String`, `ytdlp` and `merge` build protocol `UserError`s (including UI navigation text such as "Settings, Add-ons", 10 copies across the crate) directly inside the media layer, and only `convert` returns a typed `ConvertError` that the manager words. Consequences: callers and tests branch on message text (`error.contains("does not match")`, `contains("not offered")`), wording changes are behaviour changes, errors cannot be localized, and side effects follow the same split (`ytdlp` and `merge` call `tools.mark_broken()` themselves, while for `convert` the manager must remember to do it at media_run.rs:703 and :827). The crate rule is `thiserror` in libraries.
- fix: introduce `thiserror` enums per module (`ArchiveError { Unreadable, NotPlainFile(String), Duplicate(String), Missing(String), TooLarge, UnsafePath(String), Disk(io::Error) }`, `SignatureError`, `ResolveError`, `YtDlpError { Cancelled, TimedOut, Start(io::Error), Refused { reason }, BadOutput }`, `JoinError`), keep `RunError` as the shared source, and convert to `UserError` in exactly one place in the manager (next to `user_error.rs`). Do `mark_broken` in that one conversion for every `Start` variant.
- cross-package: no

## CMD-9 `output.rs` is a 1,700-line module with ten responsibilities and a 170-line argv builder
- severity: medium
- category: design
- location: crates/core/src/media/output.rs:18-330, crates/core/src/media/output.rs:443-836, crates/core/src/media/output.rs:838-1018, crates/core/src/media/output.rs:1020-1162, crates/core/src/media/output.rs:1164-1410, crates/core/src/media/output.rs:1394-1410, crates/core/src/media/output.rs:1460-1629, crates/core/src/media/output.rs:1542-1561, crates/core/src/media/output.rs:1662-1692
- problem: one file holds codec detection, container capabilities, variant planning (D24 policy), subtitle listing, subtitle and image sniffing, the ffmetadata writer, an ISO 639 table, the encoder catalog, the ffmpeg argv builder, `-progress` parsing and settings memory (`remembered_subtitles`). `arguments` is a 170-line function driven by closures that mutate a shared counter. Video encoders are a struct (`Encoder`) but audio encoders are an anonymous `(&str, &[&str])` tuple. `arguments` accepts states that cannot be valid: `VideoAction::Encode` with `encoder: None` falls into the `_ => -c:v:0 copy` arm (line 1560) and silently copies instead of failing.
- fix: split into `media/codec.rs` (VideoCodec, AudioCodec, Container), `media/variants.rs` (kept_ext, video_kbps, video_variants, audio_variants), `media/subtitles.rs` (listing, sniffing, iso639_2), `media/metadata.rs` (ffmetadata, escape), `media/encoders.rs` (Encoder, AudioEncoder struct, catalogs, ordering, probe args) and `media/ffmpeg_args.rs` (arguments, srt_arguments, progress parsing). Make the encoder part of the action (`VideoAction::Encode { codec, kbps }` resolved to a `VideoPlan::Encode { encoder: &'static Encoder, kbps }` before argv building) so the invalid combination cannot be expressed, and break `arguments` into `inputs`, `maps`, `codecs`, `metadata` helpers that each return `Vec<OsString>`.
- cross-package: no

## CMD-10 `MediaJob::Page` stores a stringly typed, redundant format selection built from unvalidated site ids
- severity: medium
- category: design
- location: crates/core/src/media/mod.rs:47-63, crates/core/src/media/choice.rs:386-429, crates/core/src/media/output.rs:705-711, crates/core/src/media/ytdlp.rs:424-427
- problem: `format: String` ("137+140") duplicates `video` and `audio`, is persisted in the database, and can disagree with them; every constructor rebuilds it with `format!("{}+{}", ...)` (choice.rs:390, output.rs:706). The ids are site-controlled `format_id` strings passed to yt-dlp's `-f`, which is a selector language where `+`, `/`, `,`, `[`, `(` have meaning; nothing in Wirecatch checks them, so correctness relies entirely on yt-dlp sanitising ids in its `-J` output. `choice::from_page` also computes the option id by pattern-matching the job it just built and `continue`s on variants that cannot occur (choice.rs:426-429).
- fix: introduce `struct FormatId(String)` with a constructor that accepts only `[A-Za-z0-9_.-]` (drop formats that fail) and `struct Selection { video: Option<FormatId>, audio: Option<FormatId> }` with a `to_ytdlp_arg()` method. Keep reading the old `format` field with `#[serde(default)]` for stored jobs but stop writing it (or derive it on serialize). Compute the option id from the selection directly.
- cross-package: no

## CMD-11 Add-on components are identified by string literals, and the public manifest types are unvalidated Strings
- severity: medium
- category: design
- location: crates/core/src/addons/mod.rs:36, crates/core/src/addons/mod.rs:154-156, crates/core/src/addons/mod.rs:225, crates/core/src/addons/manifest.rs:12-31, crates/core/src/addons/manifest.rs:61-88, crates/core/src/addons/manifest.rs:196, crates/core/src/addons/manifest.rs:256, crates/core/src/addons/manifest.rs:292, crates/core/src/addons/signature.rs:32-38, crates/core/src/manager/addons.rs:994-996
- problem: "yt-dlp", "ffmpeg" and "deno" appear as literals in `tools()`, `outdated()`, the manifest and about five places in `manager/addons.rs`; a typo compiles and silently means "not installed". `Record.components` is keyed by `String`. The re-exported public types (`lib.rs:31`) carry `name`, `url`, `sha256`, `fingerprint` and `ytdlp_release_downloads` as plain `String`s: `sha256` must be 64 lower-case hex digits but that is only asserted in a test of `official()`, so an upper-case or malformed value from a caller never matches and surfaces as a misleading checksum error. `name` is used as a path component without `safe_join` in `swap_in` (`dir.join(&component.name)` and `remove_dir_all(work.join(format!("old-{}", name)))`), so a name such as `../x` from a caller of the public API moves and deletes directories outside the add-on folder.
- fix: add `enum ComponentId { YtDlp, Ffmpeg, Deno }` with `as_str()`/serde rename for the record and folder names, and use it as the `BTreeMap` key and in `AddonComponent`. Add newtypes `Sha256Digest([u8; 32])` (parse once from hex, compare bytes, `Display` as lower-case hex), `Fingerprint`, and `Url` for the URL fields, validated at construction. Until then, run `component.name` through `archive::safe_join` in `swap_in`.
- cross-package: no

## CMD-12 Magic values: about 90 inline CLI flags plus tuning numbers and duplicated literals
- severity: low
- category: magic-value
- location: crates/core/src/media/output.rs:503-530, crates/core/src/media/output.rs:792, crates/core/src/media/output.rs:1181, crates/core/src/media/output.rs:1364-1389, crates/core/src/media/output.rs:1550, crates/core/src/media/ytdlp.rs:277, crates/core/src/media/ytdlp.rs:386, crates/core/src/media/ytdlp.rs:431, crates/core/src/media/ytdlp.rs:531, crates/core/src/media/ytdlp.rs:647-650, crates/core/src/media/mod.rs:350, crates/core/src/media/choice.rs:485, crates/core/src/media/output.rs:893, crates/core/src/addons/manifest.rs:140, crates/core/src/addons/manifest.rs:145, crates/core/src/addons/manifest.rs:179, crates/core/src/media/process.rs:96, crates/core/src/addons/archive.rs:30
- problem: outside tests there are about 62 ffmpeg flag literals in output.rs, 18 yt-dlp flags in ytdlp.rs, 8 in merge.rs and 3 in convert.rs. Worst examples: the D24 bit-rate policy is unnamed numbers in code (height table 500 to 35,000 kbps, clamp 300..80,000, default height 720, codec efficiency 1.8/1.4, maxrate 3/2 and bufsize 2x at line 1550, WAV 1,536 kbps); the probe clip (`320x240`, 25 fps, 10 frames, `1000k`); `/dev/dri/renderD128` hard-codes the first render node so multi-GPU Linux machines never get VA-API; `"wcprogress"` is written both as `PROGRESS_MARK` and inside the template string at line 431, so changing one breaks progress silently; the yt-dlp release download base URL is written twice in manifest.rs (145 and 179); validation limits (`35`, `1..=10`, `150`, `1e8`, `1e9`, `100_000`) are bare; the YouTube host list also exists in apps/extension/src/lib/media/page-sites.ts; `CREATE_NO_WINDOW` is hand-defined here, in crates/streams/src/ffmpeg.rs:323 and in apps/desktop/src-tauri/src/bridge.rs:187, and `FILE_SHARE_READ` is hand-defined although `windows-sys` (already a dependency) exports both.
- fix: name the bit-rate policy as constants next to `video_kbps` (`DEFAULT_SOURCE_HEIGHT`, `H264_KBPS_BY_HEIGHT: &[(u32, f64)]`, `MIN_VIDEO_KBPS`, `MAX_VIDEO_KBPS`, `MAXRATE_FACTOR`, `BUFSIZE_FACTOR`, `PCM_S16_STEREO_48K_KBPS`), build the progress template from `PROGRESS_MARK` with `format!`, derive the pinned yt-dlp URL from `ytdlp_release_downloads`, make the VA-API device discoverable (first `/dev/dri/renderD*` that opens), name the validation limits, and import `CREATE_NO_WINDOW`/`FILE_SHARE_READ` from `windows_sys` (shared helper for the three crates).
- cross-package: yes (wirecatch-streams, wirecatch-desktop, extension)

## CMD-13 Repeated outcome mapping and small helpers duplicated within the area
- severity: low
- category: duplication
- location: crates/core/src/media/ytdlp.rs:355-371, crates/core/src/media/ytdlp.rs:475-488, crates/core/src/media/merge.rs:110-145, crates/core/src/media/convert.rs:297-308, crates/core/src/media/convert.rs:321-338, crates/core/src/media/convert.rs:69, crates/core/src/media/convert.rs:147, crates/core/src/media/convert.rs:280, crates/core/src/media/convert.rs:320, crates/core/src/media/merge.rs:88-90, crates/core/src/media/choice.rs:294-307, crates/core/src/media/output.rs:549-575, crates/core/src/media/merge.rs:239-262, crates/core/src/media/convert.rs:357-380
- problem: the `match process::run(...) { Ok(ran) if ran.success && output.is_file() .. Cancelled .. TimedOut .. Start }` block is written five times with small differences (fallback text is "it gave no reason" in three places and "no reason given" in merge.rs; `to_srt` removes the partial output on failure but not on timeout); `output.parent().unwrap_or(Path::new("."))` appears five times and falls back to the app's working directory; `choice::rate`/`choice::best` are byte-for-byte `output::rate`/`output::best_audio`; the test helpers `real_ffmpeg()` and `make()` are copied verbatim between merge.rs and convert.rs tests (and a third `real_ffmpeg` lives in crates/core/tests/manager/media_support.rs:261).
- fix: add `process::run_ffmpeg(tools, args, output, opts) -> Result<Outcome, FfmpegError>` that does the success-and-file check, partial-output cleanup and reason extraction once; add `fn ffmpeg_dir(output: &Path) -> &Path` that requires a parent (paths are always absolute here); make `output::rate`/`best_by_rate` `pub(crate)` and use them from choice.rs; move the ffmpeg test helpers into a `#[cfg(test)] mod test_support` in `media`.
- cross-package: no

## CMD-14 `process::run` throws away diagnostics and passes untrusted text through unbounded
- severity: low
- category: error-handling
- location: crates/core/src/media/process.rs:110-114, crates/core/src/media/process.rs:179-181, crates/core/src/media/process.rs:333-336, crates/core/src/media/ytdlp.rs:372-375, crates/core/src/media/ytdlp.rs:688-697
- problem: stdout read errors are discarded (`let _ = ...read_to_end`), and output past `MAX_STDOUT` is silently truncated and returned as success, so a page description over 64 MB surfaces as "Update the media add-on" from the JSON parser; a `JoinError` from the collector becomes empty stdout; `child.wait()` errors and the exit code are collapsed into `success: bool`; a read error in `forward` ends the stream silently. Separately, `failed()` puts yt-dlp's `ERROR:` line (site-controlled text, up to `MAX_LINE` = 16 KiB) straight into the user-facing message with no length cap.
- fix: return `Ran { status: Result<ExitStatus, io::Error>, stdout, stdout_truncated: bool, stderr }` (or a `RunError::OutputTooLarge`) and log read and join errors; map `stdout_truncated` to its own message. Cap the reason embedded in user messages (for example 300 characters, cut at a char boundary with an ellipsis).
- cross-package: no

## CMD-15 yt-dlp `Format` fields are strict while the rest of `Info` is lenient
- severity: low
- category: error-handling
- location: crates/core/src/media/ytdlp.rs:75-85, crates/core/src/media/ytdlp.rs:112-150, crates/core/src/media/ytdlp.rs:233-237
- problem: the stated rule is "one odd field never makes a whole page unreadable" and `Info` applies `lenient` to most fields, but inside `Format` only `vbr` is lenient: `height: Option<u32>`, `filesize: Option<u64>`, `fps`, `tbr`, `abr`, `ext`, `protocol` and `http_headers: HashMap<String, String>` are strict. One extractor emitting `"height": 1080.0`, a numeric header value, or `"filesize": 1.5e6` fails `serde_json::from_slice::<Info>` for the whole page. Also `Info::all_formats()` clones the full format list (with header maps) on every call.
- fix: apply `#[serde(default, deserialize_with = "lenient")]` to every optional `Format` field (or a `#[serde_as]` helper that accepts integer-valued floats for `height`/`filesize`), and skip individual formats that fail instead of the page (deserialize `formats` as `Vec<serde_json::Value>` and keep the ones that parse). Return `Cow<'_, [Format]>` from `all_formats`.
- cross-package: no

## CMD-16 The add-on record treats every read error as "not installed" and is replaced without a flush
- severity: low
- category: error-handling
- location: crates/core/src/addons/mod.rs:65-73, crates/core/src/addons/mod.rs:76-82
- problem: `Record::load` maps every `io::Error` (permission denied, a sharing violation while an antivirus holds the file) to `Self::default()` without logging, so a transiently locked file makes the app offer a full reinstall of about 130 MB. `save` writes the temporary file and renames it without `sync_all`, so a power loss can leave an empty `installed.json`, which is then read as "not installed" as well.
- fix: return `Result<Record, io::Error>` and treat only `ErrorKind::NotFound` as empty; log and surface other errors (the slot can show "could not be read, try again"). In `save`, `File::sync_all` the temporary file before `rename` (and the directory on Unix).
- cross-package: no

## CMD-17 The program that is run is not required to be one of the hashed files
- severity: low
- category: security
- location: crates/core/src/addons/mod.rs:136-158
- problem: `tools()` verifies size (and with `deep`, SHA-256) of each `component.files` entry, then returns `safe_join(folder, component.program)` without checking that `program` is one of those files. The manifest test asserts this for `official()` only. A record whose `program` names an unlisted file (a bug in a future manifest, or a record written by an older version) is executed with no integrity check at all, which defeats the "nothing half-installed or damaged is ever run" guarantee.
- fix: in `tools()`, return `None` (with a warning) unless `component.files.iter().any(|f| f.path == component.program)`; enforce the same invariant when building an `AddonComponent` (constructor instead of public fields, see CMD-11).
- cross-package: no

## CMD-18 yt-dlp shares the user's own yt-dlp cache
- severity: low
- category: security
- location: crates/core/src/media/ytdlp.rs:559-585
- problem: `common_args` isolates configuration (`--ignore-config`, `--no-plugin-dirs`, `--no-js-runtimes`) but not the cache, so the add-on's yt-dlp reads and writes the shared `~/.cache/yt-dlp` (or `XDG_CACHE_HOME`), including cached YouTube player and signature data written by whatever other yt-dlp version the user runs. Behaviour then depends on machine state outside Wirecatch's control, which the module doc says it avoids, and stale entries from a different version are a known source of extraction failures.
- fix: pass `--cache-dir <data_dir>/addons/media/cache` (inside the private data folder) via a `cache_dir` field on `Context`, and clear it when yt-dlp is updated.
- cross-package: no

## CMD-19 Any yt-dlp failure on YouTube triggers a second full run with cookies
- severity: low
- category: performance
- location: crates/core/src/media/ytdlp.rs:294-308, crates/core/src/media/ytdlp.rs:369-371, crates/core/src/media/ytdlp.rs:393-412, crates/core/src/media/ytdlp.rs:486-488
- problem: `Attempt::Refused` is "yt-dlp exited non-zero", so "Video unavailable", "This video has been removed", a network error or an unsupported URL all repeat the whole extraction (several seconds) or the whole download with the browser's cookies, doubling time-to-error and sending the user's signed-in cookies to YouTube for requests that cannot benefit from them.
- fix: classify the error line before retrying: retry with cookies only for sign-in related failures (yt-dlp's "Sign in to confirm", "age", "members-only", "Private video", HTTP 401/403 messages), kept as a named list next to `cookies_first`; return other failures directly.
- cross-package: no

## CMD-20 Unneeded synchronization, dead state and boolean-typed channels
- severity: low
- category: convention
- location: crates/core/src/media/ytdlp.rs:441-471, crates/core/src/media/ytdlp.rs:489, crates/core/src/media/process.rs:73-81, crates/core/src/media/process.rs:103, crates/core/src/media/process.rs:326, crates/core/src/addons/mod.rs:136, crates/core/src/media/choice.rs:190
- problem: in `download_once` the `on_line` closure is `FnMut`, yet it writes through a `Mutex<Option<PathBuf>>` and two `AtomicU64`s; `done` is stored and never read, and `total` is stored and immediately loaded back. `process::run` sends `(bool, String)` over its channel and converts the bool to the `Output` enum that already exists, and takes seven positional parameters including `collect_stdout: bool` and `timeout: Option<Duration>`. Other boolean parameters (`tools(.., deep: bool)`, `from_stream(.., ffmpeg: bool)`) make call sites unreadable (`tools(dir, &record, false)`).
- fix: use plain captured locals (`let mut printed = None; let mut total = 0;`) in the closure; send `(Output, String)`; replace the positional arguments with a `RunOptions { stdout: StdoutMode::{Collect, Lines}, timeout, idle_timeout }` struct; replace `deep: bool` with `enum Check { Sizes, Hashes }` and `ffmpeg: bool` with `FfmpegAvailable::{Yes, No}` or a container preference.
- cross-package: no

## CMD-21 `MediaProgress` hand-encodes an enum and optional values into atomics next to a mutex
- severity: low
- category: design
- location: crates/core/src/media/mod.rs:104-138, crates/core/src/media/mod.rs:180-206, crates/core/src/media/mod.rs:218-232, crates/core/src/media/mod.rs:288-297
- problem: `ProcessingStep` is mirrored by `STEP_*` u8 constants with a manual two-way mapping that must be updated when a step is added (an unknown value silently reads as "no step"); `None` is encoded as sentinels (`u16::MAX`, `0` for `retry_at` and `own_total`), so a real total of 0 is indistinguishable from unknown. The struct already takes a `Mutex` on every `read()`, so six independent `Relaxed` atomics buy nothing, and `start_over` resets them one by one, letting a concurrent `read()` observe `own == true` with `own_done == 0` or a stale step.
- fix: keep one `Mutex<State>` with `step: Option<ProcessingStep>`, `per_mille: Option<u16>`, `own: Option<(u64, Option<u64>)>`, `retry_at: Option<i64>` and the parts; updates arrive at most a few times a second, so the lock is not contended. If lock-freedom is wanted, add `ProcessingStep: TryFrom<u8>` in the protocol crate instead of the private constants.
- cross-package: no

## CMD-22 Truncating to `MAX_OPTIONS` drops the audio-only option
- severity: low
- category: testing
- location: crates/core/src/media/choice.rs:253-260, crates/core/src/media/choice.rs:451-479
- problem: both `from_stream` and `from_page` push the "Audio only" offer last and then `offers.truncate(MAX_OPTIONS)`. A stream with more than 11 distinct heights (or a page with more than 11 height rows) loses the audio-only choice, and the remembered "audio only" settings can no longer apply. No test covers more than four rows.
- fix: truncate the video rows to `MAX_OPTIONS - 1` before appending the audio offer, and add a test with 14 heights asserting the last offer is audio-only.
- cross-package: no

## CMD-23 Risky helpers lack direct tests
- severity: low
- category: testing
- location: crates/core/src/media/process.rs:73-185, crates/core/src/media/process.rs:326-351, crates/core/src/addons/archive.rs:101-145, crates/core/src/addons/archive.rs:161-166, crates/core/src/addons/archive.rs:221-230
- problem: `process::run` has one unit test (the Python variable filter); timeout, cancellation while output is still streaming, the `MAX_STDOUT` cap, stderr tail rotation and `MAX_LINE` truncation on a UTF-8 boundary are only exercised indirectly, if at all, through the integration fake tool. In `archive`, the duplicate-entry refusal (`claim`), the 1 GiB cap, a directory entry named like an expected file, and zip names with a `./` prefix (handled for tar at line 130 but not for zip) have no test; these are the checks that make extraction safe.
- fix: add unit tests that run the `fake_media_tool` example (or a tiny `#[cfg(test)]` helper binary) for timeout, cancel and long-line cases, including a multi-byte character straddling 16 KiB; add archive tests built with the existing `zip`/`tar_xz` helpers in crates/core/tests/manager/media_support.rs for a duplicated entry, an oversize entry (use a smaller cap behind `#[cfg(test)]`), a directory entry and a `./`-prefixed zip entry.
- cross-package: no

## Summary
23 findings: 0 critical, 0 high, 11 medium, 12 low.
The security-critical paths (archive extraction, signature pinning, ffmpeg input whitelisting, extension and result-path checks) are sound; the medium findings are process lifecycle gaps (Windows job race, Unix PGID reuse, no stall watchdog), encoder fallback policy and global caching, blocking I/O on async tasks, and duplication of ffmpeg plumbing with wirecatch-streams.
Structural fixes with the best return: one typed error model converted to `UserError` in the manager, newtypes for component ids, digests and format ids, and splitting `output.rs` along the lines in CMD-9.

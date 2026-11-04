# Cross-module flow: security-addon-unpacking

The media add-on from its pinned release through download, checksum and signature checks and unpacking to the moment the app runs its programs, where those programs are found, and what can swap them between the check and the run.

## Files read

Whole:

crates/addons/src/lib.rs
crates/addons/src/archive.rs
crates/addons/src/manifest.rs
crates/addons/src/signature.rs
crates/core/src/manager/addons/install.rs
crates/core/src/manager/addons/mod.rs
crates/core/src/manager/addons/updates.rs
crates/media/src/tools.rs
crates/files/src/discard.rs
crates/shell/src/elevation.rs
crates/streams/src/process/run.rs

The parts the flow passes through:

crates/media/src/ytdlp/mod.rs (all but its tests)
crates/media/src/ytdlp/download.rs (running yt-dlp, up to the arguments)
crates/media/src/ffmpeg.rs (all but its tests)
crates/media/src/describe.rs (the start failures and the shared ffmpeg wording)
crates/core/src/manager/media.rs (`analyse`)
crates/core/src/manager/media_run/mod.rs (handing the programs to a job)
crates/core/src/manager/mod.rs (`load_beside_the_list`, `Manager::open`, `with_addon_sources`)
crates/core/src/manager/settle.rs (`read_as_video`)
crates/core/src/manager/captures.rs (`insert_confirmed`)
crates/shell/src/host.rs (`data_dir`)
crates/settings/src/folders.rs (`KnownFolders`, `protected_folders`, `resolve_local`)
crates/settings/src/settings_file.rs (`merge_localized`, `localize_folders`)
scripts/package.mjs (the AppImage's libraries and entry point)
docs/DECISIONS.md (D20, D21, D38, D80)
contracts/03.md, contracts/11.md, contracts/13.md, contracts/16.md

Left out: `crates/core/src/manager/addons/tests.rs` and `crates/core/tests/manager/addons.rs` were only searched, for what they assert about the work folder, and `crates/streams/src/process/tree.rs` was taken from contract 13. Linux behaviour is read from the code, not run. Only the Windows ffmpeg release address was tried on the network; it answers 200.

## Findings

What held up, so it need not be checked again:

- **The trust chain.** Every part's checksum is pinned in the app or, for a newer yt-dlp, comes from a list signed by the pinned key. The check and the unpack share one open file, which on Windows nobody else can write, replace or delete. Entry names are only compared, never used as paths, and links and folders under an expected name are refused. A malformed tag cannot reach a URL or a file name, because `newer` lets only dotted numbers through. A replayed older release is refused by its `--version` answer, and a refused one is deleted.
- **Where programs are found.** The paths come only from `addons::tools`, built from the record's checked files under the data folder. No search of `PATH`, no environment override and no second source exists in `core`, `media` or `streams`. `with_addon_sources` has no caller outside the tests. yt-dlp is given the add-on's Deno and ffmpeg by absolute path, with configuration, plugins and updates turned off.
- **Swaps between the check and the run.** Another program of the same person can change a file after `tools` looked at it. D38.2 accepts this, and its premises hold in the code: an elevated start is refused (`shell/elevation.rs`) unless the account always runs elevated, the data folder is made owner-only by `Store::open` with a notice when that fails, and Linux file modes are explicit. `.work` is covered by the same reasoning. The one case the premise does not cover is the app itself writing there, in Xsecurity-addon-unpacking-1.
- **Crash during a swap.** Each point between the two renames and the record's save ends in `tools` returning `None` or in a checked program, and the next install repairs it.
- **The AppImage's `LD_LIBRARY_PATH`.** The programs inherit it, because `process::run` strips only `PYTHON*`. It holds only the three xkb libraries, which ffmpeg, Deno and yt-dlp do not use, so it causes no failure.

Contract 11 already records a stale `installed.json` written by an update check in flight (11-2) and repair going back to the pin (11-1). They are not repeated here. One addition for whoever validates 11-2: `whole_file::replace` uses one `.tmp` name and is not safe for two writers (contract 03), so the check's save and `swap_in`'s can also mix their bytes, and a damaged record reads as not installed.

### Xsecurity-addon-unpacking-1 [P2] A settings file can send downloads into the add-on's folders, and nothing checks what lands there before the next run

- Where: `crates/settings/src/folders.rs`, `protected_folders` (Windows) and `KnownFolders::resolve_local`; `crates/settings/src/settings_file.rs`, `localize_folders`; `crates/addons/src/lib.rs`, `tools`
- Category: wrong implementation
- Failure: on Windows, a settings file from elsewhere sets its default folder, a category folder or a queue folder to `{Home}/AppData/Local/Wirecatch/addons/media/ffmpeg`. The person imports it and later downloads a file a site named, for example a `.dll` that `ffmpeg.exe` or `yt-dlp.exe` imports and that is not one of Windows' KnownDLLs. The app writes it beside the program, the loader searches the program's own folder first, and the next media job runs the site's code. If the file has the name of an installed program and "when a file exists" is Replace, the program itself is overwritten and the same session runs it. D38.2's reasoning does not apply here: it is about a program already running as the person, while here the code comes from a website and the app itself puts it in place.
- Evidence: `resolve_local` refuses a folder "where a downloaded file could run by itself", but `protected_folders` on Windows lists only the Start menus, `WindowsApps`, Windows and the program folders. `KnownFolders` has no field for the data folder, so the check cannot know of it. The path is inside `{Home}`, so `is_outside_user_folders` is false and the import's report does not single it out. `tools` looks only at recorded files, so an unlisted one is never noticed, and a changed one is noticed only at the next start, a repair or a failed start. On Linux the same folder is normally inside `~/.local`, which the hidden folder rule refuses, though an `XDG_DATA_HOME` outside a hidden folder would lose that. Nothing reaches this through the bridge: `crates/protocol/src/extension.rs` carries no folder, and `captures.rs` takes a folder only from the person's answer. It was read, not run, and which DLLs the pinned programs import was not listed.
- Confidence: likely (certain for the folder passing the check; the code running depends on the person importing the file and then downloading the file)

### Xsecurity-addon-unpacking-2 [P2] A program that dies because a library is missing does not trigger the closer look at the add-on

- Where: `crates/media/src/describe.rs`, `cannot_start` and `ffmpeg_common`; `crates/media/src/ffmpeg.rs`, `run`; `crates/streams/src/process/run.rs`, `run`
- Category: wrong implementation
- Failure: while the app runs, a security product removes `avcodec-63.dll` or another library from the ffmpeg folder, the case the code names ("an antivirus took it"). `CreateProcess` succeeds and the child ends with `STATUS_DLL_NOT_FOUND`; on Linux the dynamic loader exits with 127. The run is therefore `Ok` with a failure status and becomes `FfmpegError::Failed`, whose reason is "it gave no reason (exit code ...)". `mark_broken` is not called, no `Check::Hashes` scan runs, the add-on still shows as installed, and every join or conversion fails the same way until the app restarts or the person installs again. The message does not point at the add-on.
- Evidence: `mark_broken` is called only for `YtDlpError::Start`, `FfmpegError::Start`, `E::FfmpegMissing` and `E::FfmpegRun(RunError::Start)`. `RunError::Start` comes only from the spawn failing. D38.2 promises the SHA-256 check "after a program could not be started", and `Check::Sizes` at the next start would find the missing file, so the gap lasts only for the session. Read, not run; the loader's behaviour is the systems' documented one.
- Confidence: likely

### Xsecurity-addon-unpacking-3 [P2] One bad newest release stops the check from offering an older good one, and any failed check forgets what was already found

- Where: `crates/core/src/manager/addons/updates.rs`, `new_ytdlp_releases` and `check_addon_updates`
- Category: wrong implementation
- Failure: the newest yt-dlp release has no signature file yet, lacks this system's file or cannot be fetched, while an older signed release at least two days old is in the list. `checked_release(...).await?` returns at the first candidate, so the older one is never looked at, although the function's own doc says it finds that release so the add-on can move to it meanwhile. `check_addon_updates` then sets `ytdlp_releases` to the empty list, which also drops a mature release an earlier check had verified. The next quiet try is 24 hours later. Whoever can publish on yt-dlp's page without its key, the attacker D21.1 is about, can keep this up by publishing an unsigned release with a higher tag: nothing is installed, as intended, but updates stop for as long as it stays among the newest, and a quiet check only logs it. A check while offline likewise makes an offered update disappear from the page until the next good check.
- Evidence: the `?` in the candidate loop, and `found` being `Vec::new()` on every `Err` arm before it replaces `state.addon.ytdlp_releases`. D21.1 says such a release is never installed and does not say the others should be skipped.
- Confidence: certain

### Xsecurity-addon-unpacking-4 [P3] Release files of versions no longer wanted stay in `.work` for good

- Where: `crates/core/src/manager/addons/install.rs`, `swap_in` and `fetch_part`; `crates/core/src/manager/addons/mod.rs`
- Category: inefficiency
- Failure: a download is named by its version and kept on purpose for a later try. If the app quits during a quiet yt-dlp update, or a found release fails to start, and a newer release or a new pin then installs, the older `yt-dlp-<version>.download` and any resume file stay. The same goes for an ffmpeg (77 MB) or Deno (43 MB) file left when an app update changes its pin. Nothing but stopping an install or removing the add-on ever deletes them. Each is at most a release's size, and they add up across versions.
- Evidence: `swap_in` deletes only the file of the version it just installed. `.work` is otherwise removed only in `cancel_addon_operation` and `remove_addon`. The test's "left in .work" assertion in `crates/core/tests/manager/addons.rs` covers only a plain install.
- Confidence: certain

## Suggestions

### Xsecurity-addon-unpacking-S1 A repair downloads only what is damaged

- Today: when `tools` is `None`, `addon_plan` takes every part, so one missing library downloads all three parts again, about 136 MB on Windows. The comment in `swap_in` says the opposite: "the next install repairs only what is missing".
- Better: `tools` could say per part what is intact, and the plan could take the parts that failed or are out of date. Package managers repair the same way, one damaged package rather than everything.
- Value: a repair after an antivirus took one file, or an install that failed in the swap, takes seconds and uses little data.

### Xsecurity-addon-unpacking-S2 Flush unpacked files before the record says they are there

- Today: `Record::save` is durable, but `copy_file` drops each file without `sync_all`, and the renames do not force the data out. After a power cut the record can be on disk while a program or library is empty, or on some file systems has its full size with zeros in it. `Check::Sizes` then passes, against the module's promise that a half-finished install never counts.
- Better: `sync_all` each file in `copy_file`, and flush the part's folder before the rename, as `whole_file` does for the record.
- Value: an install that survives a power cut is either whole or shown as needing repair, at a small cost, since an install writes few files and rarely runs.

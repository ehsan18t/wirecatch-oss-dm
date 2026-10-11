# Audit: crates/ipc, crates/bridge, crates/cli, crates/protocol

Scope read in full: ipc (lib, paths, unix, windows, tests/frames), bridge (main, caller, config, app, relay, raw_frame, shell_parent, install), cli (main), protocol (lib, frame, wire, extension; app skimmed for type shape). Findings verified against the committed code on `refactor/code-quality` (9e1580d). Note: the working tree has uncommitted `[lints] workspace = true` additions in every Cargo.toml (another agent), so items those lints catch (`missing_docs`, `undocumented_unsafe_blocks`) are left out here.

Overall verdict on the Win32 unsafe code: it is careful and, as far as I could verify, sound. Buffer sizes are checked (`current_user_sid`, `RegGetValueW`, `InitializeProcThreadAttributeList`), alignment is handled (u64/usize-backed buffers), LocalAlloc results are always freed by a guard, handle lifetimes are tied with borrows (`AttributeList<'a>`), the client opens pipes at identification level, and overlapped IO is delegated to tokio. The problems are in the security policy around it, in duplication, and in the ad hoc helpers that a crate would replace.

## IPB-1 An elevated app lets medium-integrity processes drive it through the pipe
- severity: high
- category: security
- location: crates/ipc/src/windows.rs:204, crates/ipc/src/windows.rs:326
- problem: `owner_only_sddl` hardcodes the mandatory label `S:(ML;;NWNR;;;ME)` regardless of the creating process's integrity. Nothing in the repo checks for elevation (no `TokenElevation`/`IsUserAnAdmin` anywhere). If the user starts Wirecatch with "Run as administrator", the pipe is created by a high-integrity process but labelled medium, and the DACL grants the user SID, which every medium token of that user carries. Any unelevated process of the user (including malware that has not passed UAC) can then open the pipe and issue `add_download`, `update_extension_settings` and so on to a process running with an administrator token that writes files and runs add-on programs. That turns the app into a UAC bypass. The label exists precisely to stop lower-integrity callers; pinning it to ME defeats it whenever the server is above ME.
- fix: Read the process token's `TokenIntegrityLevel` in `PipeListener::bind` and emit the label from it (`ME`, `HI`, `SI` via the integrity SID), so only callers at or above the server's level can open the pipe; or refuse to listen when `TokenElevation` is set and tell the user to start Wirecatch normally. Add a test that builds the descriptor for a high-integrity token and asserts the label SID is `S-1-16-12288`.
- cross-package: yes (apps/desktop, which decides what an elevated start should do)

## IPB-2 Dev endpoint is derived by string surgery, and on Linux release and dev builds share one lock
- severity: medium
- category: design
- location: crates/ipc/src/unix.rs:104, crates/ipc/src/paths.rs:12, crates/ipc/src/paths.rs:24, apps/desktop/src-tauri/src/host.rs:187
- problem: `wirecatch-ipc` keeps its naming private (`PIPE_PREFIX`, `SOCKET_NAME`, `LOCK_NAME`), so the desktop rebuilds a debug endpoint by `to_string_lossy()` plus `strip_suffix("app.sock")` / `replacen("wirecatch-app-", "wirecatch-app-dev-", 1)`, duplicating ipc's constants as literals and silently breaking if ipc renames anything (the fallback branch would return the release name unchanged). Worse, `SocketListener::bind` takes the lock at `dir.join(LOCK_NAME)`, which depends only on the folder, not on the socket. The dev socket `app-dev.sock` lives in the same folder, so on Linux a running release app holds `app.lock` and the debug app's `bind` fails with `AlreadyInUse` (and vice versa). The Windows path has no lock, so the two platforms behave differently, contradicting the intent of "release and debug builds keep separate folders" in host.rs.
- fix: Give ipc an explicit API such as `pub fn endpoint_for(channel: Channel) -> Result<Endpoint, IpcError>` (`Channel::Release | Channel::Dev`) that owns both the pipe name and the socket file name, and derive the lock file from the socket file name (`<socket>.lock`) instead of a fixed `app.lock`. Delete `dev_endpoint` from the desktop. Add a Linux test that binds two listeners on sibling sockets in one folder.
- cross-package: yes (apps/desktop)

## IPB-3 Product identity (ids, host name, binary names, folder names) is hardcoded across crates instead of one product config
- severity: medium
- category: magic-value
- location: config/extension-ids.json:3, crates/bridge/src/app.rs:23, crates/bridge/src/install.rs:53, crates/bridge/src/install.rs:415, crates/ipc/src/paths.rs:12, crates/ipc/src/paths.rs:41, apps/desktop/src-tauri/src/bridge.rs:13, apps/desktop/src-tauri/src/host.rs:171
- problem: The extension ids and host name do have a single source (`config/extension-ids.json`, embedded by `config.rs`), which is good, but every other product identifier is a scattered literal, and the tests re-type the ids rather than reading the config. For a product going commercial (rebrand, white-label, a second channel) every one of these must be found by hand. Full inventory, production code: (a) Chromium id `chdgnmpjoipbgfeldicipccjjghmagki`: only config/extension-ids.json:3 (good). (b) Firefox id `extension@wirecatch.app`: only config/extension-ids.json:5 (good). (c) Native host name `app.wirecatch.bridge`: config/extension-ids.json:6; also hardcoded in apps/extension/src/lib/native/status.ts:44 (comment describing the browser error text). (d) App binary name `wirecatch-app`/`wirecatch-app.exe`: crates/bridge/src/app.rs:23-27, apps/desktop/src-tauri/src/appimage.rs:19, apps/desktop/src-tauri/src/autostart.rs:145, apps/desktop/src-tauri/src/lifecycle.rs:291, apps/desktop/src-tauri/tauri.conf.json:4, apps/desktop/src-tauri/Cargo.toml:17. (e) Bridge binary name `wirecatch-bridge`: apps/desktop/src-tauri/src/bridge.rs:13-17, apps/desktop/scripts/prepare-bridge.mjs:39, crates/bridge/Cargo.toml:11. (f) Pipe and socket names: crates/ipc/src/paths.rs:12 (`\\.\pipe\wirecatch-app-`), :16 (`wirecatch`), :20 (`app.sock`), :24 (`app.lock`), :41 (`/tmp/wirecatch-<uid>`), re-typed in apps/desktop/src-tauri/src/host.rs:189-192. (g) Product folder `Wirecatch`: crates/bridge/src/install.rs:415 (manifest folder inside the app's data folder), apps/desktop/src-tauri/src/host.rs:171 and :173, apps/desktop/src-tauri/src/lib.rs:126 and :270, apps/desktop/src-tauri/src/download_windows.rs:467, apps/desktop/src-tauri/tauri.conf.json:3, :18, :51. (h) App identifier `app.wirecatch.desktop`: apps/desktop/src-tauri/tauri.conf.json:6, apps/desktop/src-tauri/src/lib.rs:419. (i) Manifest description `Wirecatch browser bridge`: crates/bridge/src/install.rs:53. (j) The product name inside user-facing strings: about 56 non-comment occurrences in the audited crates (ipc lib.rs 5, paths.rs 6, unix.rs 1, bridge app.rs 18, install.rs 7, main.rs 7, relay.rs 6, raw_frame.rs 1, cli main.rs 5). Test code re-typing the ids: Chromium id in crates/bridge/src/caller.rs:91, 96, 126-128; crates/bridge/src/config.rs:119, 123, 129; crates/bridge/src/install.rs:1091; crates/bridge/tests/bridge.rs:28, 270; apps/desktop/src-tauri/tests/bridge.rs:24. Firefox id in caller.rs:111, 116, 133, 134; config.rs:120; install.rs:1144, 1402; tests/bridge.rs:29, 273. Host name in caller.rs:110; config.rs:118, 146; install.rs:1128-1163, 1215-1216, 1257-1261, 1386-1397, 1470-1475, 1566; tests/bridge.rs:245, 265; crates/e2e/src/extension.rs:49; apps/extension/tests/helpers.test.ts:149, 162, 166, 174, 190. The bridge also coincidentally depends on the desktop's data folder name (`%LOCALAPPDATA%\Wirecatch`) through two unrelated literals.
- fix: Extend `config/extension-ids.json` into a `config/product.json` (product name, app identifier, app and bridge binary stems, data folder name, pipe/socket stems, host name, ids) and expose it through one tiny crate or a `wirecatch-protocol::product` module generated at build time (Rust consts plus a TS export for the extension). Make `PIPE_PREFIX`, `APP_FILE_NAME`, `BRIDGE_FILE_NAME`, the manifest folder and the desktop data folder read from it. Tests should assert against `Config::embedded()` values, keeping one golden test that pins the literal ids (the D22 guard).
- cross-package: yes (protocol, ipc, bridge, desktop, extension, e2e)

## IPB-4 Protocol versioning is an exact-match integer with no capability negotiation or unknown-variant fallback
- severity: medium
- category: design
- location: crates/protocol/src/lib.rs:21, crates/protocol/src/wire.rs:264, crates/protocol/src/wire.rs:293, crates/protocol/src/wire.rs:317, crates/core/src/server.rs:136, apps/extension/src/lib/native/client.ts:264
- problem: `PROTOCOL_VERSION: u32 = 1` is compared with `==` on both sides, so any bump breaks every installed extension until both the store-distributed extension and the self-updated app catch up, and the two update channels never land at the same moment. Additive changes avoid the bump, but the enums (`Call`, `Reply`, `Event`, `ErrorCode`, `ClientKind`) have no `#[serde(other)]` catch-all, so a Rust peer one minor version behind fails to decode a whole message when it meets a new variant (an `Event` it does not know kills the frame, not just the event). There is also no way for the extension to learn whether the app supports a newer call before sending it. For a product with two independently updated halves this is the contract most likely to cause a support incident.
- fix: Put a supported range and capabilities in the handshake: `Hello { protocol: { min, max } }` and `HelloReply { protocol_version: u32 /* negotiated */, features: Vec<Feature> }`, with `Feature` an enum that has an `#[serde(other)] Unknown` variant. Add `#[serde(other)] Unknown` to `ErrorCode` and an `Unknown` arm for `Event` (or decode events as `serde_json::Value` first) so old peers degrade instead of failing. Document the compatibility rule (additive changes never bump) next to `PROTOCOL_VERSION`.
- cross-package: yes (core, extension, packages/protocol)

## IPB-5 Add-on ids are free strings, the protocol docs name ids that no longer exist, and the real id lives in core
- severity: medium
- category: design
- location: crates/protocol/src/app.rs:333, crates/protocol/src/app.rs:337, crates/protocol/src/app.rs:498, crates/protocol/src/app.rs:613, crates/protocol/src/app.rs:656, crates/core/src/addons/manifest.rs:7, apps/desktop/src-tauri/src/questions.rs:110
- problem: `AddonInfo.id: String` is documented as "Stable id, for example `ffmpeg` or `yt-dlp`", but since D20 the only add-on id is `"media"` (`MEDIA_ADDON_ID` in core). `SettingsProfile.addons`, `ImportSummary.addons_to_offer` and `ImportPreview.addons_to_offer` are `Vec<String>` too, so a settings profile, the UI and the core agree on ids only by convention, and the desktop test re-types `"media"`. A typo or a rename compiles everywhere and fails at runtime (an import silently offers nothing). The generated TS type is `string`, so the UI cannot switch exhaustively either.
- fix: Add `pub enum AddonId { Media, #[serde(other)] Unknown }` to the protocol (snake_case), use it in `AddonInfo`, `SettingsProfile.addons` and both `addons_to_offer` fields, delete `MEDIA_ADDON_ID` from core, and fix the stale doc comments. The `Unknown` arm keeps profiles exported by newer versions importable.
- cross-package: yes (core, desktop, packages/protocol)

## IPB-6 Win32 helpers are hand-rolled three times, including a full unsafe registry wrapper a maintained crate provides
- severity: medium
- category: unsafe
- location: crates/bridge/src/install.rs:631, crates/bridge/src/install.rs:645, crates/bridge/src/install.rs:658, crates/bridge/src/shell_parent.rs:87, crates/bridge/src/shell_parent.rs:152, crates/ipc/src/windows.rs:144, crates/ipc/src/windows.rs:191
- problem: The NUL-terminated UTF-16 conversion exists three times (`to_wide` in ipc, `wide` in shell_parent, `wide` in the registry module). Handle ownership is done three different ways: std `OwnedHandle` in ipc, a custom `Handle` with a null check in shell_parent, and a custom `Key` guard in install. The registry module is about 160 lines of `unsafe` FFI (`RegGetValueW` retry loop with `ERROR_MORE_DATA`, `RegCreateKeyExW`, `RegSetValueExW`, `RegQueryInfoKeyW`, `RegDeleteKeyW`) whose every operation `windows-registry` (Microsoft's safe wrapper, already in the `windows-rs` family the repo uses through `windows-sys`) does in one safe call each. Every extra unsafe block is review and audit surface for a commercial codebase, and the duplicated guards drift (only `Handle` checks for null, only `LocalBox` is shared).
- fix: Replace the registry module with `windows-registry` (`CURRENT_USER.create(key)?.set_string("", value)`, `get_string`, `remove_tree`/`open` + `keys().count()`), which removes all unsafe from install.rs. In shell_parent use `HandleOrNull` to `OwnedHandle` (`OwnedHandle::try_from(HandleOrNull::from_raw_handle(..))`) instead of the custom `Handle`, and for `PROCESS_INFORMATION` wrap both handles in `OwnedHandle`. Keep one `to_wide` (or use `windows::core::HSTRING` if moving to the `windows` crate) in a small shared `win32` module.
- cross-package: no

## IPB-7 The CLI exits with status 0 when a download is paused by Ctrl+C
- severity: medium
- category: error-handling
- location: crates/cli/src/main.rs:105, crates/cli/src/main.rs:133, crates/cli/src/main.rs:142
- problem: The first Ctrl+C cancels, the engine returns `Outcome::Paused`, `main` prints "Paused with ... left" and returns `Ok(())`, so the process exits 0. Scripts and CI (`wirecatch URL && next-step`) treat an interrupted, incomplete file as success. The second Ctrl+C exits 130, so the two interrupts disagree. The `other => println!("Finished: {other:?}")` arm prints a Rust `Debug` dump to the user for any future `Outcome` variant, against the plain-language rule.
- fix: Return `ExitCode` from `main`: 0 for `Completed`, 130 (or a documented nonzero code such as 2) for `Paused`, 1 for errors. Replace the `{other:?}` arm with a plain sentence ("Wirecatch stopped before the download finished") and still exit nonzero.
- cross-package: no

## IPB-8 The shell-parent launch does not check whose shell it borrows, and races on the PID
- severity: low
- category: security
- location: crates/bridge/src/shell_parent.rs:25, crates/bridge/src/shell_parent.rs:45
- problem: With `PROC_THREAD_ATTRIBUTE_PARENT_PROCESS` the child inherits the parent's token and job, not the bridge's. `spawn` takes whatever process owns `GetShellWindow()` and opens it with `PROCESS_CREATE_PROCESS` without checking that its token user (and integrity) match the bridge's. If the bridge runs as a different account than the desktop shell (browser started with "Run as different user" by an admin), the app is launched as the shell's user, listens on that user's pipe, and the bridge times out after 5 s with a misleading "did not respond" message. Between `GetWindowThreadProcessId` and `OpenProcess` the PID can also be recycled if Explorer restarts, so the parent could be an unrelated process of the same user.
- fix: After `OpenProcess`, open the shell's token (`OpenProcessToken(parent, TOKEN_QUERY)`) and compare its user SID with `Sid::current_user()` (move `Sid` to a shared helper or re-export it from ipc); fall back to the normal spawn on mismatch. Re-check `GetShellWindow()` maps to the same PID after opening to close the recycle window.
- cross-package: yes (ipc, if `Sid` is exposed)

## IPB-9 The Windows listener never checks its client, though the docs say both ends check
- severity: low
- category: security
- location: crates/ipc/src/lib.rs:11, crates/ipc/src/windows.rs:348, crates/ipc/src/unix.rs:138
- problem: The crate docs state "both ends check who is on the other side". On Linux the listener checks `SO_PEERCRED` and returns `PeerRefused`; on Windows `PipeListener::accept` accepts whoever the DACL let in and never asks who connected. The DACL is correct, so this is not exploitable today, but the two platforms give different guarantees behind one API, the doc is inaccurate, and the Windows side has no defense in depth if the descriptor ever regresses (for example the label issue in IPB-1).
- fix: After `connect()` on Windows, get the client's identity (`GetNamedPipeClientProcessId` then `OpenProcess` + token user, or `ImpersonateNamedPipeClient` + `OpenThreadToken` + `RevertToSelf` at identification level) and return `IpcError::PeerRefused` unless it is the current user, mirroring unix.rs. Otherwise correct the module docs to say the Windows server relies on the descriptor alone.
- cross-package: no

## IPB-10 Endpoint names are predictable, so another local user can deny service before Wirecatch starts
- severity: low
- category: security
- location: crates/ipc/src/paths.rs:12, crates/ipc/src/paths.rs:41
- problem: The Windows pipe namespace is global per machine and the name is `wirecatch-app-<user SID>`; SIDs are not secret, so any other local account can create that pipe first and the victim's app refuses to start until reboot (`EndpointTaken`). The Linux `/tmp/wirecatch-<uid>` fallback has the same property. The code detects and explains this well (nothing is exposed), but it remains a cheap cross-user denial of service on shared machines (terminal servers, labs) that a commercial product may be sold into.
- fix: On Windows, add an unguessable component: generate a random suffix at first run, store it in a file under the user's `%LOCALAPPDATA%` (already private to the user), and have `endpoint()` read it; clients read the same file. On Linux prefer `$XDG_RUNTIME_DIR` and only fall back to a `mkdtemp`-style random folder recorded in `~/.cache`.
- cross-package: no

## IPB-11 Secrets in protocol types and CLI errors are printable through Debug and error text
- severity: low
- category: security
- location: crates/protocol/src/extension.rs:9, crates/protocol/src/extension.rs:26, crates/protocol/src/extension.rs:28, crates/cli/src/main.rs:91, crates/cli/src/main.rs:93
- problem: `NewDownload` derives `Debug` while carrying `cookies`, `extra_headers` (which can hold `Authorization`) and `url` (which can hold FTP credentials). No current code logs it with `{:?}`, but `Call`, `Message` and every test assertion failure would print the user's session cookies the first time someone does, including in panic messages captured by crash reporting. In the CLI, `-H "Authorization: Bearer ..."` with a bad value is echoed whole into the error (`bad header value in {raw}`).
- fix: Introduce `pub struct Secret(String)` in the protocol with `#[serde(transparent)]`, `#[ts(type = "string")]` and a `Debug` that prints `"<redacted>"`, and use it for `cookies` and `Header::value`. In the CLI, report the header name only ("bad value for header Authorization").
- cross-package: yes (core, extension via generated types are unaffected because the TS type stays string)

## IPB-12 The bridge re-types the protocol's envelope tags and fully parses every app frame to find them
- severity: low
- category: duplication
- location: crates/bridge/src/raw_frame.rs:306, crates/bridge/src/relay.rs:199, crates/protocol/src/wire.rs:198
- problem: `RawFrame::envelope` matches the literals `"request"` and `"response"`, duplicating `#[serde(tag = "kind", rename_all = "snake_case")]` on `Message`. A rename in the protocol compiles and the bridge silently classifies every frame as `Other`, which disables duplicate-id refusal and the `AppUnavailable` answers for in-flight requests. It also deserializes `kind` into an owned `String` and walks the entire body (up to 1 MiB, for example a full `QueuesChanged` list) for every frame from the app, only to read two fields.
- fix: Export from the protocol a borrowed envelope type, for example `#[derive(Deserialize)] pub struct EnvelopeRef { pub kind: MessageKind, pub id: Option<u64> }` with `MessageKind` sharing the same serde renames as `Message`, plus a protocol test that round-trips each `Message` variant through it. Use it in the bridge, and only parse frames from the app when there are pending ids.
- cross-package: yes (protocol)

## IPB-13 Frame reading is implemented four times, and a header cut short is reported as a clean end
- severity: low
- category: duplication
- location: crates/protocol/src/frame.rs:85, crates/protocol/src/frame.rs:106, crates/bridge/src/raw_frame.rs:361, crates/bridge/src/raw_frame.rs:384
- problem: "read 4 bytes, map `UnexpectedEof` to `None`, `body_len`, allocate, `read_exact`" is written out in `frame::read`, `frame::read_async`, `raw_frame::read_blocking` and `raw_frame::read_async`. All four map `UnexpectedEof` from `read_exact(&mut header)` to `Ok(None)`, but `read_exact` also returns `UnexpectedEof` when 1 to 3 header bytes arrived before the stream ended, so a truncated frame is reported as a clean close and the peer's protocol error is hidden.
- fix: Add `frame::read_body` / `frame::read_body_async` to the protocol returning `Option<Vec<u8>>` (reading the header byte-by-count so a partial header is an `Io(UnexpectedEof)` error), build the typed readers on top of them, and let `RawFrame` wrap the returned body. The bridge's oversized-skip logic stays bridge-only.
- cross-package: yes (protocol)

## IPB-14 frame::encode allocates and copies every message twice
- severity: low
- category: performance
- location: crates/protocol/src/frame.rs:58
- problem: `encode` serializes into one `Vec`, then allocates a second `Vec` of `4 + len` and copies the body into it. Every message on every local connection (including 1 MiB settings or queue lists) pays an extra allocation and memcpy, against the repo's "no per-message allocations in hot paths" rule.
- fix: Serialize straight into the output: `let mut out = vec![0u8; 4]; serde_json::to_writer(&mut out, value)?; let len = out.len() - 4; if len > MAX_FRAME_LEN { return Err(TooLarge(len)) }; out[..4].copy_from_slice(&(len as u32).to_le_bytes());`.
- cross-package: no

## IPB-15 The default queue id is a literal in the protocol while core defines a constant for it
- severity: low
- category: magic-value
- location: crates/protocol/src/extension.rs:153, crates/core/src/settings.rs:18
- problem: `ExtensionSettings::default()` uses `QueueId(1)` while core exports `DEFAULT_QUEUE_ID: QueueId = QueueId(1)`. Two sources of truth for "the default queue" that only agree by coincidence; core depends on the protocol, so the constant is in the wrong crate.
- fix: Move `DEFAULT_QUEUE_ID` to `wirecatch_protocol` (next to `QueueId`), use it in `ExtensionSettings::default`, and re-export or import it in core.
- cross-package: yes (core)

## IPB-16 Test builds of the bridge overwrite the shipping binary name, and the override variable names are copied four times
- severity: low
- category: testing
- location: crates/bridge/Cargo.toml:26, crates/bridge/src/app.rs:46, crates/bridge/tests/bridge.rs:26, crates/e2e/src/bridge.rs:20, crates/e2e/src/bridge.rs:68, apps/desktop/src-tauri/tests/bridge.rs:27
- problem: `crates/e2e` and the desktop tests run `cargo build -p wirecatch-bridge --features test-overrides` into the normal target folder, so `target/debug/wirecatch-bridge(.exe)` (and `target/release/...` for release test runs) becomes the build that honors `WIRECATCH_BRIDGE_TEST_LAUNCH`. A developer's debug app registers `<its folder>/wirecatch-bridge` with every browser on start (D19), so after a test run the browsers on that machine start a bridge that launches whatever the environment names. Packaging is safe (prepare-bridge.mjs rebuilds without features), but the guard is a convention, not a build property. The two variable names are also re-declared in four places.
- fix: Build the override binary under a different name: a second `[[bin]] name = "wirecatch-bridge-test"` with `required-features = ["test-overrides"]` (same `main.rs`), and have e2e and desktop tests use that name. Export the variable names from one place (e2e already has them as `pub const`; the bridge's own tests can use `env!`-free constants from a `test_support` module).
- cross-package: yes (e2e, desktop)

## IPB-17 Unnamed timing and size literals in the IPC and bridge paths
- severity: low
- category: magic-value
- location: crates/ipc/src/windows.rs:395, crates/bridge/src/relay.rs:229, crates/bridge/src/install.rs:669, crates/bridge/src/install.rs:671
- problem: Most tunables are named (`BUSY_WAIT`, `START_BUDGET`, `FIRST_RETRY`, `MAX_RETRY`, `QUEUE_LEN`, `MAX_HELD_WHILE_CONNECTING`), but four are inline: the 10 ms busy-pipe retry, the 1 s grace in `finish_reading`, the 260-unit initial registry buffer and the 4 registry retries. They encode real decisions (how long the bridge waits for in-flight answers before dropping them) that are invisible next to their named siblings.
- fix: Name them (`BUSY_RETRY`, `DRAIN_GRACE`, `REGISTRY_INITIAL_UNITS`, `REGISTRY_RETRIES`) with a one-line reason each; the registry two disappear with IPB-6.
- cross-package: no

## IPB-18 install.rs is a 1,590-line module mixing data, planning, platform FFI, reporting and the command
- severity: low
- category: convention
- location: crates/bridge/src/install.rs:55, crates/bridge/src/install.rs:394, crates/bridge/src/install.rs:501, crates/bridge/src/install.rs:631, crates/bridge/src/install.rs:810, crates/bridge/src/install.rs:935
- problem: The browser table (with its provenance notes), the pure planner, the `Host` trait and file helpers, the Windows registry FFI, install/uninstall reporting and the terminal command all live in one file with about 630 lines of tests. Reviewing a change to the browser table means scrolling past FFI, and the unsafe code is harder to isolate for audit.
- fix: Split into `install/mod.rs` (command and report), `install/browsers.rs` (table), `install/plan.rs`, `install/host.rs` (trait, `RealHost`, files) and `install/registry.rs` (or drop it per IPB-6), keeping tests next to each part.
- cross-package: no

## IPB-19 install rewrites every manifest and key on every app start, through a fixed temp name
- severity: low
- category: performance
- location: crates/bridge/src/install.rs:810, crates/bridge/src/install.rs:537
- problem: D19 runs `wirecatch-bridge install` on every app start. `install` unconditionally rewrites each manifest (temp file plus rename) and each registry value even when they already hold the same bytes, so every start does several file writes and registry writes, bumps mtimes, and can trigger antivirus scans of the folder. `files::write` always uses `<name>.tmp`, so two concurrent installs (two app starts racing on first launch) can interleave on the same temp file.
- fix: In `install`, read first and skip the write when the file or value already matches (the `Host` trait already has `read_file` and `registry_value`), reporting "Already registered". Use a unique temp name (`<name>.<pid>.tmp`).
- cross-package: no

## IPB-20 Risky paths without tests, and private-endpoint test helpers copied six times
- severity: low
- category: testing
- location: crates/ipc/src/windows.rs:348, crates/bridge/src/install.rs:695, crates/bridge/src/app.rs:348, crates/ipc/src/unix.rs:288, crates/ipc/src/windows.rs:488, crates/ipc/tests/frames.rs:12, crates/e2e/src/app.rs:141, apps/desktop/src-tauri/tests/bridge.rs:31
- problem: Untested: `PipeListener::accept` recovery when a connect or the next `create_instance` fails (the listener must stay usable, as the public docs promise); the `ERROR_MORE_DATA` resize branch of the registry reader (the real-registry test only uses a short value); the shell-parent fallback path when `OpenProcess` fails. Meanwhile the "make a unique private endpoint plus cleanup guard" helper exists in six variants across ipc, bridge, e2e and desktop tests.
- fix: Add a `testing` module to `wirecatch-ipc` behind a `test-support` feature exposing `private_endpoint() -> (Endpoint, Guard)` and use it everywhere. Add a registry test with a value longer than 260 UTF-16 units (moot if IPB-6 lands) and a listener test that forces `new_instance` to fail (for example by binding with a descriptor that makes the second instance fail) and asserts the next `accept` still works.
- cross-package: yes (e2e, desktop)

## Summary
1 high, 6 medium, 13 low. The Win32 unsafe code itself is sound; the high finding is a policy gap (fixed medium label) that lets unelevated processes control an elevated app.
The medium items are about contracts between packages: endpoint naming owned by ipc but re-derived in the desktop (with a real Linux lock collision), product identity scattered across seven packages, an exact-match protocol version without capability negotiation, and stringly-typed add-on ids.
Cheapest wins: derive the pipe label from the token, move the lock name to the socket name, adopt `windows-registry`, return a nonzero exit code on CLI pause, and add a redacting `Secret` type.

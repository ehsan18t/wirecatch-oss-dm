# Chunk plan: desktop

- Scope: `desktop`, as `docs/audit/AUDIT-PLAN.md` defines it.
- Branch: `docs/record-open-items`
- Made from commit: `37603b330f1ec58dac95c3c68ba9fea0d77f2b18`
- Size limit: about 8,000 lines or 300 KB per chunk. The largest here is chunk 01 at 7,358 lines and 279 KB.
- Covered: 33 chunks, 143,859 lines, about 5,223 KB, 486 files.

## Dependency graph

From `cargo metadata --no-deps --format-version 1`, workspace crates only. "Used by" counts the crates with a normal dependency on it. The graph is an input to the order, not the order.

| Crate          | Depends on                                                                                                                                                  | Dev only                                              | Used by |
| -------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------- | ------- |
| `protocol`     | none                                                                                                                                                        | `test-support`                                        | 16      |
| `engine`       | none                                                                                                                                                        | `test-support`                                        | 13      |
| `product`      | none                                                                                                                                                        | none                                                  | 11      |
| `logging`      | `product`, `protocol`                                                                                                                                       | `test-support`                                        | 6       |
| `sites`        | `engine`, `protocol`                                                                                                                                        | `test-support`                                        | 6       |
| `files`        | `engine`                                                                                                                                                    | `test-support`                                        | 5       |
| `ipc`          | `product`, `protocol`                                                                                                                                       | `protocol`, `test-support`                            | 4       |
| `records`      | `protocol`, `streams`                                                                                                                                       | none                                                  | 4       |
| `core`         | `addons`, `engine`, `files`, `logging`, `media`, `messages`, `product`, `protocol`, `queues`, `records`, `settings`, `sites`, `store`, `streams`, `updates` | `files`, `logging`, `server`, `sites`, `test-support` | 4       |
| `streams`      | `engine`, `logging`                                                                                                                                         | `test-support`                                        | 3       |
| `queues`       | `protocol`                                                                                                                                                  | none                                                  | 3       |
| `addons`       | `engine`, `files`, `product`                                                                                                                                | none                                                  | 2       |
| `messages`     | `engine`, `protocol`, `sites`                                                                                                                               | none                                                  | 2       |
| `server`       | `core`, `engine`, `protocol`                                                                                                                                | none                                                  | 2       |
| `updates`      | none                                                                                                                                                        | `product`                                             | 2       |
| `media`        | `addons`, `engine`, `files`, `messages`, `protocol`, `records`, `sites`, `streams`                                                                          | `test-support`                                        | 1       |
| `settings`     | `engine`, `files`, `protocol`, `queues`, `records`, `sites`                                                                                                 | none                                                  | 1       |
| `store`        | `engine`, `files`, `product`, `protocol`, `records`, `sites`                                                                                                | `files`, `test-support`                               | 1       |
| `shell`        | `core`, `engine`, `ipc`, `logging`, `product`, `protocol`, `server`, `updates`                                                                              | `test-support`                                        | 1       |
| `test-support` | `product`, `protocol`                                                                                                                                       | none                                                  | 1       |
| `bridge`       | `ipc`, `logging`, `product`, `protocol`                                                                                                                     | `test-support`                                        | 0       |
| `cli`          | `engine`, `logging`, `product`                                                                                                                              | none                                                  | 0       |
| `desktop`      | `core`, `engine`, `ipc`, `logging`, `product`, `protocol`, `queues`, `shell`, `sites` (build: `product`)                                                    | `test-support`                                        | 0       |
| `e2e`          | `core`, `ipc`, `product`, `protocol`, `server`, `test-support`                                                                                              | none                                                  | 0       |

What the graph does not show, and the grouping relies on:

- `protocol` is depended on most but is mostly type definitions, so its plain types come late (chunk 30). The parts with logic travel with the code they guard: frames and versions in chunk 06, the browser's payload in chunk 07, queue rule types in chunk 18, proxy types in chunk 19, secrets in chunk 20.
- `core` is one crate in the graph and several jobs in practice (chunks 04, 05, 07, 10, 11, 12, 17, 18, 19 and 28 each hold part of it). Its modules share the one mutex in `manager/state.rs`, so a chunk of `core` reads `state.rs` as context from chunk 04's contract rather than owning it again.
- `shell` and `desktop` split by job, not by crate: a window's chunk holds the `shell` module that decides for it.

## Order and grouping in brief

The order runs: the path of a file's bytes to disk, then the boundary where outside input and other programs arrive, then what the app runs and installs, then media, then the rest of the manager, then the interface, then code with little logic.

| #   | Chunk                         | Risk   | Lines | KB  |
| --- | ----------------------------- | ------ | ----- | --- |
| 01  | engine-disk-resume (pilot)    | high   | 7,358 | 279 |
| 02  | engine-http                   | high   | 6,145 | 223 |
| 03  | saved-state                   | high   | 4,873 | 176 |
| 04  | core-run-lifecycle            | high   | 6,499 | 239 |
| 05  | core-final-path               | high   | 1,794 | 69  |
| 06  | bridge-and-frames             | high   | 6,796 | 251 |
| 07  | browser-handover              | high   | 5,696 | 210 |
| 08  | local-endpoint                | high   | 4,220 | 163 |
| 09  | request-routing               | high   | 2,506 | 91  |
| 10  | site-rules                    | high   | 2,019 | 75  |
| 11  | media-addon                   | high   | 5,276 | 197 |
| 12  | app-updates                   | high   | 1,432 | 48  |
| 13  | program-running               | high   | 2,023 | 76  |
| 14  | stream-manifests              | high   | 5,914 | 217 |
| 15  | stream-download               | high   | 4,040 | 149 |
| 16  | media-layer                   | medium | 6,903 | 251 |
| 17  | core-media-runs               | high   | 6,473 | 241 |
| 18  | queues-and-scheduling         | medium | 3,682 | 125 |
| 19  | settings-and-proxies          | medium | 5,596 | 203 |
| 20  | logging-and-secrets           | medium | 4,875 | 157 |
| 21  | engine-ftp                    | medium | 3,161 | 109 |
| 22  | app-start-and-quit            | medium | 5,113 | 192 |
| 23  | interface-commands-and-ui-kit | medium | 4,018 | 133 |
| 24  | download-windows              | medium | 4,719 | 168 |
| 25  | main-window                   | medium | 7,135 | 249 |
| 26  | notifications-and-tray        | medium | 2,899 | 100 |
| 27  | settings-and-queue-forms      | medium | 6,896 | 237 |
| 28  | failure-messages              | low    | 1,199 | 46  |
| 29  | packaging-and-release         | medium | 1,668 | 69  |
| 30  | protocol-types                | low    | 4,555 | 154 |
| 31  | test-servers                  | low    | 3,315 | 117 |
| 32  | cli                           | low    | 505   | 18  |
| 33  | repo-checks                   | low    | 4,556 | 191 |

## Pilot

Chunk 01, `engine-disk-resume`. It is the highest-value chunk, so its findings set the bar on the code that matters most. It is real logic with tests that can be judged, its findings are the easiest to prove with a failing test, and the coverage and fault-test results speak to it directly. It is also the largest chunk, so what it costs is an upper bound for projecting the rest of the run.

## Chunks

### 01. engine-disk-resume

- Job: carry one file from the first answer to its final name: split it into ranges, stream each to its offset on disk, save what is done so a later run continues, and finish without replacing anyone else's file.
- Risk: high
- Why here: file corruption and lost progress are made here, 13 crates depend on the engine, and the logic is real (range splitting, flush order, the decision to resume). Every later chunk of the download path relies on its contract.
- Why together: the offsets in `segment` (`claimed`, `flushed`, `end`) are shared by `worker`, `writer`, `checkpoint` and `state`. `abandon` and `file_work` guard the same blocking file work. `target`, `naming` and `storage` decide the file all of it writes to (D30.1, D30.7). `limiter` and `receive` sit inside the worker's loop, and `options` bounds it. `tests/download.rs`, `tests/options.rs` and `tests/request_limit.rs` drive this path through the public API.
- Size: 7,358 lines, 279 KB, 25 files
- Files:

```text
crates/engine/Cargo.toml
crates/engine/README.md
crates/engine/src/abandon.rs
crates/engine/src/checkpoint.rs
crates/engine/src/download.rs
crates/engine/src/file_work.rs
crates/engine/src/lib.rs
crates/engine/src/limiter.rs
crates/engine/src/naming.rs
crates/engine/src/numeric.rs
crates/engine/src/options.rs
crates/engine/src/progress.rs
crates/engine/src/receive.rs
crates/engine/src/segment.rs
crates/engine/src/segment/proptests.rs
crates/engine/src/state.rs
crates/engine/src/storage.rs
crates/engine/src/target.rs
crates/engine/src/units.rs
crates/engine/src/worker.rs
crates/engine/src/writer.rs
crates/engine/tests/common/mod.rs
crates/engine/tests/download.rs
crates/engine/tests/options.rs
crates/engine/tests/request_limit.rs
```

### 02. engine-http

- Job: turn requests into trusted bytes: ask for ranges, check every answer against the first, and retry or give up by one policy, over HTTP/1.1, HTTP/2 and HTTP/3, including the single-request `Fetcher`.
- Risk: high
- Why here: an answer wrongly accepted here (a file changed mid-download, a wrong `Content-Range`) corrupts what chunk 01 writes, so it has the same reach. The engine's `Error` is read by every layer above.
- Why together: `http/range`, `Validators` and `transport` are the one path HTTP and HTTP/3 share, and `Fetcher` follows the same rules through them. `retry` and `error` classify what those layers report. `tests/fetch.rs` and `tests/hardening.rs` test how servers are treated, and the benchmark examples drive these transports.
- Size: 6,145 lines, 223 KB, 16 files
- Files:

```text
crates/engine/examples/bench_http3.rs
crates/engine/examples/bench_local.rs
crates/engine/src/error.rs
crates/engine/src/fetch.rs
crates/engine/src/http.rs
crates/engine/src/http/client.rs
crates/engine/src/http/range.rs
crates/engine/src/http/status.rs
crates/engine/src/http/tests.rs
crates/engine/src/http3.rs
crates/engine/src/http3/tests.rs
crates/engine/src/request.rs
crates/engine/src/retry.rs
crates/engine/src/transport.rs
crates/engine/tests/fetch.rs
crates/engine/tests/hardening.rs
```

### 03. saved-state

- Job: keep the download list on disk: the saved types, the SQLite thread with its migrations and repair, and the helpers that write a file whole and keep it private.
- Risk: high
- Why here: a defect loses the whole list, not one file, and the database holds browser cookies. The manager's restart relies on this contract, so it comes before chunk 04.
- Why together: the store exists to save the types in `records`, checked by the compiler (D81), and the privacy of its folder and the names of its side files go through `files` (D17). `store/samples` and `store/tests/samples.rs` are the format's own test (D81.3).
- Size: 4,873 lines, 176 KB, 34 files
- Files:

```text
crates/files/Cargo.toml
crates/files/src/discard.rs
crates/files/src/error.rs
crates/files/src/lib.rs
crates/files/src/owner_only.rs
crates/files/src/whole_file.rs
crates/files/src/zone.rs
crates/records/Cargo.toml
crates/records/src/batch.rs
crates/records/src/lib.rs
crates/records/src/media/mod.rs
crates/records/src/media/output.rs
crates/records/src/origin.rs
crates/records/src/proxy.rs
crates/store/Cargo.toml
crates/store/samples/batches/example.json
crates/store/samples/download_error/example.json
crates/store/samples/last_ids/example.json
crates/store/samples/media/page_before_extras.json
crates/store/samples/media/page_before_subtitle_files.json
crates/store/samples/media/page_with_format_field.json
crates/store/samples/media/page_with_output.json
crates/store/samples/media/stream.json
crates/store/samples/media/unreadable.json
crates/store/samples/origin/forgot_sign_in.json
crates/store/samples/origin/with_headers.json
crates/store/samples/proxy_logins/example.json
crates/store/samples/proxy_servers/example.json
crates/store/samples/queue_settings/main.json
crates/store/samples/queue_settings/own_values.json
crates/store/samples/settings/defaults.json
crates/store/src/error.rs
crates/store/src/lib.rs
crates/store/tests/samples.rs
```

### 04. core-run-lifecycle

- Job: own every download's state in memory and move it through add, run, pause, stop and restart.
- Risk: high
- Why here: the heart of the manager, reviewed once the engine's and the store's contracts exist. Stop races, progress lost between saves and the restore of running downloads are all here.
- Why together: the one mutex in `state` is shared by all of it. `run` records how an engine run ended, `ticker` samples and saves progress, `restore` and `startup` rebuild what `run` left. `fake` and `manager/tests.rs` exist to make runs misbehave, and `tests/manager/lifecycle.rs` and `restart.rs` prove the same life from outside, on the harness in `common.rs`.
- Size: 6,499 lines, 239 KB, 19 files
- Files:

```text
crates/core/Cargo.toml
crates/core/examples/headless.rs
crates/core/src/clock.rs
crates/core/src/lib.rs
crates/core/src/manager/arrival.rs
crates/core/src/manager/downloads.rs
crates/core/src/manager/fake.rs
crates/core/src/manager/mod.rs
crates/core/src/manager/restore.rs
crates/core/src/manager/run.rs
crates/core/src/manager/startup.rs
crates/core/src/manager/state.rs
crates/core/src/manager/tests.rs
crates/core/src/manager/ticker.rs
crates/core/src/speed.rs
crates/core/tests/manager/common.rs
crates/core/tests/manager/lifecycle.rs
crates/core/tests/manager/main.rs
crates/core/tests/manager/restart.rs
```

### 05. core-final-path

- Job: settle a download's final name and folder from the server's answer, and handle a name already taken or a move to another folder.
- Risk: high
- Why here: `run` calls it, so it follows chunk 04. A mistake replaces or loses a file the person already had, which is P0 territory in few lines.
- Why together: all three flows hold and release the same name reservation (`naming::Claimed`), so two downloads never share a file (D88, D96).
- Size: 1,794 lines, 69 KB, 6 files
- Files:

```text
crates/core/src/manager/file_exists.rs
crates/core/src/manager/relocate.rs
crates/core/src/manager/settle.rs
crates/core/src/naming.rs
crates/core/tests/manager/file_exists.rs
crates/core/tests/manager/settle.rs
```

### 06. bridge-and-frames

- Job: carry frames between the browser and the app: read and bound each frame, agree a protocol version, apply the relay's rules, and start the app when needed.
- Risk: high
- Why here: it opens the boundary group. Everything a web page can influence through the extension arrives here as raw bytes, and the scope marks it untrusted.
- Why together: `raw_frame` and `relay` work directly on `frame`, `wire` and `version`, which set the limits they enforce (D14, D32). `caller`, `app` and `shell_parent` decide who is served and how the app starts (D41.3). `crates/e2e` starts the real bridge executable with a scripted extension, so its scenarios test this code first.
- Size: 6,796 lines, 251 KB, 24 files
- Files:

```text
crates/bridge/Cargo.toml
crates/bridge/src/app.rs
crates/bridge/src/caller.rs
crates/bridge/src/config.rs
crates/bridge/src/main.rs
crates/bridge/src/raw_frame.rs
crates/bridge/src/relay.rs
crates/bridge/src/shell_parent.rs
crates/bridge/tests/bridge.rs
crates/bridge/tests/needs_overrides.rs
crates/e2e/Cargo.toml
crates/e2e/README.md
crates/e2e/src/app.rs
crates/e2e/src/bin/wirecatch-e2e-app.rs
crates/e2e/src/bridge.rs
crates/e2e/src/extension.rs
crates/e2e/src/lib.rs
crates/e2e/src/server.rs
crates/e2e/tests/e2e.rs
crates/protocol/Cargo.toml
crates/protocol/src/frame.rs
crates/protocol/src/lib.rs
crates/protocol/src/version.rs
crates/protocol/src/wire.rs
```

### 07. browser-handover

- Job: serve one client connection and turn what the browser hands over into a question or a download, exactly once (D27), with only the headers and cookies that may go.
- Risk: high
- Why here: the next hop after chunk 06. A lost or doubled browser download and a cookie sent where it should not go are both decided here.
- Why together: `server` dispatches to `captures`, `extension.rs` holds the payload and its checks, `handovers` and `questions` hold the state a repeat or a withdrawal finds, and `headers` and `sign_in` decide which secrets are used and kept. `tests/manager/protocol.rs` is the server's own test suite (D79.2); `again.rs` and `windows.rs` test the same question flow (D23, D89).
- Size: 5,696 lines, 210 KB, 11 files
- Files:

```text
crates/core/src/headers.rs
crates/core/src/manager/captures.rs
crates/core/src/manager/handovers.rs
crates/core/src/manager/questions.rs
crates/core/src/manager/sign_in.rs
crates/core/tests/manager/again.rs
crates/core/tests/manager/protocol.rs
crates/core/tests/manager/windows.rs
crates/protocol/src/extension.rs
crates/server/Cargo.toml
crates/server/src/lib.rs
```

### 08. local-endpoint

- Job: make sure only the same user reaches the app's pipe or socket, in both directions, and that the endpoint's name stays private.
- Risk: high
- Why here: the same boundary from the other side, where another process on the computer is the threat (D41). It is heavy in system calls, which review finds what a single machine's tests do not.
- Why together: `paths` holds the pure rules, `key` the name under its lock, and `windows` and `unix` keep one promise on two systems behind one API. The tests use only that API.
- Size: 4,220 lines, 163 KB, 9 files
- Files:

```text
crates/ipc/Cargo.toml
crates/ipc/src/key.rs
crates/ipc/src/lib.rs
crates/ipc/src/paths.rs
crates/ipc/src/unix.rs
crates/ipc/src/windows.rs
crates/ipc/tests/channel.rs
crates/ipc/tests/frames.rs
crates/ipc/tests/keyed.rs
```

### 09. request-routing

- Job: decide for every request which proxy it takes and whether it may reach the local network, including after redirects and name resolution.
- Risk: high
- Why here: a mistake lets a web page reach the person's network or skips the chosen proxy, so it belongs with the security group. It is the engine's half of the rule chunk 10 supplies.
- Why together: `net_guard` decides with `proxy`'s `Proxies`, since a proxy resolves names the guard cannot see (D34, D62), and `url` takes sign-in details out of a link before it is shown or logged. The two test binaries exercise the proxy and the guard together.
- Size: 2,506 lines, 91 KB, 6 files
- Files:

```text
crates/engine/src/net_guard.rs
crates/engine/src/proxy.rs
crates/engine/src/proxy/tests.rs
crates/engine/src/url.rs
crates/engine/tests/http_proxy_guard.rs
crates/engine/tests/proxy.rs
```

### 10. site-rules

- Job: say which site a name belongs to, which cookies go with a captured request and who may reach the local network, and keep the Public Suffix List current.
- Risk: high
- Why here: it completes chunk 09, and a wrong answer sends cookies to another site. It is small and pure, so review is cheap for the harm.
- Why together: `public_suffix` feeds `cookie_scope` and `local_network` (D39, D112). `manager/suffix_list.rs` is the manager's half of the same list: fetching a newer one, checking it and keeping it (D44, D79.2).
- Size: 2,019 lines, 75 KB, 7 files
- Files:

```text
crates/core/src/manager/suffix_list.rs
crates/sites/Cargo.toml
crates/sites/src/cookie_scope.rs
crates/sites/src/lib.rs
crates/sites/src/local_network.rs
crates/sites/src/public_suffix.rs
crates/sites/src/video_sites.rs
```

### 11. media-addon

- Job: download, verify and unpack the third-party programs Wirecatch then runs, keep them updated, and find them on disk.
- Risk: high
- Why here: these files get executed, so a hole in the checksum, the signature or the unpacking is a P0. It relies on engine and file contracts already reviewed.
- Why together: the crate holds the checks and `manager/addons` the install work that drives them (D20, D21, D38, D79.5). `tests/manager/addons.rs` goes through both with fake releases.
- Size: 5,276 lines, 197 KB, 11 files
- Files:

```text
crates/addons/Cargo.toml
crates/addons/src/archive.rs
crates/addons/src/lib.rs
crates/addons/src/manifest.rs
crates/addons/src/signature.rs
crates/core/examples/ffmpeg_release.rs
crates/core/src/manager/addons/install.rs
crates/core/src/manager/addons/mod.rs
crates/core/src/manager/addons/tests.rs
crates/core/src/manager/addons/updates.rs
crates/core/tests/manager/addons.rs
```

### 12. app-updates

- Job: the trust chain of the app's own updates: write and sign the manifest at release, pick the release to offer, check the installer, and run it.
- Risk: high
- Why here: the same harm as chunk 11, in fewer lines.
- Why together: one flow across five places (D111). `scripts/release/manifest.mjs` writes what `updates::manifest` reads, `manager/app_update.rs` applies it, and `shell/updates.rs` and `desktop/app_update.rs` act on the result.
- Size: 1,432 lines, 48 KB, 11 files
- Files:

```text
crates/core/src/manager/app_update.rs
crates/desktop/src/app_update.rs
crates/shell/src/updates.rs
crates/updates/Cargo.toml
crates/updates/samples/update.json
crates/updates/src/lib.rs
crates/updates/src/manifest.rs
crates/updates/src/release.rs
scripts/release/manifest.mjs
scripts/release/manifest.test.mjs
scripts/release/updater-key.mjs
```

### 13. program-running

- Job: start ffmpeg and yt-dlp, enforce time limits, follow their output within bounds, and kill a program with everything it started.
- Risk: high
- Why here: it opens the media group because chunks 15, 16 and 17 all run through it. Hangs, leaked processes and unbounded output are decided here.
- Why together: `run`, `tail` and `tree` are one small module with one job (D31), and `tests/process.rs` drives it through `fake_program`. Its interface to callers is small and clear, so it is split from the rest of `streams`.
- Size: 2,023 lines, 76 KB, 6 files
- Files:

```text
crates/streams/examples/fake_program.rs
crates/streams/src/process/mod.rs
crates/streams/src/process/run.rs
crates/streams/src/process/tail.rs
crates/streams/src/process/tree.rs
crates/streams/tests/process.rs
```

### 14. stream-manifests

- Job: read hostile HLS and DASH text into a bounded plan of pieces, pick tracks, and give them ids that survive a refreshed playlist.
- Risk: high
- Why here: pure parsing of outside input with memory limits, and chunk 15 consumes its output. A site controls every byte.
- Why together: `hls` and `dash` both produce what `plan` spends its budgets on, `select`, `source` and `track_id` turn that into a choice, and `model`, `error` and `codec` are the types and policy they speak (D31). `tests/manifest_memory.rs` measures this code. The natural seam with chunk 15 is the plan of pieces.
- Size: 5,914 lines, 217 KB, 15 files
- Files:

```text
crates/streams/src/codec.rs
crates/streams/src/dash/list.rs
crates/streams/src/dash/mod.rs
crates/streams/src/dash/sidx.rs
crates/streams/src/dash/template.rs
crates/streams/src/dash/tests.rs
crates/streams/src/error.rs
crates/streams/src/extension.rs
crates/streams/src/hls.rs
crates/streams/src/model.rs
crates/streams/src/plan.rs
crates/streams/src/select.rs
crates/streams/src/source.rs
crates/streams/src/track_id.rs
crates/streams/tests/manifest_memory.rs
```

### 15. stream-download

- Job: fetch a plan's segments into a resumable work folder, decrypt them, append each track, and join the tracks into one file.
- Risk: high
- Why here: follows chunks 13 and 14, which it runs on. Its resume is separate from the engine's, so what chunk 01 proved does not carry over.
- Why together: `download/*` writes and reads the records in `workdir`, `crypto` sits inside the fetch, and `ffmpeg` is the join's last step. `tests/hls.rs`, `tests/dash.rs` and `tests/ffmpeg.rs` run whole downloads from a local server.
- Size: 4,040 lines, 149 KB, 13 files
- Files:

```text
crates/streams/Cargo.toml
crates/streams/examples/fake_ffmpeg.rs
crates/streams/src/crypto.rs
crates/streams/src/download/fetch.rs
crates/streams/src/download/join.rs
crates/streams/src/download/mod.rs
crates/streams/src/ffmpeg.rs
crates/streams/src/lib.rs
crates/streams/src/workdir.rs
crates/streams/tests/common/mod.rs
crates/streams/tests/dash.rs
crates/streams/tests/ffmpeg.rs
crates/streams/tests/hls.rs
```

### 16. media-layer

- Job: turn what yt-dlp and streams offer into the quality choice, and build and run the yt-dlp and ffmpeg commands that make the file.
- Risk: medium
- Why here: chunk 17 calls all of it. Text a site chose reaches arguments and metadata files here, but programs get argument vectors and no shell, so it sits below the chunks that own bytes on disk.
- Why together: one crate with one flow (D24, D31, D79.8): `ytdlp` reads what a page offers, `choice` and `output` decide what to do with it, and `convert`, `merge` and `ffmpeg` carry it out. `describe` words their errors.
- Size: 6,903 lines, 251 KB, 19 files
- Files:

```text
crates/media/Cargo.toml
crates/media/src/choice.rs
crates/media/src/convert.rs
crates/media/src/describe.rs
crates/media/src/ffmpeg.rs
crates/media/src/lib.rs
crates/media/src/merge.rs
crates/media/src/output/arguments.rs
crates/media/src/output/codec.rs
crates/media/src/output/encoders.rs
crates/media/src/output/metadata.rs
crates/media/src/output/mod.rs
crates/media/src/output/subtitles.rs
crates/media/src/output/variants.rs
crates/media/src/tools.rs
crates/media/src/ytdlp/cookies.rs
crates/media/src/ytdlp/download.rs
crates/media/src/ytdlp/info.rs
crates/media/src/ytdlp/mod.rs
```

### 17. core-media-runs

- Job: run a media download from the person's choice through reading the page again, downloading the parts, converting and placing the file, and continuing it after a restart.
- Risk: high
- Why here: last of the media group, since it depends on all of it. It is a long multi-stage run with its own resume, and its placement can overwrite files.
- Why together: `manager/media.rs` makes the question and `media_run/*` carries out the answer, sharing a work folder and a saved job (D24, D51). `added_pages` is the same flow started in the app. `fake_media_tool` and `media_support.rs` exist for these tests.
- Size: 6,473 lines, 241 KB, 14 files
- Files:

```text
crates/core/src/bin/fake_media_tool.rs
crates/core/src/manager/added_pages.rs
crates/core/src/manager/media.rs
crates/core/src/manager/media_run/extras.rs
crates/core/src/manager/media_run/mod.rs
crates/core/src/manager/media_run/network.rs
crates/core/src/manager/media_run/page.rs
crates/core/src/manager/media_run/placement.rs
crates/core/src/manager/media_run/save.rs
crates/core/src/manager/media_run/tests.rs
crates/core/src/manager/media_run/workdir.rs
crates/core/tests/manager/media.rs
crates/core/tests/manager/media_output.rs
crates/core/tests/manager/media_support.rs
```

### 18. queues-and-scheduling

- Job: decide which queue a download joins, when each may start under its queue's limits and schedule, and what a queue reports when it is done.
- Risk: medium
- Why here: a defect stalls or misplaces downloads but loses no data, and much of it is pure functions with direct tests.
- Why together: `crates/queues` holds the rules and `placement`, `scheduling`, `policy` and `manager/queues.rs` apply them to the manager's state (D72, D74). `batches` is what a queue did while running (D52). The four test modules prove limits, schedules and rules from outside.
- Size: 3,682 lines, 125 KB, 16 files
- Files:

```text
crates/core/src/batches.rs
crates/core/src/manager/placement.rs
crates/core/src/manager/policy.rs
crates/core/src/manager/queues.rs
crates/core/src/manager/scheduling.rs
crates/core/tests/manager/limits.rs
crates/core/tests/manager/queues.rs
crates/core/tests/manager/rules.rs
crates/core/tests/manager/schedule.rs
crates/protocol/src/rules.rs
crates/queues/Cargo.toml
crates/queues/src/lib.rs
crates/queues/src/plan.rs
crates/queues/src/rules.rs
crates/queues/src/schedule.rs
crates/queues/src/sites.rs
```

### 19. settings-and-proxies

- Job: settings' defaults, checks and category rules, settings files in and out, and proxy profiles with their passwords and test button.
- Risk: medium
- Why here: outside input arrives through an imported file, and passwords pass through here, but a mistake rarely touches a download's bytes. The core chunks that read settings are done by now.
- Why together: `crates/settings` holds the rules (D71, D79.9), and `preferences`, `manager/proxy.rs` and `core/src/proxy.rs` are the manager's side of the same settings. `protocol/src/proxy.rs` is the profile type they check (D73).
- Size: 5,596 lines, 203 KB, 14 files
- Files:

```text
crates/core/src/manager/preferences.rs
crates/core/src/manager/proxy.rs
crates/core/src/proxy.rs
crates/core/tests/manager/proxy.rs
crates/core/tests/manager/settings.rs
crates/core/tests/manager/settings_file.rs
crates/protocol/src/proxy.rs
crates/settings/Cargo.toml
crates/settings/src/error.rs
crates/settings/src/folders.rs
crates/settings/src/lib.rs
crates/settings/src/proxy.rs
crates/settings/src/proxy/tests.rs
crates/settings/src/settings_file.rs
```

### 20. logging-and-secrets

- Job: send every binary's lines to stderr and a capped file, and keep secrets out of them and out of anything that prints.
- Risk: medium
- Why here: a leak is serious but the code is well bounded and test-driven. Later reviewers rely on its contract when judging what they see logged.
- Why together: `redact` reads what `protocol::secrets` says a secret is, `protocol::secret` hides values at the type (D40, D41.7), and `test-support/src/redaction.rs` with `data/redaction.json` is the one file of samples those rules are tested on.
- Size: 4,875 lines, 157 KB, 14 files
- Files:

```text
crates/logging/Cargo.toml
crates/logging/src/fields.rs
crates/logging/src/file.rs
crates/logging/src/lib.rs
crates/logging/src/redact.rs
crates/logging/src/redact/named.rs
crates/logging/src/redact/scan.rs
crates/logging/src/redact/tests.rs
crates/logging/src/sink.rs
crates/logging/tests/panics.rs
crates/protocol/src/secret.rs
crates/protocol/src/secrets.rs
crates/test-support/data/redaction.json
crates/test-support/src/redaction.rs
```

### 21. engine-ftp

- Job: FTP and FTPS downloads: the control connection, reply parsing, passive data connections, and SOCKS5 for FTP.
- Risk: medium
- Why here: it reuses the path chunk 01 reviewed and is used less than HTTP, though replies are outside input and credentials pass through. Later than the HTTP chunks for that reason.
- Why together: `ftp/*` implements the one `transport` seam, and `socks` is here because only the FTP client needs its own; the HTTP client speaks SOCKS itself. The FTP test binaries cover logging and proxy refusal.
- Size: 3,161 lines, 109 KB, 9 files
- Files:

```text
crates/engine/src/ftp/control.rs
crates/engine/src/ftp/mod.rs
crates/engine/src/ftp/reply.rs
crates/engine/src/ftp/tests.rs
crates/engine/src/socks.rs
crates/engine/src/socks/tests.rs
crates/engine/tests/ftp.rs
crates/engine/tests/ftp_logging.rs
crates/engine/tests/ftp_proxy.rs
```

### 22. app-start-and-quit

- Job: open the manager once, claim the endpoint or hand a second launch to the running copy, register with browsers and the system, and quit so that downloads continue next time.
- Risk: medium
- Why here: shutdown and start-up failures hurt, but the state they protect is reviewed above. The first of the interface-side chunks because the windows all sit on it.
- Why together: everything here runs at the process's start or end (D13, D25, D103, D105). `bridge/src/install` is included because `shell/src/bridge.rs` runs it on every start (D19), and `desktop`'s `main`, `backend`, `lifecycle`, `closing` and `sign_in` are the same sequence from the window side.
- Size: 5,113 lines, 192 KB, 21 files
- Files:

```text
crates/bridge/src/install/browsers.rs
crates/bridge/src/install/host.rs
crates/bridge/src/install/mod.rs
crates/bridge/src/install/plan.rs
crates/desktop/Cargo.toml
crates/desktop/build.rs
crates/desktop/src/backend.rs
crates/desktop/src/closing.rs
crates/desktop/src/lifecycle.rs
crates/desktop/src/main.rs
crates/desktop/src/sign_in.rs
crates/shell/Cargo.toml
crates/shell/src/appimage.rs
crates/shell/src/autostart.rs
crates/shell/src/bridge.rs
crates/shell/src/elevation.rs
crates/shell/src/host.rs
crates/shell/src/launch.rs
crates/shell/src/lib.rs
crates/shell/src/message_box.rs
crates/shell/src/quit.rs
```

### 23. interface-commands-and-ui-kit

- Job: what every window shares: the commands as plain functions, the library of what windows show, text in short words, the theme and the UI pieces.
- Risk: medium
- Why here: ahead of the window chunks, so its contract is ready for them, since `api` and `library` are what those windows call.
- Why together: `shell/src/api.rs` and `desktop/src/library.rs` are the two sides of how a window reaches the manager, and `text`, `theme` and `ui/*` are how it draws the result (D99.6).
- Size: 4,018 lines, 133 KB, 24 files
- Files:

```text
crates/desktop/assets/theme.json
crates/desktop/src/files.rs
crates/desktop/src/library.rs
crates/desktop/src/links.rs
crates/desktop/src/text.rs
crates/desktop/src/theme.rs
crates/desktop/src/ui/callout.rs
crates/desktop/src/ui/choice.rs
crates/desktop/src/ui/dialog.rs
crates/desktop/src/ui/field.rs
crates/desktop/src/ui/file_tile.rs
crates/desktop/src/ui/folder_row.rs
crates/desktop/src/ui/icon.rs
crates/desktop/src/ui/layout.rs
crates/desktop/src/ui/mark.rs
crates/desktop/src/ui/mod.rs
crates/desktop/src/ui/queue_tile.rs
crates/desktop/src/ui/select_circle.rs
crates/desktop/src/ui/small_title_bar.rs
crates/desktop/src/ui/speed_graph.rs
crates/desktop/src/ui/switch_row.rs
crates/desktop/src/ui/tokens.rs
crates/shell/src/api.rs
crates/shell/src/appearance.rs
```

### 24. download-windows

- Job: the small window for each question and each download's progress, including the quality form and the download complete window.
- Risk: medium
- Why here: the most logic of the windows, showing text websites chose and acting on the person's answers (D43), so it goes before the main window.
- Why together: `shell/download_windows.rs` and `shell/finished.rs` decide which windows are open and what they list, and `desktop/download_windows/*` draws those decisions (D23, D52.6, D114).
- Size: 4,719 lines, 168 KB, 8 files
- Files:

```text
crates/desktop/src/download_windows/finished.rs
crates/desktop/src/download_windows/media_form.rs
crates/desktop/src/download_windows/media_output.rs
crates/desktop/src/download_windows/mod.rs
crates/desktop/src/download_windows/run_record.rs
crates/desktop/src/download_windows/view.rs
crates/shell/src/download_windows.rs
crates/shell/src/finished.rs
```

### 25. main-window

- Job: the main window: list, selection, sorting, dragging, menus, details, sidebar, search and keyboard, plus adding links.
- Risk: medium
- Why here: bugs here are visible and fixable without data loss, unless a remove or a move acts on the wrong rows, which is where review should look.
- Why together: all of these act on the same list and selection through `applies` (D47, D91, D115), and `add_dialog` is opened from them.
- Size: 7,135 lines, 249 KB, 21 files
- Files:

```text
crates/desktop/src/add_dialog.rs
crates/desktop/src/main_window/applies.rs
crates/desktop/src/main_window/command_bar.rs
crates/desktop/src/main_window/confirm.rs
crates/desktop/src/main_window/details.rs
crates/desktop/src/main_window/drag.rs
crates/desktop/src/main_window/empty.rs
crates/desktop/src/main_window/fixes.rs
crates/desktop/src/main_window/keymap.rs
crates/desktop/src/main_window/list.rs
crates/desktop/src/main_window/mod.rs
crates/desktop/src/main_window/notices_bar.rs
crates/desktop/src/main_window/order.rs
crates/desktop/src/main_window/palette.rs
crates/desktop/src/main_window/rail.rs
crates/desktop/src/main_window/refresh_dialog.rs
crates/desktop/src/main_window/row_menu.rs
crates/desktop/src/main_window/selection.rs
crates/desktop/src/main_window/sidebar.rs
crates/desktop/src/main_window/sort.rs
crates/desktop/src/main_window/title_bar.rs
```

### 26. notifications-and-tray

- Job: tell the person what started, finished or failed, in the browser or by the system, and the tray icon with its menu.
- Risk: medium
- Why here: a wrong notification misleads but loses nothing. It uses queue batches from chunk 18 and windows from chunk 24.
- Why together: `activity` feeds both the tray and `teller`, which picks how `notifications` get shown (D52, D63, D65), and `desktop/notices.rs` and `desktop/tray.rs` draw them (D104).
- Size: 2,899 lines, 100 KB, 11 files
- Files:

```text
crates/desktop/src/notices.rs
crates/desktop/src/tray.rs
crates/shell/src/activity.rs
crates/shell/src/browser_notices.rs
crates/shell/src/notifications.rs
crates/shell/src/system_notifications.rs
crates/shell/src/teller.rs
crates/shell/src/tray.rs
crates/shell/src/tray_proxy.rs
crates/shell/src/tray_queues.rs
crates/shell/src/tray_speed.rs
```

### 27. settings-and-queue-forms

- Job: every form that changes settings, proxies or a queue, and settings import and export dialogs.
- Risk: medium
- Why here: "settings nothing reads" and controls that only partly work are found here, but the rules they feed were reviewed in chunks 18 and 19.
- Why together: all are forms that read from the library and send a change through the manager, and they share dialogs (`queue_proxy` opens `proxy_dialog`, `queue_rules` opens `link_trial`) (D72, D73, D85).
- Size: 6,896 lines, 237 KB, 18 files
- Files:

```text
crates/desktop/src/main_window/backup_dialog.rs
crates/desktop/src/main_window/link_trial.rs
crates/desktop/src/main_window/proxy_dialog.rs
crates/desktop/src/main_window/queue_delete.rs
crates/desktop/src/main_window/queue_dialog.rs
crates/desktop/src/main_window/queue_header.rs
crates/desktop/src/main_window/queue_proxy.rs
crates/desktop/src/main_window/queue_rules.rs
crates/desktop/src/main_window/settings/about.rs
crates/desktop/src/main_window/settings/addon.rs
crates/desktop/src/main_window/settings/browser.rs
crates/desktop/src/main_window/settings/files.rs
crates/desktop/src/main_window/settings/general.rs
crates/desktop/src/main_window/settings/media.rs
crates/desktop/src/main_window/settings/mod.rs
crates/desktop/src/main_window/settings/network.rs
crates/desktop/src/main_window/settings/notifications.rs
crates/desktop/src/main_window/settings/speed.rs
```

### 28. failure-messages

- Job: turn engine and manager errors into words for a person, each with the fix to offer.
- Risk: low
- Why here: a wrong message or fix misleads but does no damage, and the reviewer now knows the errors the earlier chunks really produce.
- Why together: `messages` and `core/src/error.rs` are the two steps from an error to what the protocol carries (D3, D79.7). `tests/manager/ftp.rs` tests that wording.
- Size: 1,199 lines, 46 KB, 4 files
- Files:

```text
crates/core/src/error.rs
crates/core/tests/manager/ftp.rs
crates/messages/Cargo.toml
crates/messages/src/lib.rs
```

### 29. packaging-and-release

- Job: build the installers, the third-party notices, the package hooks and the release workflow.
- Risk: medium
- Why here: a bad installer reaches everyone, but real installers are tested by hand after the fixes, so review here is limited to what reading can show. Not product logic, so late.
- Why together: `release.yml` runs `package.mjs`, which reads the `.iss` file, the hook templates and `notices/*` (D106, D117), and the `ci/linux-package*` scripts check the result.
- Size: 1,668 lines, 69 KB, 16 files
- Files:

```text
.github/workflows/release.yml
config/templates/SECURITY.md
config/templates/prerm.sh
crates/desktop/PACKAGING.md
crates/desktop/assets/icons/icon.svg
crates/desktop/packaging/windows/installer.iss
scripts/ci/linux-package-check.sh
scripts/ci/linux-packages.sh
scripts/ci/linux-packages.txt
scripts/gate/notices.mjs
scripts/notices/cargo.mjs
scripts/notices/licences.mjs
scripts/notices/notices.test.mjs
scripts/notices/npm.mjs
scripts/notices/render.mjs
scripts/package.mjs
```

### 30. protocol-types

- Job: the types the interface sees, the constants exported to TypeScript, the language list, and where a video's quality window starts.
- Risk: low
- Why here: mostly type definitions, so a poor starting point even though everything depends on them. Earlier chunks have read them as context, and `media_start` is the one part with real logic.
- Why together: what is left of the shared contract after its parts with logic joined their users.
- Size: 4,555 lines, 154 KB, 5 files
- Files:

```text
crates/protocol/src/app.rs
crates/protocol/src/desktop.rs
crates/protocol/src/exported.rs
crates/protocol/src/languages.rs
crates/protocol/src/media_start.rs
```

### 31. test-servers

- Job: the local HTTP, FTP, HTTP/3 and proxy servers and helpers that every crate's tests run on.
- Risk: low
- Why here: no user is reached directly. A server that does not do what a test assumes makes that test prove nothing, and reviewing last means the reviewer can check it against what the tests were seen to assume.
- Why together: one crate with one job (D26). Its redaction samples moved to chunk 20.
- Size: 3,315 lines, 117 KB, 14 files
- Files:

```text
crates/test-support/Cargo.toml
crates/test-support/src/bridge.rs
crates/test-support/src/downloads.rs
crates/test-support/src/files.rs
crates/test-support/src/ftp.rs
crates/test-support/src/http.rs
crates/test-support/src/http3.rs
crates/test-support/src/lib.rs
crates/test-support/src/logs.rs
crates/test-support/src/proxy.rs
crates/test-support/src/resume.rs
crates/test-support/src/silent.rs
crates/test-support/src/tls.rs
crates/test-support/tests/logs.rs
```

### 32. cli

- Job: the `wirecatch` command-line downloader: arguments, progress and exit status.
- Risk: low
- Why here: a thin front of the engine, and the fault tests run through it, so anything serious shows there first.
- Why together: one binary. It stays apart so unrelated code is not merged just to be small.
- Size: 505 lines, 18 KB, 2 files
- Files:

```text
crates/cli/Cargo.toml
crates/cli/src/main.rs
```

### 33. repo-checks

- Job: the gate and its checks, the product identity generator, CI workflows and the repository's build configuration.
- Risk: low
- Why here: no user is reached, and a defect shows as a check that passes too easily. Last because it is the least likely to hold a product bug.
- Why together: the gate runs these checks and CI runs the gate (D18, D78), the `product` check compares what the generator writes (D29), and the root manifests configure what they check.
- Size: 4,556 lines, 191 KB, 43 files
- Files:

```text
.cargo/config.toml
.githooks/commit-msg
.github/workflows/audit.yml
.github/workflows/ci.yml
.github/workflows/outdated.yml
Cargo.toml
clippy.toml
config/product.toml
crates/product/Cargo.toml
deny.toml
rust-toolchain.toml
scripts/gate.mjs
scripts/gate/browser.mjs
scripts/gate/comments.mjs
scripts/gate/commits.mjs
scripts/gate/exceptions.mjs
scripts/gate/imports.test.mjs
scripts/gate/javascript.mjs
scripts/gate/javascript.test.mjs
scripts/gate/js-policy.mjs
scripts/gate/linux.mjs
scripts/gate/process.mjs
scripts/gate/product.mjs
scripts/gate/product.test.mjs
scripts/gate/rust-attributes.mjs
scripts/gate/rust.mjs
scripts/gate/rust.test.mjs
scripts/gate/suffix-list.mjs
scripts/gate/suffix-list.test.mjs
scripts/gate/test-timing.mjs
scripts/gate/test-timing.test.mjs
scripts/gate/text.mjs
scripts/gate/text.test.mjs
scripts/install-hooks.mjs
scripts/linux-test.sh
scripts/outdated.mjs
scripts/product/generate.mjs
scripts/product/manifests.mjs
scripts/product/manifests.test.mjs
scripts/product/outputs.mjs
scripts/product/read.mjs
scripts/product/read.test.mjs
scripts/product/sources.mjs
```

## Left out

Tracked files in scope that belong to no chunk, under the plan's exclusions:

- Generated: `crates/product/src/lib.rs`, `crates/engine/proptest-regressions/naming.txt`, and `crates/protocol/released/v1/*.ts` (saved copies of generated types).
- Data snapshots: the Public Suffix List, and the yt-dlp checksum list, its signature and the pinned signing key, all copied from elsewhere.
- Binary files: the icons and installer bitmaps.
- Standard licence texts: `config/licenses/*` and `crates/engine/LICENSE`.

```text
config/licenses/Apache-2.0.txt
config/licenses/BSD-3-Clause.txt
config/licenses/ISC.txt
config/licenses/MIT.txt
config/licenses/MPL-2.0.txt
config/licenses/appimage-runtime.txt
config/licenses/lucide.txt
crates/addons/src/testdata/ytdlp-2026.08.19-SHA2-256SUMS
crates/addons/src/testdata/ytdlp-2026.08.19-SHA2-256SUMS.sig
crates/addons/src/ytdlp-signing-key.asc
crates/desktop/assets/icons/128x128.png
crates/desktop/assets/icons/128x128@2x.png
crates/desktop/assets/icons/32x32.png
crates/desktop/assets/icons/64x64.png
crates/desktop/assets/icons/icon.ico
crates/desktop/assets/icons/icon.png
crates/desktop/packaging/windows/wizard-sidebar-200.bmp
crates/desktop/packaging/windows/wizard-sidebar.bmp
crates/desktop/packaging/windows/wizard-small-100.bmp
crates/desktop/packaging/windows/wizard-small-125.bmp
crates/desktop/packaging/windows/wizard-small-150.bmp
crates/desktop/packaging/windows/wizard-small-200.bmp
crates/desktop/packaging/windows/wizard-small-250.bmp
crates/engine/LICENSE
crates/engine/proptest-regressions/naming.txt
crates/product/src/lib.rs
crates/protocol/released/v1/Call.ts
crates/protocol/released/v1/ErrorCode.ts
crates/protocol/released/v1/Event.ts
crates/sites/data/public_suffix_list.dat
```

Kept in chunks though they are data, because a test reads them and they are written here, not copied: `crates/store/samples/*` (chunk 03), `crates/updates/samples/update.json` (chunk 12) and `crates/test-support/data/redaction.json` (chunk 20).

## Mutation targets

For a 2-hour `cargo-mutants` run: 15 files in 4 crates, chosen because a wrong answer here is silent and does the most harm, and their logic is decisions rather than system calls. Run the three small crates first, since the engine's integration tests could otherwise use the whole cap. `core`, `store` and `streams` are left out because each mutant there would rerun a large integration suite.

| Crate               | File                   | Why                                                                             |
| ------------------- | ---------------------- | ------------------------------------------------------------------------------- |
| `wirecatch-addons`  | `src/archive.rs`       | `safe_join` and `unpack`: a weak check writes outside the add-on's folder       |
| `wirecatch-addons`  | `src/signature.rs`     | `verify_detached`: a weak check installs a yt-dlp nobody signed                 |
| `wirecatch-addons`  | `src/manifest.rs`      | Parsing the pinned digests that decide what is allowed to run                   |
| `wirecatch-updates` | `src/manifest.rs`      | `verified`: nothing about a release is trusted before this passes               |
| `wirecatch-updates` | `src/release.rs`       | `newest`: version order, drafts and pre-releases decide what gets offered       |
| `wirecatch-sites`   | `src/local_network.rs` | Whether a link may reach the person's network                                   |
| `wirecatch-sites`   | `src/cookie_scope.rs`  | Which cookies go with a request                                                 |
| `wirecatch-sites`   | `src/public_suffix.rs` | The registrable domain both rules above are built on                            |
| `wirecatch-engine`  | `src/segment.rs`       | Range arithmetic and splitting: an off-by-one corrupts or skips bytes           |
| `wirecatch-engine`  | `src/state.rs`         | `can_resume`: whether kept bytes are trusted                                    |
| `wirecatch-engine`  | `src/target.rs`        | Whether an earlier run continues, and which file it writes                      |
| `wirecatch-engine`  | `src/http/range.rs`    | `check_partial` and `parse_content_range`: accepting bytes for the wrong offset |
| `wirecatch-engine`  | `src/naming.rs`        | `sanitize` and `is_safe_name`: path safety for names a server chose             |
| `wirecatch-engine`  | `src/net_guard.rs`     | Address classification behind the local network guard                           |
| `wirecatch-engine`  | `src/retry.rs`         | Which failures retry and when a download gives up                               |

## Progress

Stage 2 batches are added once `GROUPS.md` exists, and UI windows once `ui.md` lists them. A flow too large for one reader is split when it starts.

| Stage | Unit                                    | Status      |
| ----- | --------------------------------------- | ----------- |
| 0     | Feature map (`FEATURES.md`)             | done        |
| 0     | Tool: `cargo llvm-cov`                  | done        |
| 0     | Tool: `jscpd`                           | done        |
| 0     | Tool: `cargo-mutants`                   | done        |
| 0     | Fault tests (`fault-tests.md`)          | done        |
| 1     | Chunk 01: engine-disk-resume (pilot)    | done        |
| 1     | Chunk 02: engine-http                   | done        |
| 1     | Chunk 03: saved-state                   | done        |
| 1     | Chunk 04: core-run-lifecycle            | done        |
| 1     | Chunk 05: core-final-path               | done        |
| 1     | Chunk 06: bridge-and-frames             | done        |
| 1     | Chunk 07: browser-handover              | done        |
| 1     | Chunk 08: local-endpoint                | done        |
| 1     | Chunk 09: request-routing               | done        |
| 1     | Chunk 10: site-rules                    | done        |
| 1     | Chunk 11: media-addon                   | done        |
| 1     | Chunk 12: app-updates                   | done        |
| 1     | Chunk 13: program-running               | done        |
| 1     | Chunk 14: stream-manifests              | done        |
| 1     | Chunk 15: stream-download               | done        |
| 1     | Chunk 16: media-layer                   | done        |
| 1     | Chunk 17: core-media-runs               | done        |
| 1     | Chunk 18: queues-and-scheduling         | done        |
| 1     | Chunk 19: settings-and-proxies          | done        |
| 1     | Chunk 20: logging-and-secrets           | done        |
| 1     | Chunk 21: engine-ftp                    | done        |
| 1     | Chunk 22: app-start-and-quit            | done        |
| 1     | Chunk 23: interface-commands-and-ui-kit | done        |
| 1     | Chunk 24: download-windows              | done        |
| 1     | Chunk 25: main-window                   | done        |
| 1     | Chunk 26: notifications-and-tray        | done        |
| 1     | Chunk 27: settings-and-queue-forms      | done        |
| 1     | Chunk 28: failure-messages              | done        |
| 1     | Chunk 29: packaging-and-release         | done        |
| 1     | Chunk 30: protocol-types                | done        |
| 1     | Chunk 31: test-servers                  | done        |
| 1     | Chunk 32: cli                           | done        |
| 1     | Chunk 33: repo-checks                   | done        |
| 1     | Flow `path-normal-download`             | done        |
| 1     | Flow `path-media-download`              | done        |
| 1     | Flow `path-app-update`                  | done        |
| 1     | Flow `feature-behaviour`                | done        |
| 1     | Flow `crash-and-resume`                 | done        |
| 1     | Flow `shutdown`                         | done        |
| 1     | Flow `concurrency`                      | done        |
| 1     | Flow `database-migrations`              | done        |
| 1     | Flow `security-bridge-and-pipe-input`   | done        |
| 1     | Flow `security-path-handling`           | done        |
| 1     | Flow `security-update-signatures`       | done        |
| 1     | Flow `security-addon-unpacking`         | done        |
| 1     | Flow `repeated-logic`                   | done        |
| 1     | Flow `second-review-resume`             | done        |
| 1     | Flow `second-review-disk-writes`        | done        |
| 1     | UI review (`ui.md`)                     | done        |
| 2     | Grouping (`GROUPS.md`)                  | done        |
| 2     | Batch B01                               | pending     |
| 2     | Batch B02                               | done        |
| 2     | Batch B03                               | pending     |
| 2     | Batch B04                               | done        |
| 2     | Batch B05                               | pending     |
| 2     | Batch B06                               | pending     |
| 2     | Batch B07                               | pending     |
| 2     | Batch B08                               | pending     |
| 2     | Batch B09                               | pending     |
| 2     | Batch B10                               | pending     |
| 2     | Batch B11                               | pending     |
| 2     | Batch B12                               | pending     |
| 2     | Batch B13                               | pending     |
| 2     | Batch B14                               | pending     |
| 2     | Batch B15                               | pending     |
| 2     | Batch B16                               | pending     |
| 2     | Batch B17                               | pending     |
| 2     | Batch B18                               | pending     |
| 2     | Batch B19                               | pending     |
| 2     | Mutation survivors                      | pending     |
| 2     | New findings batch                      | pending     |
| 3     | Stage 3 (`AUDIT-LOG.md`)                | pending     |

## Approvals

| What       | Status                           |
| ---------- | -------------------------------- |
| Chunk plan | approved                         |
| Pilot      | waived: run all chunks           |
| Fix order  | approved in advance: do not wait |

## Bar corrections

- All stages are approved: run through to the finished audit log without stopping for approval.
- Stage 1 runs at most 12 chunk reviewers at once, using Opus 5.5 at medium effort.

# crates/engine audit

## ENG-1 A failed disk write lets the next write mark the gap as flushed (silent file corruption)
- severity: high
- category: error-handling
- location: crates/engine/src/worker.rs:445-473, crates/engine/src/worker.rs:280-295, crates/engine/src/segment.rs:150-154
- problem: `Writer::flush` advances `self.offset` to the end of a buffer as soon as it hands the buffer to `spawn_blocking`. If that write later fails, `wait()` returns the error and the next buffer (already filled by `push`) stays in `self.buf`. `stream()` then calls `writer.finish()` on its way out (worker.rs:295), which spawns a write of that next buffer at the advanced offset; if it succeeds, `mark_flushed(index, end)` moves `flushed` past the range whose write failed, because `mark_flushed` takes `max` and does not require contiguity. The resume file and `is_complete()` then treat a hole of zeros (or stale bytes) as downloaded. Transient write failures are real on Windows (sharing or lock violations from antivirus and indexers, `ERROR_LOCK_VIOLATION`) and on removable or network drives.
- fix: Make the writer fail closed: when a write job fails, drop the buffered bytes (`buf.clear()`, reset `offset`) and set a poisoned flag so `finish()` writes nothing more for that segment. In `SegmentTable::mark_flushed`, take the write's start offset and only advance when `start <= flushed` (debug-assert contiguity), so a non-contiguous flush can never be recorded.
- cross-package: no

## ENG-2 DownloadOptions is never validated: zero values hang, panic or silently disable checkpoints
- severity: high
- category: design
- location: crates/engine/src/download.rs:40-142, crates/engine/src/download.rs:521-528, crates/engine/src/download.rs:550-552, crates/engine/src/worker.rs:432-442
- problem: Every field is `pub` and nothing checks it. `buffer_size = 0` makes `Writer::push` loop forever without yielding (`n` is always 0, `flush` returns immediately), pinning an executor thread and ignoring cancellation. `min_split = 0` panics in `remaining.div_ceil(shared.opts.min_split)` (download.rs:524; `SegmentTable` clamps with `.max(1)` but this call does not). `checkpoint_interval = Duration::ZERO` panics inside `tokio::time::interval` in the spawned checkpoint task; the panic is swallowed because the handle is never awaited, so the download runs with no checkpoints and a crash loses everything. `stall_timeout = 0` turns every request into an immediate stall. These come from user settings in the app, so a bad settings file or a UI slider at 0 reaches them.
- fix: Add `DownloadOptions::validate(&self) -> Result<(), Error>` (or normalize in one place) called at the top of `download_guarded` and `Fetcher::with_private_network`: reject or clamp `buffer_size`, `min_split`, `checkpoint_interval`, `stall_timeout` and `connect_timeout` to sane minimums, with an error variant that names the field. Add unit tests for each zero value.
- cross-package: yes (core, which builds options from settings, should rely on this instead of its own clamping)

## ENG-3 Pausing waits for the rate limiter's sleep to finish
- severity: medium
- category: concurrency
- location: crates/engine/src/worker.rs:286-288, crates/engine/src/fetch.rs:458-460, crates/engine/src/limiter.rs:58-76
- problem: `RateLimiter::consume` sleeps off the whole debt for a chunk, and both callers await it outside any `select!` on the cancellation token. With an FTP chunk of up to 128 KiB (`READ_BUFFER`) and a 32 KiB/s limit, a pause blocks for about 4 s per limiter; with a limit of 1 B/s (what `normalize` turns 0 into) it is effectively forever. The crate docs promise that "cancelling never waits". Raising or removing the limit also does not wake sleepers, so `set_limit(None)` takes effect only after the old sleep ends, contradicting "takes effect within one chunk".
- fix: Make `consume` take the `CancellationToken` (or return the wait and let the caller `select!` on `cancel.cancelled()`), and have `set_limit` notify a `tokio::sync::Notify` so sleepers recompute their wait. Add an integration test that pauses a rate-limited download and asserts it returns `Paused` promptly.
- cross-package: no

## ENG-4 Blocking file system calls run on async worker threads
- severity: medium
- category: performance
- location: crates/engine/src/download.rs:423, crates/engine/src/download.rs:447, crates/engine/src/download.rs:492-501, crates/engine/src/download.rs:589-591, crates/engine/src/download.rs:600-601, crates/engine/src/download.rs:671-691, crates/engine/src/download.rs:702-741, crates/engine/src/naming.rs:190-205, crates/engine/src/storage.rs:54-68
- problem: The project rule is "no blocking calls on async tasks", and the checkpoint loop correctly uses `spawn_blocking`, but the rest of `download_guarded` does not: `create_dir_all`, `ResumeState::load` (`std::fs::read`), `metadata`, `remove_file`, `unique_path` (an unbounded series of `exists()` calls), `create_part` (`fallocate` or sparse `set_len`), the initial and final `save_state()` (write plus rename) and `finalize` (set_len, DeviceIoControl, rename) all run inline. On Windows with antivirus or a network drive each can take tens to hundreds of milliseconds, stalling every other download on that runtime thread. Worse, `*closed.lock().unwrap() = true` (download.rs:589, 601) blocks the async thread on a `std::sync::Mutex` that a checkpoint holds for the duration of its file I/O on the blocking pool.
- fix: Move target planning (`plan_exact`/`plan_target` plus `create_part`) into one `spawn_blocking` call, and run the final save and `finalize` through `spawn_blocking` as well. Replace the `closed: Mutex<bool>` + `abandoned: AtomicBool` pair with a small `StateWriter` that owns the path and serializes saves with `tokio::sync::Mutex` (awaited, not blocked on).
- cross-package: no

## ENG-5 The resume file is trusted: bad ranges underflow, write past the end, or fail every rerun
- severity: medium
- category: error-handling
- location: crates/engine/src/state.rs:13-25, crates/engine/src/state.rs:46-56, crates/engine/src/segment.rs:67-83, crates/engine/src/download.rs:439-444
- problem: `ResumeState::load` checks only the version, and `SegmentTable::from_remaining` only drops empty ranges. A `.wcstate` with overlapping ranges, ranges ending past `total`, or unsorted duplicates (a hand edit, a partial write on a file system without rename atomicity, or a future bug) is accepted because `can_resume` only compares size and validators. Then `total.unwrap_or(0) - table.remaining_bytes()` (download.rs:442) underflows: a panic in debug builds, a nonsense `resumed_from` and progress in release. Ranges past `total` make workers request bytes beyond the end, get `416` (not retryable), and fail; every rerun loads the same file and fails again, so the user is stuck until they delete files by hand. This is data from outside the process being used without validation.
- fix: Validate in `ResumeState::load` (or a `ResumeState::ranges_within(total)` called by `plan_*`): ranges sorted, non-overlapping, `start < end <= total`, `remaining_bytes <= total`. Treat a failure like an unresumable state (delete and start over, with a `warn!`). Use `checked_sub` at download.rs:442. Add a test with a tampered state file.
- cross-package: no

## ENG-6 A file that changes on the server at the same size is not detected mid-download
- severity: medium
- category: security
- location: crates/engine/src/http.rs:579-622, crates/engine/src/http3.rs:333-350, crates/engine/src/fetch.rs:479-510, crates/engine/src/ftp/mod.rs:388-398
- problem: Validators (`ETag`, `Last-Modified`) are compared only at the first request of a run. Every later range request (HTTP/1.1, HTTP/2, HTTP/3, the `Fetcher`) checks only that `Content-Range` starts at the right offset and that the total is unchanged. If the file is replaced by another of the same size while connections are running (rolling CDN deploys, nightly builds, re-encoded media with fixed bitrate), bytes of two versions are stitched together and reported as `Completed`. FTP re-checks `SIZE` on new control connections but never `MDTM`.
- fix: Send `If-Range: <etag or last-modified>` on every range request when a strong validator is known (a changed file then answers `200`, which already maps to `RangeNotHonored`), and compare `ETag`/`Last-Modified` of each `206` against the probe's `Validators`, returning `Error::RemoteChanged` on mismatch. For FTP, re-check `MDTM` in `Server::connect` alongside `SIZE`. Put the comparison in one helper shared by all three HTTP paths (see ENG-8).
- cross-package: no

## ENG-7 "Crash-safe resume" only holds for process crashes: nothing is fsynced
- severity: medium
- category: error-handling
- location: crates/engine/src/state.rs:59-66, crates/engine/src/worker.rs:445-473, crates/engine/src/storage.rs:54-68, crates/engine/src/lib.rs:5-7
- problem: The checkpoint records ranges as done once `write_at` returns, i.e. once the bytes are in the page cache, and the state file itself is written and renamed without `sync_data`. After a power loss or OS crash the state file can claim ranges whose data never reached the disk (they read back as zeros in a sparse file), and on some file systems the renamed state file can be empty. The download then completes with corrupt content and no error. The crate docs and README sell resume "even after a crash".
- fix: At each checkpoint, `sync_data()` the part file before writing the state file, and `sync_data()` the temporary state file before the rename (the checkpoint interval already bounds the cost to once per second; make it configurable if needed). Fsync the final file before returning `Completed`. If that cost is unacceptable, narrow the docs to "after the process is killed".
- cross-package: no

## ENG-8 HTTP range handling and retry loops are implemented three times
- severity: medium
- category: duplication
- location: crates/engine/src/http.rs:594-621, crates/engine/src/http3.rs:281-287, crates/engine/src/http3.rs:313-350, crates/engine/src/fetch.rs:486-510, crates/engine/src/http.rs:90-107, crates/engine/src/transport.rs:317-341, crates/engine/src/worker.rs:113-167, crates/engine/src/worker.rs:190-230, crates/engine/src/fetch.rs:356-394, crates/engine/src/worker.rs:252-293, crates/engine/src/fetch.rs:425-461
- problem: The `Range` header is formatted in three places (with slightly different end handling: `to - 1` vs `end.saturating_sub(1)`), the `206` / `Content-Range` start / total checks in three places, and the "429 or 503 with Retry-After" parsing in two (http3 rebuilds what `Failure::status` does). There are three retry loops with different semantics: `with_retries` counts every failure and never resets, `Fetcher::transfer` resets on progress, `Attempts` resets on progress and handles refusals. Two body-streaming loops repeat the `select!` on cancel plus `timeout(stall, next)` plus skip logic. Each fix (for example ENG-6 or ENG-3) has to be made three times, and the copies have already drifted.
- fix: Introduce a `ByteRange`-based request type with one `fn range_header(from, to) -> HeaderValue` and one `fn check_partial(headers, from, expected_total, validators) -> Result<(), Error>` used by `http::fetch`, `http3::request` and `Fetcher::open`; have http3 build its busy failure via a shared `Failure::from_status(status, headers, url)`. Make `Attempts` the single retry policy (with a flag for refusal handling) and use it from `with_retries` and `Fetcher::transfer`. Extract the cancel/stall/skip read loop into one helper that yields slices.
- cross-package: no

## ENG-9 The private-network classifier is duplicated in core, in a weaker form
- severity: medium
- category: security
- location: crates/engine/src/fetch.rs:594-646, crates/core/src/manager/media_run.rs:1205-1229
- problem: The engine's `is_private` covers CGNAT (100.64/10), benchmarking, multicast, reserved, NAT64, 6to4 and IPv4-compatible addresses. Core has its own `is_private`/`all_private`/`addresses` that covers none of those, and uses it to decide `PrivateNetwork::Allow` vs `Block` and to refuse formats. The two disagree on, for example, a Tailscale `100.x` host or a NAT64 / 6to4 address wrapping `10.0.0.1`, so the same URL is "local" to one layer and "public" to the other. Security rules that drift between layers are how bypasses appear.
- fix: Expose the engine's classifier publicly (for example `wirecatch_engine::net::is_local_address(IpAddr)` and an async `host_is_local(&Url, timeout)`), delete core's copy, and keep the tests in one place.
- correction (W4, D34.5): the classifier is public as `wirecatch_engine::AddressKind`; no `host_is_local` was added, because a separate lookup of a host name decides nothing about the connection that follows (CORE-3).
- cross-package: yes (core)

## ENG-10 Disk errors carry no path and every io::Error is labelled "disk error"
- severity: medium
- category: error-handling
- location: crates/engine/src/error.rs:66-69, crates/engine/src/download.rs:423, crates/engine/src/download.rs:447, crates/engine/src/download.rs:590, crates/engine/src/storage.rs:12-23, crates/engine/src/state.rs:46-66, crates/engine/src/worker.rs:54
- problem: `Error::Io(#[from] io::Error)` is filled by bare `?` on `create_dir_all`, `create_part`, `open_for_write`, `ResumeState::load/save` and `finalize`. The user sees "disk error: Access is denied. (os error 5)" with no file, no operation and no hint. The project rule is that errors say what happened and how to fix it. `#[from]` also lets any future `?` on a socket or Tokio I/O error be mislabelled as a disk error.
- fix: Replace `#[from] io::Error` with a structured variant, for example `Io { action: IoAction /* CreateFolder, CreatePart, WriteResume, Rename... */, path: PathBuf, source: io::Error }`, built through a small `.context(action, path)` extension, with a Display that names the file and suggests the fix (free space, permissions, file in use). Drop the `From` impl so every site has to choose.
- cross-package: yes (core and desktop format these errors for users)

## ENG-11 Names are not reserved, so a file can be overwritten despite "never overwritten"
- severity: medium
- category: concurrency
- location: crates/engine/src/naming.rs:190-205, crates/engine/src/storage.rs:12-23, crates/engine/src/storage.rs:67, crates/engine/src/download.rs:702-741
- problem: `unique_path` picks the first name whose final, part and state files are absent, but nothing claims it: `create_part` opens with `create(true).truncate(true)`, so two downloads that resolve the same name concurrently on the multi-threaded runtime can both open and truncate the same `.wcpart`. At the end, `finalize` uses `std::fs::rename`, which replaces an existing destination on both Linux and Windows, so a file the user created or downloaded under that name while this download ran (hours, possibly) is silently overwritten. The crate docs state "An existing file is never overwritten".
- fix: Create the part file with `create_new(true)` and on `AlreadyExists` move to the next candidate in `unique_path` (the part file is then the reservation). In `finalize`, for the non-exact-path case, refuse to replace an existing destination (on Linux `renameat2(RENAME_NOREPLACE)` or link-then-unlink; on Windows `MoveFileExW` without `MOVEFILE_REPLACE_EXISTING`) and fall back to the next free name.
- cross-package: no

## ENG-12 Module layering: the low-level HTTP layer depends on fetch, and protocols depend on the orchestrator's god struct
- severity: medium
- category: design
- location: crates/engine/src/http.rs:273, crates/engine/src/http.rs:316, crates/engine/src/fetch.rs:590-875, crates/engine/src/download.rs:298-325, crates/engine/src/transport.rs:12, crates/engine/src/ftp/mod.rs:37, crates/engine/src/ftp/mod.rs:590, crates/engine/src/http3.rs:25, crates/engine/src/http3.rs:254-292
- problem: `fetch.rs` (1192 lines) holds the public `Fetcher`, the IP classifier, the DNS-rebinding resolver, the per-hop guard and proxy environment parsing; `http.rs` (the transport layer) imports `crate::fetch::FilteringResolver`, and `download.rs` imports `private_guard` from `fetch`, so the security policy lives inside one of its consumers. Separately, `download::Shared` (all-`pub` fields: url, remote, table, opts, cancel, progress, counters) is passed into `ftp::Connection::open`, `http3::request` and `transport::Connection::open`, which only need a cancel token, the URL, `ranged`, `total` and a user agent. Protocol code can therefore reach the segment table and progress, and cannot be tested without building a whole download.
- fix: Move `is_private`, `embedded_ipv4`, `NetworkGuard`, `FilteringResolver`, `PrivateGuard`, `proxy_hosts` and `private_guard` into a `net_guard` (or `netpolicy`) module that both `http` and `fetch` depend on. Pass protocol layers a narrow `RequestContext { url, ranged, total, cancel, user_agent }` instead of `&Shared`, and keep `Shared` private to `download` and `worker`.
- cross-package: no

## ENG-13 Test gaps on the riskiest paths
- severity: medium
- category: testing
- location: crates/engine/src/worker.rs:402-480, crates/engine/src/limiter.rs:58-76, crates/engine/src/state.rs:46-56, crates/engine/src/download.rs:144-163, crates/engine/tests/download.rs:17-60, crates/engine/tests/hardening.rs:20-43, crates/engine/tests/request_limit.rs:50-60
- problem: Nothing exercises a failed positional write (ENG-1), a pause while a rate limiter is sleeping (ENG-3), a corrupted or tampered `.wcstate` (ENG-5), zero or extreme option values (ENG-2), or a same-size file change between range requests (ENG-6); these are exactly the paths where silent corruption or hangs happen. The integration test files each redefine the same `temp_dir`, `request` and `run` helpers and the same option tweaks instead of putting them in `tests/common`.
- fix: Add a test-only fault-injecting writer (behind `#[cfg(test)]` or a trait for the positional write) and the five tests above; move the shared helpers into `tests/common/mod.rs`.
- cross-package: no

## ENG-14 Magic values: protocol codes, backoff and cache sizes are inline literals
- severity: low
- category: magic-value
- location: crates/engine/src/ftp/mod.rs:244, crates/engine/src/ftp/mod.rs:247, crates/engine/src/ftp/mod.rs:253, crates/engine/src/ftp/mod.rs:276-298, crates/engine/src/ftp/mod.rs:404, crates/engine/src/ftp/mod.rs:418, crates/engine/src/ftp/mod.rs:446, crates/engine/src/ftp/mod.rs:530, crates/engine/src/worker.rs:326, crates/engine/src/worker.rs:334, crates/engine/src/worker.rs:381-391, crates/engine/src/error.rs:84, crates/engine/src/error.rs:209, crates/engine/src/http3.rs:314, crates/engine/src/http3.rs:328, crates/engine/src/limiter.rs:66, crates/engine/src/ftp/control.rs:119, crates/engine/src/http3.rs:99, crates/engine/src/fetch.rs:664, crates/engine/src/fetch.rs:811
- problem: About 27 FTP reply codes (120, 202, 213, 220, 227, 229, 230, 234, 331, 350, 421, 425, 530 ...) are bare integers across `ftp/mod.rs` and `worker.rs`; HTTP statuses are `u16` literals in `error.rs`, `worker.rs` and `http3.rs` while `http.rs` uses `StatusCode` constants. Backoff uses literal 500 ms base, `min(5)` exponent cap and 250 ms jitter; the limiter's burst uses `4.0` and `16.0 * 1024.0`; TLS caches use 32 and 64; port fallback `80` appears twice. `MAX_REDIRECTS = 20` is also hard-coded as "20" in the `TooManyRedirects` message, so changing the constant leaves a wrong message. The "busy" status set lives in `is_retryable`, `is_refusal`, `Failure::status`, http3 and the crate docs separately.
- fix: Add an `ftp::reply` module of named codes (`SERVICE_READY`, `LOGGED_IN`, `NEED_PASSWORD`, `FILE_ACTION_PENDING`, `TOO_MANY_CONNECTIONS`...), use `StatusCode` everywhere for HTTP with one `fn is_busy(StatusCode)`, name the backoff and burst constants next to `MAX_BACKOFF`, and format the redirect message from `MAX_REDIRECTS`.
- cross-package: no

## ENG-15 Proxy environment variables are listed and read in three places
- severity: low
- category: duplication
- location: crates/engine/src/fetch.rs:738-773, crates/engine/src/transport.rs:298-310, crates/engine/src/http3.rs:177
- problem: `PROXY_VARIABLES`, `FTP_PROXY_VARIABLES` and `HTTPS_PROXY_VARIABLES` are three overlapping hand-written lists, each read with its own `std::env::var_os` closure, and none consider `NO_PROXY`, which reqwest honours. If one list is updated (for example to add `NO_PROXY` handling, or to match a reqwest change) the others silently disagree about whether a proxy is in effect, which matters because two of them gate security behaviour (FTP refusal, HTTP/3 bypass).
- fix: One `proxy` module with an enum of schemes and `fn configured_proxy(scheme, host, env) -> Option<ProxySetting>` that all three call, tested once.
- cross-package: no

## ENG-16 Stringly-typed errors and parameters in internal and public APIs
- severity: low
- category: design
- location: crates/engine/src/download.rs:196, crates/engine/src/error.rs:63-64, crates/engine/src/error.rs:77-79, crates/engine/src/state.rs:48, crates/engine/src/http.rs:397, crates/engine/src/http.rs:544, crates/engine/src/http3.rs:140-173, crates/engine/src/http3.rs:245-250
- problem: `PathResolver` returns `Result<PathBuf, String>`, so callers cannot return a typed reason. `Error::State(String)` flattens the `serde_json` error and drops its source. `Error::Stalled(u64)` stores whole seconds, so a sub-second timeout reports "no data received for 0 seconds". `http::send` takes the range as `Option<&str>` and `probe_range` decides whether the body is the whole file by comparing that string to `"bytes=0-"`. `http3::Origin::connect` and `Outcome::Unusable` carry `String` errors, losing the source chain that `NetworkError` otherwise preserves (and the internal `Outcome` name shadows the public `Outcome`).
- fix: `PathResolver` returning `Result<PathBuf, Box<dyn std::error::Error + Send + Sync>>`; `Error::State { source: serde_json::Error }` plus a separate "unsupported version" variant; `Stalled { after: Duration }`; a typed `RequestRange { from, to: Option<u64> }` for `send`/`probe`; `Error::transport` for http3 failures and rename the internal enum to `H3Failure`. The public changes need a minor-version note since `Error` is `#[non_exhaustive]`.
- cross-package: yes (core matches on `Error` variants)

## ENG-17 Public module surface exposes internals and duplicate paths
- severity: low
- category: design
- location: crates/engine/src/lib.rs:213, crates/engine/src/lib.rs:219, crates/engine/src/lib.rs:227-236, crates/engine/src/naming.rs:181-205
- problem: `pub mod fetch` plus `pub use fetch::{Fetched, Fetcher, PrivateNetwork}` gives every type two public paths. `pub mod naming` publishes `part_paths` and `unique_path` (a blocking, racy helper, see ENG-11) and `from_content_disposition`, freezing them into the semver surface of a crates.io library although only `sanitize` is used outside the crate. `pub use reqwest::header` ties the public API to the reqwest re-export path rather than the stable `http` crate it comes from.
- fix: Make `fetch` private (keep the re-exports), make `naming` private and re-export only `sanitize` (and `from_content_disposition` if wanted), and re-export headers from `http::header` directly. Do it before the next minor release while the API is still young.
- cross-package: yes (core imports `wirecatch_engine::naming::sanitize` and `wirecatch_engine::fetch`)

## ENG-18 Retry jitter comes from the wall clock, so simultaneous failures retry in lockstep
- severity: low
- category: concurrency
- location: crates/engine/src/worker.rs:388-394
- problem: `jitter()` is `SystemTime::now().subsec_millis() % 250`. Workers that fail together (a server dropping all connections at once, the case the jitter exists for) read the clock within the same millisecond and get the same jitter, so they reconnect in lockstep and are refused together again, which the connection-limit logic then misreads as refusals.
- fix: Use a per-call random value (for example `fastrand`, or a per-worker seed from the worker id mixed with the clock) for jitter, or decorrelated jitter from the previous delay.
- cross-package: no

## ENG-19 parse_content_range turns "bytes */N" into a range starting at 0
- severity: low
- category: error-handling
- location: crates/engine/src/http.rs:644-666, crates/engine/src/http.rs:531-537, crates/engine/src/http.rs:609
- problem: The unsatisfied-range form `bytes */N` is returned as `ContentRange { start: 0, end_inclusive: 0, total }`. `probe_range` then accepts a `206` carrying it as a valid ranged answer starting at byte 0, and `http::fetch` accepts it for `from == 0`, although the body is not that range. The type cannot tell "no span" from "span 0-0".
- fix: Model the span as `Option<(u64, u64)>` (or a separate `Unsatisfied { total }` variant) and only accept a `206` whose span is present and starts at `from`.
- cross-package: no

## ENG-20 Credential stripping is implemented three times and lives in the FTP module
- severity: low
- category: duplication
- location: crates/engine/src/ftp/mod.rs:365-370, crates/engine/src/http.rs:206-210, crates/engine/src/fetch.rs:890-900, crates/engine/src/download.rs:406, crates/engine/src/download.rs:696-699
- problem: `ftp::without_password` (password only), `http::split_userinfo` (user and password) and `fetch::clean_url` (user and password) each strip credentials with their own `let _ = set_...` calls. `download.rs` uses the FTP helper for HTTP URLs to build the resume key, so an HTTP URL keeps its user name in the `.wcstate` file while the request URL drops it, and `same_download` needs a compatibility shim.
- fix: One `url_util::without_credentials(&Url) -> Url` (and `without_password` if the user name must stay for display), used by all three, with the resume key built from the same function as the request URL.
- cross-package: no

## ENG-21 Long extensions defeat the name-length cap
- severity: low
- category: error-handling
- location: crates/engine/src/naming.rs:159-170
- problem: `truncate_keeping_extension` computes `room = max.saturating_sub(ext.len())`; when the "extension" (everything after the last dot) is longer than 200 bytes, `room` is 0 and the function returns the extension untouched, which can exceed the 255-byte file name limit. A hostile `Content-Disposition` such as `filename="a.<300 chars>"` then fails file creation with an unhelpful I/O error instead of being shortened.
- fix: If `ext.len()` exceeds a sane maximum (for example 32 bytes), treat the name as having no extension, and always assert the result is within `max` (truncate on a char boundary as a last resort). Add a unit test.
- cross-package: no

Summary: 21 findings, 2 high (a failed write can be recorded as flushed and silently corrupt files; unvalidated options hang, panic or disable checkpoints), 11 medium, 8 low.
Biggest themes: correctness under failure (write errors, corrupt resume files, same-size remote changes, no fsync), blocking I/O and an uncancellable rate-limit sleep on the async path, and HTTP range/retry logic copied three ways.
Unsafe code (two FFI calls in storage.rs) is sound with adequate SAFETY comments; no unsafe findings.

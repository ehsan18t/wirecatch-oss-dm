# Download reliability

A busy CDN should slow a download down and let it finish. The main acceptance case uses two active downloads on one host, one expired link and one cancelled waiter. The active downloads finish with the right bytes, the expired link takes its usual failure or page-refresh path, and cancelling the waiter takes effect while the host is still resting.

## Delivery order

1. **C and D first.** Add failure diagnostics and fix the yt-dlp working directory. Each can ship as soon as its own checks pass.
2. **A next.** Share pacing across HTTP(S) files and native streams. Use the diagnostics from C for the new wait events.
3. **B after A.** Enable direct HTTP(S) media files with cookies once reading ownership, redirect scope and fallback pass the checks below.
4. **E and F separately.** The Windows window investigation and the Cargo manifest cleanup can proceed independently of download reliability.

These are planned changes. Record each product decision with the implementation that ships it. A revises D69, D271 and D283. B revises the cookie routing introduced in PR 287 and the reading reuse in D223, subject to D280's private-session rules.

## Part A. Share a host's pace across HTTP downloads

On 2026-11-21 an HLS download failed on part 51 of 120 with HTTP 410, then resumed and finished when started again 10 to 15 seconds later. Refusals followed bursts of 51 parts in 27 seconds and 35 in 7 seconds. Two concurrent downloads from the CDN failed six times in 21 seconds. PR 538 added per-host pacing to stream segments. Plain files and streams can still compete for the same host without sharing that limit.

### Decide what to retry

| What happened                                                                                                                 | What Wirecatch does                                                                                                                                                                                                                                                                                                                                        |
| ----------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| A request receives `429` or `503`                                                                                             | Rest the responding host and apply the request's retry budget. Honour a valid `Retry-After` up to the engine's existing five-minute cap.                                                                                                                                                                                                                   |
| A new file or format receives `403` or `410` on its first request                                                             | After any existing host wait, use the link's normal failure or media refresh path. Another download's success or refusal supplies no evidence about this link.                                                                                                                                                                                             |
| A later request receives `403` or `410` after this host served data for this file or earlier parts of this stream in this run | Allow bounded retries on the host. A signed address can also expire partway. For a refused redirect target, first follow the original link once as the engine does today and validate that the renewed target serves the same file. If retries end in refusal, return the actual status to the caller, including the manager's existing page-refresh path. |
| The connection drops, a read stalls or the proxy refuses the connection                                                       | Keep the engine's existing reconnect, connection step-back and proxy rules (D69, D102, D271). A transport error alone does not establish a host-wide HTTP limit.                                                                                                                                                                                           |

`472` stays outside these rules for now. On 2026-11-21, download 189 failed twice with `472` about six seconds apart, but neither entry names the responding host, says whether bytes had already arrived, or shows the same request succeeding after a wait. Use Part C to identify the responding host and whether it had already served this transfer. Before adding `472` to the partway row, confirm in a request trace that the same host served data, refused a request with `472`, then served that same request after a wait. A broader rule for every `4xx` would also retry answers such as `405` and `451`, and slow every download sharing that host.

Count each failed attempt once. The existing file retry budget is `max_retries` (default six retries). Preserve D283's four retries for an ambiguous stream-segment refusal. Give each failure one retry decision in the layer that owns the request. In particular, a stream worker must not start a fresh retry loop after the fetcher exhausts that refusal. Progress on the affected work resets its consecutive no-progress count under the existing rules. Waiting for another request to release a turn consumes no attempt. A page refresh obtains new addresses and retains its existing separate schedule.

### Apply the limit at the request

The manager supplies one shared pace to its native HTTP(S) downloads across all queues. The CLI creates one for its run. Key it by the destination host and effective port, including when the download uses a proxy. Keep only the in-flight count, limit, recovery counters and monotonic rest deadline in memory. FTP needs its own session and data-connection acceptance case before joining this policy. Page reads and transfers made by yt-dlp stay under their existing rules.

- Take one turn for each actual outbound request, including probes, ranges, manifests, segments and encryption keys. Hold it through the network response body. Release it on completion, error, redirect or cancellation, before later disk work or another request. A redirect takes a new turn for the next hop. Remove the stream worker's outer turn around a whole segment before enabling the shared one. An encrypted stream must be able to request a same-host key with a host limit of one.
- On a qualifying refusal, rest for two seconds, doubling consecutive refusals up to 30 seconds. Set the limit to half the requests in flight at refusal, rounded down, with a minimum of one. Let requests already in flight finish. A completed response that successfully serves the requested work resets the refusal streak. Eight consecutive successes raise a limited host's allowance by one. Each download's configured connection count still bounds its own work.
- Use the latest of the existing host deadline, the applicable host backoff, the worker's retry deadline and any accepted `Retry-After` for the next eligible attempt. Apply a worker-only delay to that worker. A shorter refusal or a late success cannot shorten a host rest already promised. Recheck the deadline and capacity when a waiter wakes. Pause and cancel also wake a waiter. A retry budget bounds attempts. Server-directed waits, contention, progress and page refreshes determine elapsed time.
- Private transfers participate in pacing. Attribute a rest event to the request that caused it, inside its existing private or regular logging span (D280.6). A peer waking up must not emit the triggering host event again.

### Show what the download is doing

| Download state                                        | Status in the list                        |
| ----------------------------------------------------- | ----------------------------------------- |
| Its own request failed and is waiting to retry        | “Waiting for server · 8 s · retry 2 of 6” |
| It is waiting on another request's host rest          | “Waiting for server · 8 s”                |
| The deadline passed and it is waiting for capacity    | “Waiting for server”                      |
| Its retry has started and data has yet to arrive      | “Retrying · 2 of 6”                       |
| A connection dropped and the download is reconnecting | Existing D271 reconnect status            |

The counter shows the budget for the work being retried: six retries by default for a file request and four for an ambiguous stream-segment refusal. A peer waiting for its turn uses no retries and shows only the wait.

Show the wait only while no connection is delivering data. While waiting, leave speed and ETA empty. Show the number of active requests wherever the UI reports connections. New data restores normal progress. Preserve the final server error and include a retry count only when known.

### Acceptance

- Use one local host for an encrypted HLS stream and a ranged file. Make the host refuse work after progress and reduce its limit to one. Count requests and hold response bodies open to prove that existing transfers drain, then that the key, later segments and file ranges take turns. Both outputs must match the expected bytes.
- During that rest, queue a fresh link that will return `410` and a separate waiter to cancel. Verify prompt cancellation before releasing the rest, zero retry charges for queueing, and the normal handling of the fresh link's first response. Cover renewal of an expired redirect target and rejection of a changed file with the engine's existing validator tests.
- Reuse the paused-clock pacing tests for halving, recovery and exponential backoff. Add the interaction with a longer `Retry-After`, including a later shorter refusal, and assert the number of actual requests when retries run out. Check the displayed retry budget for each kind of request, the countdown without a retry count for a waiting peer, and the return to normal progress when data arrives. Use events, counters and paused time for timing assertions. Run the repository's before-and-after engine benchmark for the transfer-path change.

## Part B. Download direct media files with their format cookies

yt-dlp can return cookies alongside a format's URL and headers. Since PR 287, a direct video or audio file that needs format cookies goes through yt-dlp and typically downloads over one connection, even when the server supports parallel ranges. Letting the engine fetch an eligible file brings back parallel ranges, engine progress and retries.

### Own the reading before using its credentials

A regular page reading currently survives for up to 30 minutes in a cache keyed by page URL. Two browser profiles can open that URL under different accounts. Keep a credential-bearing reading in memory with the pending selection that obtained it, then with the download created from that selection. A second capture of the same page obtains its own credentials. Preserve reuse for the owning download within the existing lifetime. On expiry, restart, or a missing owner, read again with that download's own context. Discard private state when the session ends. Keep the cookies out of the database, work folder and logs.

### Choose the downloader for the whole selection

1. Route a selection natively only if every selected format is eligible. The new case is a direct HTTP(S) file with static format cookies whose scope the engine can establish. Continue existing native handling for supported formats without cookies. A cookie-bearing HLS format, an unsupported protocol, ambiguous cookie scope or a format requiring yt-dlp sends the whole selection to yt-dlp.
2. Derive the cookie header separately for each probe, range and redirect destination, checking host-only or valid domain scope, path boundaries, expiry and the secure flag. Preserve the engine's origin restrictions on other format headers. Use only cookies supplied for that format. The page's browser cookie header is not a substitute.
3. Let yt-dlp handle response-cookie rotation in this release. If rotation is known to be required, select yt-dlp at the outset. If a native response sets or changes a cookie that could affect later requests and native handling cannot establish safe continuation, hand the whole selection to yt-dlp once. Keep that choice for the run so a refreshed reading cannot send it back into the same attempt. Each downloader may resume only partials it can validate as its own.
4. Apply the native local-network rule to every native transfer and redirect. Keep the existing early check for cookie formats when any track sends the selection to yt-dlp, as well as yt-dlp's run guard under D136. A first-request refusal and a genuine expiry use A's renewal and page-refresh rules.

### Acceptance

- From two ordinary profiles, select the same page under different accounts before starting either download. Each test server request must receive only the credential belonging to its selection. Exercise reuse by the owning download and private-session teardown.
- Serve a ranged file and record the cookies on the probe, concurrent ranges and redirect hops. Cover an eligible same-domain target, a host-only cookie on a sibling host, an unrelated host, a path mismatch, expiry and a secure cookie over HTTP.
- Pair eligible video with unsupported audio. Verify the whole selection uses yt-dlp and that the early local-network check still refuses a disallowed format. Check that a cookie-rotation handoff uses yt-dlp once and produces the expected output. Extend native cookie support to HLS only after equivalent tests cover manifests, keys and segments.

## Part C. Identify where a download failed

The November log identified download 187 and segment 51 of 120. Once the row disappeared, the report could no longer distinguish the page site from the server that refused the segment.

Write one structured terminal warning per failed regular download. Include Wirecatch's download ID and the download kind, failure stage (such as page read, file transfer, stream segment or merge), and the HTTP status or error category. When known, include the page host, responding host, extractor name, retry number and budget, and segment index and total. For an HTTP refusal, record whether that host had already served data for this transfer when the request failed. A local merge failure can have no responding host. Carry the fields from the point of failure so the logger can use them without parsing display text.

The extractor's video ID can come from the URL path and can identify a person's viewing history. Omit that ID, URL paths, queries, fragments, user information, header values and local filenames from these diagnostics, including nested error text. Someone who wants support to identify the exact video can include its page link in their report. D40's redactor cannot recognize an arbitrary token in the middle of a URL path. When A lands, emit one information event for a regular request that triggers a rest, with the host, cause, rest duration and limit. D280.6 applies to both events at every log level.

Capture a regular page/segment failure and a local failure. Verify the available fields, one terminal warning and the absence of sample secrets in the path, query, headers, proxy credentials and a URL-derived video ID. Repeat a private failure that triggers a rest while a regular peer waits. Neither the private failure nor its rest event may appear in the log.

## Part D. Start yt-dlp in a short working directory

PR 538 moved ffmpeg's working directory because a deep download folder can prevent Windows from starting the child. yt-dlp still starts in that folder so it can use the relative output template `media.%(ext)s`.

Start yt-dlp in a short app-owned directory and keep the relative template. Pass the actual work folder separately using `--paths`. Continue to watch that work folder for progress, keep partial files there, and accept a final file only after the existing output checks confirm it is inside the folder. If Windows starts the child but rejects a long output path, report the path-length failure and suggest a shorter destination.

yt-dlp expands the home path using the platform's environment-variable rules. Doubling every percent and dollar sign changes valid Linux folder names. Pass a fixed variable reference as the home path, with the literal folder supplied in the child's environment. A Windows check with the installed yt-dlp `2026.10.26` wrote all 65,536 fixture bytes to the exact 295-character work path containing literal percent and dollar signs and `%(title)s`. [Path handling in yt-dlp 2026.08.19](https://github.com/yt-dlp/yt-dlp/blob/2026.08.19/yt_dlp/YoutubeDL.py#L1139-L1146)

The add-on updater can install a release newer than the one in the manifest. For the repeatable CI check, resolve the binary from the current add-on manifest for each platform and print its actual `--version` in the test output. Run it against a local file server on Windows and Linux so a pin change exercises that release's path handling. Choose a work folder containing a literal percent, a reference to a defined environment variable and `%(title)s`. On Windows also make the absolute work path exceed 260 characters. Assert that the child started and that the expected bytes are in the exact requested folder. The existing stand-in covers arguments, progress and result discovery. Real-tool coverage establishes how yt-dlp interprets the path.

## Part E. Trace a Windows window-close error

The missing-icon flood came from an older build. The desktop suite already has `every_icon_the_app_names_is_bundled` in `crates/desktop/src/ui/icon.rs`. The remaining reports include “Invalid window handle” and “window not found” during active-status changes.

Limit the initial investigation to one pass through the main window, download window, question, settings and tray menu on a current Windows build. For the first reproducible sequence, record the window, close action and late event. Fix the lifecycle in the component that owns it, whether Wirecatch or GPUI, then repeat that sequence and verify both the expected close behavior and the absence of that error. If the pass produces no reproduction, record the build and sequences tried and defer a fix until there is a trace.

### Windows trace

Closing the main window to the tray in the installed Windows build reproduced “handling active status change for window …: window not found”. The pinned GPUI Windows backend hides the HWND asynchronously during teardown. Its activation handler can still enqueue a callback that tries to update the already-removed GPUI window. The fix belongs in the Windows backend's teardown and callback lifetime. Repeat the same close sequence after a toolkit fix and confirm the tray can reopen the main window.

## Part F. Consolidate Cargo dependency versions

PR 537 moved several repeated crate versions into the workspace manifest. A review scan on 2026-11-21 still found 17 dependency names shared across crates with at least one direct declaration, including `sha2`, `rcgen` and `windows-sys`. Include that migration in the estimate for a separate maintenance PR.

Inventory current declarations by crates.io package name, accounting for aliases and normal, dev, build and target-specific sections. Move compatible shared requirements to `[workspace.dependencies]` and have members inherit them. Preserve each member's features, optionality and default-feature behavior. Review any incompatible requirements explicitly before changing the resolved versions.

Add a repository gate that names each offending manifest and declaration when a shared crates.io dependency declares its version locally, including an override of an existing workspace declaration. Test the manifest shapes above plus a clean workspace, then run the gate against the migrated tree and check the dependency and feature changes. The extension's npm manifests are outside this Cargo change. A pnpm lockfile can contain multiple versions of a package.

## Separate browser-routing checks

D282's installed-app run through a real extension and the Firefox page-read measurement remain separate from this work.

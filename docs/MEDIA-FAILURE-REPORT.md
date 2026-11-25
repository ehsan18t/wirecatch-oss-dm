# PornHub downloads: what fails, why, and what else it affects

Investigation run on 2026-11-19 in the worktree `fix/media-fetch-reliability`, against the installed app 0.1.0, yt-dlp 2026.10.26, Deno 2.9.7 and ffmpeg 9.0.2.

## Bottom line

PornHub is reachable and a freshly read media URL downloads fine. What fails is the page read: PornHub serves a JavaScript gate (the `KEY` challenge) after it decides a client has asked too much, too fast, or in a way it distrusts. yt-dlp's PornHub extractor can pass that gate only by running PhantomJS, and PhantomJS is not part of Wirecatch's media add-on (which ships yt-dlp, ffmpeg and Deno). So the page read fails every time the gate appears, and it fails with one of three server responses: the challenge page, `HTTP Error 410`, or a TLS reset during the handshake. Wirecatch then reports two misleading messages: "The media add-on could not reach the site. Check your connection" and "The server says this file does not exist (error 410). Check the link". Neither is what happened.

The gate is solvable with the Deno runtime the app already ships. I proved it end to end: solved the challenge from the page's own script in Deno, put the resulting `KEY` cookie in yt-dlp's Netscape cookie jar, and yt-dlp then read the page and offered 1080p, 3 runs out of 3.

The same class of failure exists on two other site families in yt-dlp (Douyu and iQiyi), for the same missing-PhantomJS reason, but those need a different fix because yt-dlp consumes their JS output internally rather than just a cookie. Popular sites that use yt-dlp's browser impersonation without PhantomJS (SpankBang, Chaturbate, xHamster, TV5Monde+) are not affected by this gate, though a secondary User-Agent mismatch in Wirecatch's invocation can weaken them. YouTube, X/Twitter, RedTube, YouPorn, Tube8 and XVideos use neither mechanism and are not affected.

Section 10 reconstructs when it broke. The two obvious suspects are both ruled out: it is not a yt-dlp update, and it is not the D136 guard. What is left is the session cookie the gate depends on, or a change at PornHub's end.

## 1. What the three logs actually contain

`wirecatch-app.log` is the only log with relevant material. It covers 2026-10-22 to 2026-11-19. The failure window is 2026-11-19 01:11:48 to 01:13:26 UTC.

| Time (UTC) | Source | Line, shortened | What it is |
| --- | --- | --- | --- |
| 01:11:48.861 | `wirecatch_core::manager::media` | `media could not be read error=The server says this file does not exist (error 410)...` | A captured stream or media URL was refused by the server. This is the engine path, not yt-dlp. |
| 01:11:55.295 | same | same 410 | Second attempt at a captured media URL. |
| 01:12:47.360 | `wirecatch_streams::process::run` | `a program failed ... yt-dlp.exe status=exit code: 1 stderr=ERROR: [PornHub] <id>: Unable to download webpage: Failed to perform, curl: (35) BoringSSL SSL_connect: Connection closed abruptly (SSL_ERROR_SYSCALL; error queue empty) in connection to www.pornhub.com:443` | A page read. The TLS connection was closed after the ClientHello, before any HTTP request. |
| 01:12:47.360 | `wirecatch_core::manager::media` | `media could not be read error=The media add-on could not reach the site. Check your connection, then try again.` | The app's translation of the line above. It blames the person's connection. |
| 01:12:58.115 | same | 410 | Another captured media URL. |
| 01:13:10.112 | same | 410 | Another captured media URL. |
| 01:13:26.224 | `wirecatch_streams::process::run` | the same SSL reset | Second page read. |
| 01:13:26.224 | `wirecatch_core::manager::media` | the same "could not reach the site" | Translation of the second page read. |

Three things follow from the table.

First, there were two different kinds of attempt. The four 410 entries have no `wirecatch_streams::process::run` line before them, so they are not yt-dlp failures. They come from `read_stream`, meaning the app was given a captured media URL (a direct file or an HLS/DASH manifest), fetched it with the engine, and the server answered 410. The message text is generated in `crates/messages/src/lib.rs:450`, the engine's wording for a refused link, not the media add-on's wording. yt-dlp's own 410 would have been worded "This page could not be found" by `crates/media/src/describe.rs:84`, which is not in the log.

Second, the two page reads failed before HTTP. `curl_cffi` (the BoringSSL-based client inside yt-dlp) completed the TCP connection through the loopback guard, sent the TLS ClientHello, and the server closed the connection without a TLS alert. That is not a DNS failure, a proxy failure or a normal HTTP error. It is the server or its edge refusing the client at the TLS layer.

Third, the app cannot show which URL each attempt used. The log line names only the video ID and yt-dlp's words. The real history is `%LOCALAPPDATA%\Wirecatch\wirecatch.db` (the `%APPDATA%` file is a legacy settings-only copy). It holds 36 rows, from ID 30 to 168, none of them PornHub, and its `last_ids.download` is 174. The six attempts in this window line up with IDs 169 to 174, which suggests they were recorded and their rows later removed. The `origin` column that held their cookies went with them. That is a gap for any future diagnosis: a media failure should log its own redacted source URL.

`wirecatch-bridge.log` contains only relay open/close lines, last entry 2026-11-13. `wirecatch.log` is a CLI log from 2026-11-04 with a certificate-chain test against CA test servers, unrelated to this failure.

## 2. What PornHub does

PornHub sits behind an anti-bot system that answers a suspicious client in one of four ways. I observed all four from the same machine and the same yt-dlp build.

1. **The real page.** About 1.5 to 1.6 MB, contains `flashvars_<n>` and `mediaDefinitions` with signed `https://ev.phncdn.com/...mp4?validfrom=...&validto=...&hash=...` addresses. This is what a browser receives after it has passed the gate.
2. **The `KEY` challenge.** A page of about 1.4 KB. It defines `leastFactor(n)`, a `go()` function that mutates a constant through arithmetic on a second constant, sets `document.cookie = "KEY=" + n + "*" + p/n + ":" + s + ":<constant>:1;path=/"`, and reloads. The server validates the cookie on the next request and hands over the real page. yt-dlp's extractor recognizes it by one of three patterns: `<body onload="go()">`, `document.cookie = "RNKEY="`, or `document.location.reload(true)`.
3. **`HTTP Error 410: Gone`.** PornHub's other bot response. It is also what a removed video returns, which makes the two impossible to tell apart from the text alone.
4. **A TLS reset.** The connection is dropped after the ClientHello. `curl_cffi` reports `SSL_ERROR_SYSCALL` and "Connection closed abruptly". This is what the app log shows.

The gate is not tied to the URL. It is tied to the address and its recent request history; what else the anti-bot weighs is not known. Two observations support the rate part.

- The first ten runs with the app's arguments, straight to the internet, all succeeded. Immediately after, the same command through a loopback HTTP proxy was challenged ten times in a row. From then on, direct runs were challenged too, and the block was still active for the rest of the investigation. The proxy and the rate increase were confounded in that experiment, so the proxy is not proven to be the trigger; the persistence after the experiment is the important part.
- Once the block is active, every attempt gets the challenge, whichever User-Agent is sent. I tried no override, a Chrome 145 User-Agent, and a Firefox 140 User-Agent. All three were challenged in the second and third rounds.

The cookie is the proof of work. A browser loads the challenge, runs the arithmetic, sets `KEY`, reloads once, and keeps the cookie for the life of the session. When the browser's cookie is not there, yt-dlp never runs the challenge, so it never has that cookie.

### 2.1 The 410 in yt-dlp's history is a different bug

There is a well-known PornHub `HTTP Error 410` report, yt-dlp issue #16729. It was fixed by PR #16794, merged 2026-06-09, by adding `impersonate=True` to the extractor's page requests. The installed yt-dlp (2026.10.26) contains that fix. So the 410 in this log is not the old extractor bug; it is the gate's other answer, or a stale signed URL on the captured media path. The distinction matters because updating yt-dlp again will not fix it.

## 3. Why yt-dlp cannot pass the gate

The relevant code is `yt_dlp/extractor/pornhub.py`. `PornHubBaseIE._download_webpage_handle` downloads the page, checks the three challenge patterns, and if one matches does this:

```python
phantom = PhantomJSwrapper(self, required_version='2.0')
phantom.get(url, html=webpage)
webpage, urlh = dl(*args, **kwargs)
```

`PhantomJSwrapper` is in `yt_dlp/extractor/openload.py`. It checks for a `phantomjs` executable on PATH and raises `ExtractorError(f'PhantomJS not found, {self.INSTALL_HINT}', expected=True)` when none is found. When it is found, it writes the page to a temp file, runs PhantomJS with the page, lets the challenge JS run, saves PhantomJS's cookie jar, loads those cookies into yt-dlp's cookie jar, and retries the page. In other words, the wrapper's job here is exactly two things: execute the page's arithmetic and copy the resulting cookie into yt-dlp.

PhantomJS is not an option for Wirecatch:

- The last release is 2.1.1 from 2016 and the project is discontinued. Shipping it would add an unmaintained binary with a large attack surface, for a single cookie calculation.
- The media add-on is defined by D20 as yt-dlp, ffmpeg and Deno, each pinned to an official release and verified by checksum. PhantomJS is not in that set, and the add-on code has no `ComponentId` for it.
- yt-dlp itself would rather not use it: issue #1354 ("Replace phantomjs with something else?") has been open since 2021, and issue #14812 is the PornHub-specific "PhantomJS not found" report, still open.

So the gate is fatal by construction: the page cannot be read without JS, and Wirecatch cannot run the JS yt-dlp wants.

## 4. Why the person hits the gate and the browser does not

A browser on the same machine passes the gate because it runs the script and keeps the cookie. Wirecatch tries to reuse the browser's cookies, and the plumbing is there: the extension reads the cookie store (`extension/src/lib/cookies.ts`), the context menu and media button attach the resulting `Cookie` header (`extension/src/lib/hub/hub.ts:232`, `extension/src/lib/hub/media-sender.ts`), and the app writes it into a Netscape cookie file for yt-dlp (`crates/media/src/ytdlp/cookies.rs:94`). For PornHub the cookies are sent from the first try (`cookies_first` is false only for YouTube).

For completeness, yt-dlp's own `--cookies-from-browser chrome` fails on this machine with `Failed to decrypt with DPAPI`, the Chrome app-bound encryption problem tracked as yt-dlp issue #10927. The extension's capture is therefore the only working way to give yt-dlp Chrome's cookies here, which makes the capture path in section 4 worth keeping healthy.

That still fails in several common cases.

- **The `KEY` cookie is a session cookie.** The challenge sets it with no `expires`, so closing the browser drops it. A download started after the browser is closed has no key to reuse.
- **The browser may never have been challenged.** PornHub decides per address and per session. A residential address that has not tripped the gate has no `KEY` cookie to give. The app's first request from the same address can then be the one that trips it.
- **The capture may predate the challenge, or come from another network.** With WARP on and off the browser sees a different address, the sessions are different, and a key obtained under one address may not be accepted under another.
- **The page read may not be the request that was captured.** A media button or context-menu click on the `<video>` element sends the CDN URL, not the page URL, and the CDN cookies say nothing about the page gate.

I could not verify which of these applied here: the six failing attempts were recorded as downloads 169 to 174 and their rows were removed, so the cookies that were actually sent at 01:11 are not recoverable.

### 4.1 A secondary bug: the app overrides the impersonated User-Agent

Wirecatch adds the browser's User-Agent to every yt-dlp run that has a browser capture (`crates/media/src/ytdlp/mod.rs:363`). yt-dlp's PornHub extractor requests impersonation, and `curl_cffi` sends Chrome's Client Hints with it (`sec-ch-ua: "Chromium";v="152", ..., "Google Chrome";v="152"`). The explicit header wins for `User-Agent`, but the Client Hints are not overridden. I measured this with `--print-traffic`:

```
user-agent: Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:140.0) Gecko/20100101 Firefox/140.0
sec-ch-ua:  "Chromium";v="152", "Not?A_Brand";v="24", "Google Chrome";v="152"
```

A User-Agent that says Firefox with Client Hints that say Chrome 152 is exactly the kind of inconsistency a bot check looks for. With a real Chrome browser the mismatch is smaller (the browser's Chrome version versus yt-dlp's impersonated Chrome 152) but still present. This is a plausible contributor to the first trip, not a proven cause, and it is worth removing or reconciling regardless.

### 4.2 An open question: the D136 guard proxy

Every direct yt-dlp run goes through a loopback HTTP proxy (D136, `crates/media/src/ytdlp/guard.rs`), which refuses connections into the local network. That is a Wirecatch-specific difference from a hand-run yt-dlp. The first A/B I ran looked dramatic: 10 of 10 direct runs passed, then 10 of 10 proxied runs were challenged. But the runs were sequential, and after the tenth direct run the address stayed blocked for everything, direct included, so a cumulative rate trigger explained the same data. A later test settled the important half: with a freshly solved `KEY` cookie, the same command read 1080p both directly and through the proxy. The guard does not block a valid session. Whether it makes the first, cookie-less request more likely to be gated is still open.

## 5. Reproduction and proof

All commands below use the installed media add-on from `%LOCALAPPDATA%\Wirecatch\addons\media\`. The page URL from the log is replaced by `<page-url>` because the report should not carry the person's viewing history; the behaviour is not specific to that video.

### 5.1 The gate appears on demand

| Run | Command shape | Result |
| --- | --- | --- |
| 10 direct runs | the app's arguments, run without a proxy | 10 of 10 read the page and printed the video ID |
| 10 runs through a loopback CONNECT proxy | same, with the proxy set | 10 of 10 returned the `KEY` challenge, surfaced by yt-dlp as `PhantomJS not found` |
| Later direct runs | same | challenge, block persisted for the rest of the investigation |
| With no override, Chrome 145 UA, Firefox 140 UA | same | all challenged once the block was active |

The proxy used for the test was a plain Python `CONNECT` tunnel, equivalent to the guard's tunnel path. `--print-traffic` confirmed the origin request was identical in both cases: HTTP/2, the same headers, the same cookies, the same `sec-ch-ua`. The only difference on the wire was which process opened the TCP connection to the origin.

The runs used `--simulate --print` in place of the app's `-J`, and sent no browser cookies. That difference matters for the exact bytes on the wire but not for the gate: the failing app runs and the test runs fail the same way once the address is flagged.

### 5.2 The challenge page

The dump is about 1.4 KB. Its operative part:

```html
<html><head><script type="text/javascript"><!--
function leastFactor(n) { ... }
function go() {
 var p=2297993036744; var s=3657345595; var n;
 ... arithmetic on p driven by the bits of s ...
 n=leastFactor(p);
 document.cookie="KEY="+n+"*"+p/n+":"+s+":1981505470:1;path=/;";
 document.location.reload(true);
}
//--></script></head>
<body onload="go()">
</body></html>
```

The constants change per challenge. The shape does not.

### 5.3 Solving it with the shipped Deno

I extracted the script, stripped the HTML comment markers, and ran it in Deno with a minimal DOM stub and no permissions:

```js
const document = { cookie: "", location: { reload() {} } };
// <the page's script goes here>
go();
console.log(document.cookie);
```

`deno run solve.js` printed a cookie of this shape:

```
KEY=<a factor of p>*<p divided by that factor>:<s>:<constant>:1;path=/
```

I then wrote that cookie into a Netscape file, exactly as `crates/media/src/ytdlp/cookies.rs:94` does, and ran the full app invocation with `--cookies <file>`:

| Run | Result |
| --- | --- |
| 1 | `<id>\|1080p`, exit 0 |
| 2 | `<id>\|1080p`, exit 0 |
| 3 | `<id>\|1080p`, exit 0 |

Before the cookie, the same invocation failed every time with `PhantomJS not found`. So the only missing piece is the cookie, and Deno, which the app already ships, can produce it.

### 5.4 The media URL is fine

I took a fresh 1080p format from the successful read and fetched its first 1024 bytes with the format's `Origin`, `Referer` and User-Agent, with and without the format's `__cf_bm` cookie. Both answered `206 Partial Content`. The engine's decision to drop per-format cookies (`crates/core/src/manager/media_run/page.rs:463`) is therefore not what breaks PornHub downloads.

### 5.5 What the 410 likely is

The page advertises HLS manifests as well as MP4. In my runs, yt-dlp logged four times: `Failed to download m3u8 information: HTTP Error 410: Gone`, then continued with the MP4 formats, which worked. A captured stream link can therefore be an HLS manifest that answers 410, or a signed MP4 whose `validto` or `ip` no longer matches. The log cannot tell which, because it does not record the URL. This is a separate issue from the page gate, though both are PornHub refusing the app.

## 6. Which other media sites are affected

I checked the current yt-dlp extractor sources for the two mechanisms involved: `PhantomJSwrapper` and `impersonate=True`.

| Site or family | Mechanism in yt-dlp | Same failure? | Why |
| --- | --- | --- | --- |
| PornHub, PornHub Premium, PornHub `.net`/`.org` | `PhantomJS` for the `KEY`/`RNKEY` gate, plus `impersonate=True` | Yes, exactly | The page read cannot pass the gate without PhantomJS |
| Douyu (`douyutv.py`) | `PhantomJSwrapper.execute` for a signing script | Yes, same missing-PhantomJS error | yt-dlp needs the script's output to sign the stream request, not a cookie, so a cookie solver does not help |
| iQiyi (`iqiyi.py`) | `PhantomJSwrapper.get` for a DASH signature script | Yes, same missing-PhantomJS error | Same: the output is consumed inside the extractor |
| SpankBang | `impersonate=True` only | No for this gate | Live test without browser cookies answered 403 from its own bot check; the extension's cookies are the intended route for that, not a PhantomJS solver |
| Chaturbate | `impersonate=True` only | No for this gate | Same mechanism as the other impersonation-only rows; source-based |
| xHamster | `impersonate=True` only | No for this gate | Live test without cookies read the page and offered 720p |
| TV5Monde+ | `impersonate=True` only | No for this gate | Same mechanism as the other impersonation-only rows; source-based |
| RedTube | neither | No | Plain extractor |
| YouPorn, Tube8, XVideos | neither | No | Plain extractors |
| YouTube | own JS challenge framework, uses Deno | No for this gate | The app passes `--js-runtimes deno:<path>` and yt-dlp reports `deno-2.9.7`; a public test video read successfully in this investigation |
| X/Twitter | no PhantomJS | No | A public video tweet read successfully |
| Vimeo | no PhantomJS | No | yt-dlp now demands a login for the web client; unrelated to this failure |

Two qualifications.

The PornHub gate is not the same as the old 410 bug. Updating yt-dlp again will not fix it. The installed version is already newer than the fix.

The Chaturbate and TV5Monde+ rows come from the extractor sources; I did not live-test those. xHamster, YouTube and X/Twitter were live-tested and read successfully without cookies. SpankBang was live-tested and answered 403 without browser cookies, which is its own bot check and the case the extension's cookie capture exists for. Vimeo was live-tested and answered with its new login requirement.

The YouTube "Sign in to confirm you're not a bot" lines in the same log, dated 2026-11-05, are a different, well-known YouTube bot check. They are not this gate, and YouTube reads correctly now. If those return, they need their own investigation.

The add-on plan in `docs/ADD-ONS.md` does not help here either: its v1 boundary excludes cookies and signed-in media, so a new extractor add-on would not be allowed to see the browser session that the gate depends on.

## 7. Fix options

These are options, not decisions. They differ in what they promise, how much they cost, and whether they are appropriate for the product.

### Option A: solve the `KEY`/`RNKEY` challenge with Deno and retry once

This is the only option that makes PornHub downloads work from the app, and it is the closest equivalent of what yt-dlp does with PhantomJS.

Shape:

1. Wrap the page read. On a `YtDlpError::Refused` whose text contains `phantomjs not found`, fetch the page once with `wirecatch_engine::Fetcher` under the same cookies, User-Agent, proxy and `PrivateNetwork::Block` rule the run already uses.
2. If the body has the `go()`/`leastFactor`/`RNKEY` shapes, extract the challenge script and run it in Deno with no permissions, `--no-config`, a timeout and a capped output. Capture `document.cookie`.
3. Retry `ytdlp::extract` once with `KEY=...` appended to the cookie header (the Netscape file already passes every cookie pair through).
4. If the fetch or the solve fails, report the browser-check message from Option B.

Where it would live: the two call sites are `crates/core/src/manager/media.rs` (`read_page`, for the quality choice) and `crates/core/src/manager/media_run/page.rs` (`reading`, for the download). A shared helper in `crates/core/src/manager/media_run/` can wrap both, because core already has the `Fetcher`, the `ProxyChoice`, the `Origin` and the Deno path in `MediaTools`. The media crate can keep the yt-dlp invocation and the error type.

Security notes, because this runs page-authored code:

- Deno's default is no filesystem, network, environment or subprocess access, which is exactly what the calculation needs.
- Run only the script block that defines the challenge, never the whole page, and only after matching the known patterns.
- Cap V8's heap (`--v8-flags=--max-old-space-size=...`) and the output, and print the cookie behind a sentinel line so other `console.log` output from the page cannot be mistaken for it.
- The process runner already groups and kills a run, so a hostile infinite loop is bounded by the same timeout as the other add-on programs (D31).
- Tempting shortcut to avoid: reimplementing the arithmetic in Rust. The page's constants and operations change; Deno reproduces the actual script, which is why the proof worked.

Effort: one to two days including a fixture-based unit test for detection and extraction, an integration test with the local test server, and a decision entry recording the new behaviour. Risk: site-side changes to the challenge shape; the code must fail closed and fall back to Option B.

### Option B: say what actually happened

Even with Option A, the messages need fixing. The current mapping in `crates/media/src/describe.rs:78` matches on substrings and gets all three cases wrong:

| yt-dlp says | App shows | Reality |
| --- | --- | --- |
| `Connection closed abruptly` | "could not reach the site. Check your connection" | The site is reachable and is refusing automated clients |
| `http error 410` | "This page could not be found. Check the link" | The video exists; the gate or a stale signed URL answered |
| `PhantomJS not found` | "could not get the video from this page... update the media add-on" | Updating will not help; the add-on intentionally has no PhantomJS |

A correct message would name the browser check and the one action that actually works: open the page in the browser, confirm it plays, then use the extension again (which carries the fresh session cookie). This is small, testable, and worth doing on its own.

### Option C: back off after a refusal

The gate gets stricter with repeated requests. The manager caches successful readings for 30 minutes (D223) and drops the reading on any failure, so a person clicking "Try again" can send a fresh read every few seconds. A failure-specific backoff (for example no automatic or immediate retry after a 410/reset, with a "wait a minute" note) would reduce the escalation. It does not fix the first failure.

### Option D: make the browser solve it and hand over the cookie

The extension already reads the cookie store, so when the browser has a `KEY` cookie the app already gets it. The gap is the case where the browser has none. A content script or a hidden tab on the site could let the browser run the challenge and then read the cookie, without Deno and without running page JavaScript in the app. Cost: a visible or hidden tab, site permissions, and a new protocol message. It is the most faithful to "the browser is the client that passes the check", but it is more moving parts than Option A and only works while the site is open or can be opened.

### Option E: fix it upstream in yt-dlp

The proper long-term fix is for yt-dlp to run these challenges with its JS runtime framework (the same Deno it already uses for YouTube) instead of PhantomJS. That would fix PornHub, Douyu and iQiyi together. It is outside Wirecatch's release process: the add-on is pinned to official releases and verified against yt-dlp's signature (D21, D38), so a local patch or a fork is not an option. Filing or supporting the upstream issue is the right move regardless of what Wirecatch does locally.

### Option F: bundle PhantomJS

Rejected. PhantomJS 2.1.1 is from 2016 and discontinued, it would add a browser engine and a large binary to the add-on for one arithmetic result, and it conflicts with D20's "official releases, pinned and verified" rule. The Deno proof shows it is not needed.

### Option G: fall back to the captured media URL

When the page read fails and the extension saw the actual media request, the app could download that URL with its captured `Referer`, `Origin` and cookies. That is the stream path, which already exists. It works only while the URL is fresh and the media was captured, and the 410 evidence suggests captured links can be stale or HLS manifests that PornHub refuses. Useful as a fallback, not as the fix.

## 8. What can be done today, without a code change

1. Open the video in the browser and let the page finish loading. If the gate appears, the browser solves it and stores the session cookie.
2. Without closing the browser, use the extension's media button or the context menu on the page. The app receives the cookie from the live session, and the same cookie made yt-dlp succeed in the proof above.
3. If the page read still fails, use the media button on the playing video. That sends the signed media URL the browser is using, which the engine can download while it is fresh (the 206 test). This is the path that produced the 410s in the log when the URL was not fresh or was a manifest PornHub refuses, so it is a fallback, not a guarantee.
4. Do not click "Try again" repeatedly. Each attempt is a fresh read, and the block gets stricter with volume.

This works only when the browser and the app take the same route to the site. If one uses WARP and the other does not, the cookie and the signed URL belong to a different session.

## 9. Recommended order

1. Option B now. It is small, removes the wrong diagnosis from the product, and does not depend on any decision about bypassing the gate.
2. Decide whether Wirecatch should answer the gate at all. That is a product and terms-of-service decision, not an engineering one.
3. If yes, Option A, with Option C as a companion. The Deno proof shows the mechanism; the remaining work is the retry, the sandboxed runner and the tests.
4. Option E in parallel, because it is the only fix that also covers Douyu and iQiyi.

## 10. When did it break?

The logs do not answer this directly. They record failures, not successes, and the PornHub rows were removed from the history. The records do give a timeline, and three checks narrow it down.

| Date | Event | Source |
| --- | --- | --- |
| 2026-10-22 | App log begins; media add-on installed with yt-dlp 2026.08.19 | App log |
| 2026-10-24 | yt-dlp updated to 2026.10.21 | App log |
| 2026-10-29 | yt-dlp updated to 2026.10.26, still installed | App log, `installed.json` |
| 2026-11-05 | Media add-on briefly reinstalled at 2026.08.19 then updated back to 2026.10.26; D136 guard merged | App log, git |
| 2026-11-09 | A local staged build was run from `target\packages\stage-windows-x64` | App log |
| 2026-11-13 | Installed app binary rebuilt at 13:37 and first started at 16:11 | File times, app log |
| 2026-11-19 01:11 to 01:13 | The six failures | App log |

The decisions that land in this window are D136 (the guard, 2026-11-05) and D223 (the reading cache, 2026-11-06). Neither explains a cookie-dependent gate: the cache only reduces repeated reads, and the guard is cleared by the test below. The browser cookie capture that the gate depends on is D44.6, decided 2026-10-24, and it is unchanged in the failing path.

Check 1: it is not a yt-dlp update. `pornhub.py` and `openload.py` are byte-identical in 2026.08.19, 2026.10.21 and 2026.10.26. Their SHA-256 is the same in all three releases (`C678DC3E...` and `6E745192...`). The October updates did not change how PornHub is read.

Check 2: it is not the D136 guard. With a freshly solved `KEY` cookie, the same yt-dlp command read the page and offered 1080p both directly and through a loopback `CONNECT` proxy equivalent to the guard. The guard adds a hop, but it does not block a valid PornHub session, so the change from Nov 5 is not the direct cause.

Check 3: it is not the error mapping. The messages are wrong, but they are a symptom of the gate, not a cause.

What is left is the cookie. The gate is passed by a session cookie the browser gets when it runs the challenge. That cookie is not persistent, and the Chrome, Edge, Brave and Firefox stores I checked contain no PornHub cookies at all. The most likely triggers, in order:

1. The browser session ended. Closing or restarting the browser drops the `KEY` cookie, and the next handover carries no key. This is the simplest explanation for "it used to work".
2. The network changed. Turning WARP off or on changes the address and the session, and a key from one session may not be accepted in another. The person reports trying both ways.
3. The link was pasted into the app instead of handed over from the browser. A pasted link has no browser cookies at all, so it can never pass the gate.
4. PornHub tightened the gate. The gate is not new, but how often it appears and how long a key lives is PornHub's decision. Nothing in the records can rule this out.
5. The Nov 13 app or extension update. It is the only code event in the window, but the cookie call for a page is unchanged in that commit, and a loopback proxy equivalent to the guard works with a valid cookie. It is a weak suspect for the handover, not for the gate itself.

The app's settings allow private-window downloads (`private_windows: true`), so a session in a private window is a likely setup. Those cookies live only in that window's store and are gone when it closes, which fits the missing-key explanation.

There is also a version mismatch worth cleaning up: the browser extension is an unpacked development build (`chdgnmpjoipbgfeldicipccjjghmagki`), and the native messaging host files still point at `D:\Projects\wirecatch-oss-dm\target\packages\stage-windows-x64\wirecatch-bridge.exe`, written on 2026-11-09. That staged bridge is the same build as the installed one (same size and timestamp), and the installed app is the one running, so it does not explain the failure by itself. It does mean the extension, the staged bridge and the installed app are not from one release, which is worth removing before the next diagnosis.

The clean experiment for the next time it fails: open the video in the browser, leave the browser open, and hand it to the app from the extension rather than pasting. If that works, the break was a lost session key, not a Wirecatch change. If it still fails, the extension's `Cookie` header needs to be captured (it is a secret, so log only whether a `KEY` or `RNKEY` cookie is present, not its value) and compared with what the browser holds.

## 11. What I could not determine

- Whether the D136 guard makes the first, cookie-less request more likely to be gated. It does not block a valid session (proved with a fresh cookie through the proxy), but the 10-versus-10 result was confounded by rate. A clean A/B on a fresh egress address would show whether it is an aggravator.
- What cookies the failing attempts actually carried. Downloads 169 to 174 were recorded and their rows removed, and the log does not print the `Cookie` header (correctly, it is a secret).
- Whether the 410 attempts were an expired MP4, a dead HLS manifest, or a captured link from a different network session. The log does not record the URL.
- Whether the browser had a valid `KEY` cookie at 01:11. No persistent PornHub cookies exist on the machine, which is expected for a session cookie but also fits a private window or a cleared store. The extension would have sent one if it could read the store.
- Whether turning Cloudflare WARP off changes the failure to an ISP-level block rather than the PornHub gate. The person reports the same failure both ways, but the two failures could have different causes.
- Whether PornHub changed how long a `KEY` survives or how often it serves the gate. Nothing local can show that.

## Appendix A: versions and environment

| Item | Value |
| --- | --- |
| App | 0.1.0 (`config/product.toml`) |
| Installed app binary | Rebuilt 2026-11-13 13:37, first started 16:11 |
| Browser extension | Unpacked development build, ID `chdgnmpjoipbgfeldicipccjjghmagki`; the native host files still point at the staged dev bridge from 2026-11-09 |
| yt-dlp | 2026.10.26, tag `c91f8de0b`, win_exe build |
| curl_cffi inside yt-dlp | 0.17.0 |
| Deno | 2.9.7 |
| ffmpeg | 9.0.2 full build |
| OS | Windows 11 26100 |
| Network | Cloudflare WARP tunnel present, Tailscale present; the person reports the same failure with WARP off |
| Add-on parts | yt-dlp, ffmpeg, Deno (D20); no PhantomJS |

## Appendix B: the exact yt-dlp invocation

From `crates/media/src/ytdlp/mod.rs:334` (`common_args`), the page read is:

```
yt-dlp --ignore-config --no-plugin-dirs --no-playlist --playlist-items 1 --no-update
       --no-js-runtimes --js-runtimes deno:<addon>\deno\deno.exe
       --ffmpeg-location <addon>\ffmpeg\ffmpeg.exe
       --cache-dir <addon>\yt-dlp
       [--cookies <temp>\cookies-<pid>-<n>.txt]
       [--add-headers User-Agent:<browser user agent>]
       -J -- <page url>
```

The cookies file is a Netscape file bound to the page's host, written with a 24-hour expiry and restricted to the current user (`crates/media/src/ytdlp/cookies.rs`). The proxy, when there is one, is passed through the environment, never the command line (D71.3), and the D136 guard replaces it for a direct run.

## Appendix C: code and issue references

| What | Where |
| --- | --- |
| PornHub extractor and PhantomJS call | `yt_dlp/extractor/pornhub.py`, `_download_webpage_handle` |
| PhantomJS wrapper and cookie copy | `yt_dlp/extractor/openload.py`, `PhantomJSwrapper` |
| Douyu signing script | `yt_dlp/extractor/douyutv.py`, `_calc_sign` |
| iQiyi signing script | `yt_dlp/extractor/iqiyi.py`, line 412 |
| App error wording | `crates/media/src/describe.rs:78` |
| App cookie file | `crates/media/src/ytdlp/cookies.rs:94` |
| App User-Agent override | `crates/media/src/ytdlp/mod.rs:363` |
| App guard proxy | `crates/media/src/ytdlp/guard.rs`, D136 |
| Reading cache | `crates/core/src/manager/page_readings.rs`, D223 |
| Browser cookie capture | `extension/src/lib/cookies.ts`, D44.6 |
| Add-on parts | `crates/addons`, D20 |
| yt-dlp PornHub 410 history | yt-dlp issue #16729, fixed by PR #16794 (2026-06-09) |
| yt-dlp PhantomJS replacement | yt-dlp issue #1354 (open since 2021) |
| yt-dlp PornHub PhantomJS report | yt-dlp issue #14812 (open) |
| Chrome cookie decryption failure | yt-dlp issue #10927 |

## Appendix D: how to re-run the proof

1. Run the app's yt-dlp command against a PornHub page until it prints `PhantomJS not found, Please download it from https://phantomjs.org/download.html`. Add `--write-pages --paths <dir>` to keep the page it was given.
2. Confirm the dump contains `leastFactor` and a `go()` function.
3. Extract the `<script>` content, strip `<!--` and `-->`, and run it in Deno with `document = { cookie: "", location: { reload() {} } }`, then call `go()` and print `document.cookie`.
4. Write `www.pornhub.com FALSE / TRUE <expiry> KEY <value>` into a Netscape cookie file.
5. Re-run the same yt-dlp command with `--cookies <file>`. It should print the video ID and a format, where it previously failed.

The same sequence, with the cookie produced from the first challenge, worked three times in a row in this investigation.

# Cross-module flow: security-bridge-and-pipe-input

Everything arriving through `crates/bridge` and the local pipe, treated as untrusted, followed to the disk, the network, the file name, the headers and started programs.

## Files read

Whole:

```text
crates/server/src/lib.rs
crates/protocol/src/extension.rs
crates/core/src/manager/captures.rs
crates/core/src/manager/arrival.rs
crates/core/src/manager/handovers.rs
crates/core/src/manager/media.rs
crates/core/src/manager/media_run/network.rs
crates/core/src/manager/media_run/page.rs
crates/core/src/manager/media_run/extras.rs
crates/core/src/manager/preferences.rs
crates/core/src/headers.rs
crates/records/src/origin.rs
crates/files/src/zone.rs
crates/media/src/ytdlp/mod.rs
crates/media/src/ytdlp/download.rs
crates/media/src/ytdlp/cookies.rs
crates/sites/src/local_network.rs
crates/shell/src/browser_notices.rs
crates/shell/src/system_notifications.rs
```

In part, the parts this flow passes through:

```text
crates/engine/src/http.rs (headers per origin, redirects)
crates/engine/src/http/client.rs (user agent)
crates/engine/src/naming.rs (sanitize, clean, reserved names)
crates/engine/src/ftp/mod.rs (Target::new)
crates/engine/src/download.rs (which schemes get a guard)
crates/core/src/naming.rs (settled_name, file_name)
crates/core/src/manager/downloads.rs (parse_url, parse_web_url)
crates/core/src/manager/run.rs (engine_request)
crates/settings/src/lib.rs (validate, media defaults)
crates/shell/src/api.rs (is_program, file_to_open)
crates/shell/src/launch.rs (links from a command line)
crates/shell/src/bridge.rs (connection limit)
crates/desktop/src/notices.rs (what a pressed notice does)
crates/protocol/src/desktop.rs (SAFE_EXTENSIONS)
```

Also `contracts/05, 06, 07, 08, 09, 10, 15, 16, 17, 21`, and the D34 and D39 sections of `docs/DECISIONS.md`.

Left out: `crates/bridge/src/relay.rs`, `raw_frame.rs`, `caller.rs` and `shell_parent.rs` were not read, and for them this review relies on contract 06 (the bridge relays bytes unchanged, and no field of a message reaches the app's start: `app.rs` takes no argument from a frame, by search only). `crates/streams` was not read for how segments use headers: this rests on `RequestHeaders` in the engine and on the crate's documentation. Nothing was run against the real app, a real FTP server or a real yt-dlp.

## Findings

### Xsecurity-bridge-and-pipe-input-1 [P2] A website's FTP link reaches the local network, past both refusals and with no guard

- Where: `crates/sites/src/local_network.rs` (`refuses`, `refuses_address`, `address_in`), `crates/core/src/manager/downloads.rs` (`parse_url`), `crates/engine/src/download.rs` (guard only for `http` and `https`), `crates/engine/src/ftp/mod.rs` (`Target::new`)
- Category: wrong implementation
- Failure: a capture chosen by a website (`ChosenBy::Website`) with the link `ftps://%31%32%37.0.0.1/a.zip` or `ftpes://%6c%6f%63%61%6c%68%6f%73%74/a.zip`. Neither refusal sees a local host, so the capture is accepted, and the engine then connects to 127.0.0.1 and logs in, as `anonymous` or with the login in the link, and saves the file. The same holds without any encoding for `ftp://name.example/a.zip` whose name answers public at the check and local at the connection, or answers both public and local addresses. A website can so reach any FTP server, or any port that takes the connection, on the person's network: the rule D34.9 and D39.3 state for its links does not hold for FTP.
- Evidence:
  - `ftps` and `ftpes` are not special schemes to the URL parser, so `host_str()` keeps the percent-encoding. `address_in` then finds no address, and the lookup in `refuses` fails, which counts as "not refused". `refuses_address` before every start has the same blind spot.
  - `Target::new` decodes that same host with `percent_decode_str` and `Host::parse`, so the host the checks read is not the host that is connected to.
  - A scratch program using the same `url` calls, on Windows, printed for `ftps://%31%32%37.0.0.1/a.zip`: `host_str=%31%32%37.0.0.1 parses_as_ip=false lookup=Err(No such host is known) target_host=127.0.0.1`, and for `ftpes://%6c%6f%63%61%6c%68%6f%73%74/a.zip`: `target_host=localhost`. `ftp://%31%32%37.0.0.1/a.zip` is normalised to `127.0.0.1` and is refused correctly.
  - `download_with` makes a guard only for `http` and `https`, and contracts 09 and 21 record that the FTP path applies no `PrivateNetwork` rule. So `Block` means nothing here, and the doc comment of `refuses` ("a name that answers public here and local when the engine connects is still refused by the engine") is false for FTP. D34.9 says "every start of a link takes the rule", and no decision accepts FTP as an exception.
  - The engine half (no rule on the FTP path) is the fault contract 21 cites as 21-1. New here: the percent-encoded host, the false promise of the core check, and that a capture reaches it.
  - Limits on harm: the default question window shows the link first, unless asking is off. The file lands on the person's own disk, so nothing is sent back to the site. `Target::new` refuses line breaks, so no FTP commands can be injected. The whole path was not run end to end.
- Confidence: likely

### Xsecurity-bridge-and-pipe-input-2 [P2] Reading a website's page with yt-dlp escapes the local network rule, before the person answers

- Where: `crates/core/src/manager/media.rs` (`hold_media`, `analyse`, `read_page`), `crates/media/src/ytdlp/mod.rs` (`extract_once`), `crates/core/src/manager/media_run/page.rs` (`page_attempt`)
- Category: wrong implementation
- Failure: a capture with `media.kind = page`, chosen by a website, of `https://public.example/x`, while the add-on is installed. That page answers with a redirect to `http://192.168.1.1/some/action`, or names such an address for yt-dlp to read. yt-dlp sends that GET from the person's computer. The read starts as soon as the capture is held, so this happens before the person sees or answers any question. A later fallback download has the same gap: a format that is not one Wirecatch fetches itself, or a Retry with no format chosen, goes to `ytdlp::download`, and `network_rule` is never applied, so a format address on the local network is fetched and saved.
- Evidence:
  - The only check on this path is `local_network::refuses` in `capture_arrived`, which looks at the first host only. `extract_once` and `download_once` pass yt-dlp no address rule, and yt-dlp has none of the engine's `Guard` or `FilteringResolver`.
  - `fetch_parts` and `fetch_small` apply `network_rule`, while `page_attempt`'s else branch calls `ytdlp::download` without it. The code comment there and in `network.rs` admits this for the fallback download only, and it is not in a decision: D34.5 says every URL a website chose uses `Block`, and D34.9 lists the starts that take the rule without naming the read of a page or the fallback. `read_stream` for the HLS and DASH kinds does run under the rule, so only the page kind differs.
  - Limits on harm: the cookies file is bound to the page's exact host, so sign-ins do not follow. The answer stays on this computer. It takes the extension sending a page capture, and yt-dlp following the redirect was not run here.
- Confidence: likely

Checked along the flow, with no finding:

- **Headers and cookies:** `is_allowed` is applied in both `for_capture` and `to_map`, names are trimmed the same way everywhere, line breaks in values are dropped by `HeaderValue::from_str`, and each redirect hop takes only what its origin may see, including `https` to `http` on one host, which is refused or counts as another origin.
- **yt-dlp arguments:** argument list only, `--` before the page, `http` and `https` pages only (`parse_web_url`), user agent as one argument, cookie lines refused on tab or line break and bound to one host, result only `media.<ext>` inside the work folder.
- **File names:** `suggested_name` and titles go through `sanitize`: folder parts, device names, stream colons, control and bidirectional characters. Engine-like endings are refused or changed.
- **Mark of the Web:** `clean_url` reparses `referrer` and the link and strips control characters, so no line can be added to the stream.
- **Settings patches:** `UpdateBrowserControls` and `UpdateExtensionSettings` reach neither a folder nor a proxy. Queue ids, speed limits, audio quality and language codes are checked by `validate`.
- **Started programs:** `NoticePressed` picks only from buttons the app kept, by place. `Open` asks first for any type not on `SAFE_EXTENSIONS`, and for anything marked executable on Linux. `cx.open_url` is never given `page_url` or any other field from a message.
- **Links:** `OpenLinks` only fills the Add dialog, at most 20 links, and the person still acts.
- **Text in system notifications:** the toast XML escapes site text, quotes included.
- **Limits:** frames at 1 MiB with oversized ones skipped unstored, 64 requests per connection, 64 connections, 128-byte handover ids, 1000 remembered handovers.

## Suggestions

None.

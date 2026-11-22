# Browser routing for media page reads

Proposed design. Some video sites refuse yt-dlp because it is not a browser: they close the TLS connection after the ClientHello, answer `410 Gone`, or serve a JavaScript check (`MEDIA-FAILURE-REPORT.md`). D281 covers the JavaScript check. A check on the client's identity (TLS fingerprint, Client Hints, User-Agent, cookies) can only be passed by the browser itself. Browser routing has the browser make yt-dlp's page reads, through the extension. Video data keeps going straight through the engine.

## Request flow

1. The manager starts a page read (`crates/media/src/ytdlp/info.rs`, `extract_once`) for a download that came from a connected browser whose extension offers page reads. It starts a **relay** for the run in place of the guard (D136): a proxy on `127.0.0.1`, on a free port, with a per-run certificate authority and a per-run proxy password.
2. yt-dlp is given the relay as its proxy in the environment (as `proxy_environment` does now, password included), `SSL_CERT_FILE` set to the run's CA certificate in the run's private folder (D281), and `--compat-options no-certifi`.
3. yt-dlp sends `CONNECT host:443`. The relay checks the password and refuses a local-network destination (the D136 rule). A host outside the page's site (its registrable domain, `crates/sites`) is tunnelled as the guard tunnels it today, untouched. For a host of the page's site, the relay answers `200` and completes the TLS handshake as that host with a leaf certificate made on the spot from the CA (ALPN `http/1.1`).
4. The relay reads each decrypted HTTP/1.1 request and hands it to the manager, which sends `Event::PageReadRequested` to that browser's connection only. The bridge relays it as it relays every frame.
5. The extension's background makes the request with `fetch(url, { method, headers, body, credentials: "include", redirect: "follow", cache: "no-store" })`. The browser's own network stack sends it: its TLS, HTTP/2 or 3, Client Hints, User-Agent and cookie jar.
6. The extension answers with the status, final address and headers, and the body in parts (`Call::PageReadPart`, then `Call::PageReadAnswered`), each under `MAX_FRAME_LEN`.
7. The relay writes the answer back to yt-dlp as HTTP/1.1: the body as decoded by the browser, without `Content-Encoding`, `Transfer-Encoding` or the browser's `Content-Length`, with its own `Content-Length`.
8. When yt-dlp exits, the relay stops; its CA, leaf keys and password are gone with it.

What the browser makes: the requests of a page-read run to the page's own site (the page and its JSON APIs). What it does not make: requests to other hosts such as a CDN's manifests, which the spike found answered `412` when the browser made them; the transfer of video data; and yt-dlp runs that download (`download.rs`), which keep the guard.

### What the browser does differently from yt-dlp

- **Headers.** `fetch()` sets `Cookie`, `User-Agent`, `Accept-Encoding`, `Host`, `Origin`, `Referer` and every `Sec-*` header itself and ignores yt-dlp's. The extension drops them before calling `fetch()`. Setting `Referer` or `Origin` would need `declarativeNetRequest`, which the extension does not ask for; no extractor in the spike needed them.
- **Cookies.** The browser's jar replaces yt-dlp's cookie file. Cookies an extractor sets itself (PornHub's `age_verified`, `platform`) do not reach the site, and `Set-Cookie` in an answer is not readable by `fetch()`, so yt-dlp does not see new cookies; the browser stores them, as a visit would. If an extractor turns out to need either, the extension can read the cookies for the final address after the fetch (`cookies.getAll`, permission held) and the relay can return them as `Set-Cookie`.
- **Redirects.** `fetch()` follows them; an extension cannot read `Location` from `redirect: "manual"`. The relay answers yt-dlp with the final page at the address it asked for. Where an extractor reads the final address, the relay answers `302` to the final address and keeps the fetched answer for the next request to it.
- **Private windows.** An extension's `fetch()` uses the regular profile's cookies in Chromium. A download from a private session (D280) is never routed through the browser.
- **Request context.** The request leaves as the extension's (`Sec-Fetch-Site: none`, `Sec-Fetch-Mode: cors`, `Sec-Fetch-Dest: empty`), not as a top-level navigation. A site that checks those headers can tell it apart from a page load; the spike site did not.

## Security model

- **Nothing in the system trust store.** The CA is trusted only by the one yt-dlp process, through its environment. Its key stays in the app's memory. Its certificate is valid for one hour, `pathLenConstraint=0`, and written only to the run's private folder, which is deleted after the run (D281).
- **One run, one relay.** The relay listens on loopback on a free port and accepts a `CONNECT` only with the run's random proxy password, which reaches yt-dlp in its environment, never on the command line. Without the password, any local program could use the person's browser, signed in everywhere, as an open proxy.
- **The local network stays off.** The relay applies the D136 check to every destination before the browser sees it: the browser's own Private Network Access rules do not apply to an extension's `fetch()`.
- **No `--no-check-certificates`.** With it, a connection that ever left yt-dlp without the relay would go to the internet unchecked. With the per-run CA, curl_cffi's trusted roots are replaced by the CA (measured below), so such a connection fails instead; Python's handlers keep the system roots and check it normally.
- **Bounded answers.** A page read answer is capped (proposed 32 MiB, parts of 512 KiB) and times out (proposed 60 s); the extension stops `fetch()` with an `AbortController` on `Event::PageReadCancelled` or a dropped connection.
- **The extension reads only what the relay asks.** The event names a method, address, headers and body; the extension sends no other cookies or browser state back.

## When the browser is not there

The manager routes a run through the browser only when the browser the download came from is connected and its extension said it offers page reads in `hello` (a new protocol version, D15). Otherwise the run reads directly, through the guard, as today. If the browser goes away during a run, the relay answers the waiting requests with `502`, the run fails, and the next attempt reads directly. A person who closes the browser loses nothing they have today.

## Protocol (`crates/protocol`)

```rust
/// One HTTP request of a page read, made by the browser.
pub struct PageRead {
    pub id: PageReadId,               // per connection, from the app
    pub method: PageReadMethod,       // Get, Head, Post
    pub url: Secret,
    pub headers: Vec<Header>,         // yt-dlp's, minus the ones fetch() sets itself
    pub body: Option<String>,         // base64, at most MAX_PAGE_READ_BODY
}

Event::PageReadRequested(PageRead)    // to one connection, not every client
Event::PageReadCancelled(PageReadId)

Call::PageReadPart { id: PageReadId, bytes: String }               // base64, at most MAX_PAGE_READ_PART
Call::PageReadAnswered { id: PageReadId, status: u16, url: Secret, headers: Vec<Header> }
Call::PageReadFailed { id: PageReadId, reason: PageReadFailure }   // network, aborted, too_large

pub const MAX_PAGE_READ_PART: usize = 512 * 1024;  // under MAX_FRAME_LEN once base64 and JSON
pub const MAX_PAGE_READ_LEN: usize = 32 * 1024 * 1024;
pub const MAX_PAGE_READ_BODY: usize = 256 * 1024;
pub const PAGE_READ_TIMEOUT_MS: u64 = 60_000;
```

Every event today goes to every client; this one goes to one connection, so the server needs a targeted send. `Hello` gains the capability. The extension already holds the permissions it needs (`<all_urls>` host permissions and `cookies`), so its manifest does not change.

## Spike results

Run on 2026-11-19 on Windows 11 with the add-on's yt-dlp 2026.10.26 (curl_cffi, Python 3.14.10) and Edge 157.0.4341.60. The relay was a Python stand-in for the design above; the browser side was a test extension calling `fetch()` from an extension page. The production extension would call it from its background service worker, which the spike did not test: Edge stopped an idle worker held open only by a WebSocket, and the real extension's native messaging port must be shown to keep it running.

### How yt-dlp trusts the relay

yt-dlp read a page through the relay with each setting. The default handler is `requests`; `--impersonate chrome` makes it curl_cffi, which PornHub's extractor always uses.

| Setting                                                    | `requests`                                     | curl_cffi        |
| ---------------------------------------------------------- | ---------------------------------------------- | ---------------- |
| nothing                                                    | refused: self-signed certificate               | refused: curl 60 |
| `--no-check-certificates`                                  | reads                                          | reads            |
| `SSL_CERT_FILE=<ca.pem>`                                   | refused: yt-dlp loads certifi's bundle instead | reads            |
| `REQUESTS_CA_BUNDLE=<ca.pem>`                              | refused                                        | reads            |
| `CURL_CA_BUNDLE=<ca.pem>`                                  | refused                                        | reads            |
| `SSL_CERT_FILE=<ca.pem>` and `--compat-options no-certifi` | reads                                          | reads            |

The same per-run CA, without the relay, against `https://example.com/`: curl_cffi refused it (curl 60, its roots replaced by the CA), and `requests` with `no-certifi` read it (the system roots stay). So `SSL_CERT_FILE` with `--compat-options no-certifi` is the one setting both handlers honour, and a connection that leaves the relay fails or is checked normally, never unchecked.

### Direct reads against reads through the browser

PornHub, eight video pages taken in turn, with yt-dlp run as `--ignore-config --no-cache-dir --no-playlist -O "%(height)s"`. The interleaved run alternates the two routes for 20 rounds, 3 s apart, from the same address in the same minutes. The test browser profile was new: no PornHub cookies, never used by a person.

| Route                                | Reads | Read          | TLS reset (curl 35) | `410` | Time, read                 | Time, failed |
| ------------------------------------ | ----- | ------------- | ------------------- | ----- | -------------------------- | ------------ |
| Direct, interleaved run              | 20    | 8             | 12                  | 0     | 4.2 to 23.8 s, mean 11.7 s | 20.8 s each  |
| Through the browser, interleaved run | 20    | 20, all 1080p | 0                   | 0     | 2.8 to 11.6 s, mean 3.7 s  | none         |
| Direct, two earlier runs             | 21    | 6             | 15                  | 0     |                            |              |

- Across all 41 direct reads, 14 read (34%) and 27 were reset; each reset took about 21 s to surface. All 20 reads through the browser succeeded.
- The browser fetched each 1.6 MB page in 0.68 to 3.9 s (median 0.72 s).
- No `410` came in this window, on either route, so the spike shows nothing about the `410` answer.
- 84 of 163 requests through the browser were answered `412 Precondition Failed`: all HLS manifests on `hv-h.phncdn.com`, the CDN rather than the page's site. yt-dlp found 1080p from the other formats each time. The cause is unproven; `fetch()` cannot send yt-dlp's `Referer` and sends the extension's `Origin`. Sending only the page's site through the browser leaves those reads as they are today.

The spike's relay, extension, bench script and raw results are kept outside the repository with the session notes.

## Recommendation

Build it. Through the browser, 20 of 20 page reads succeeded against 8 of 20 direct reads in the same minutes, and a read took 3.7 s on average against 21 s for a reset direct read.

1. Use a per-run CA given to yt-dlp as `SSL_CERT_FILE` with `--compat-options no-certifi`, and never `--no-check-certificates`.
2. Send only the requests for the page's own site through the browser. Other hosts keep the guard's tunnel.
3. When the download came from a connected browser that offers page reads, read the page through it first. Otherwise read directly, as today. Do not try direct first and then the browser: a refused direct read costs about 21 s and adds to the site's count against the address.
4. Before the decision record, show that the extension's background service worker stays running for a whole run with only the native messaging port open, and try Firefox, whose `fetch()` from an extension was not measured.

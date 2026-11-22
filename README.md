<p align="center">
  <img src="crates/desktop/assets/icons/icon.svg" width="96" height="96" alt="Wirecatch logo" />
</p>

<h1 align="center">Wirecatch</h1>

<p align="center">
  <strong>A free, open-source download manager for Windows and Linux.</strong><br />
  Multi-connection downloads, browser integration, HLS and DASH streams, and video sites through an optional add-on.
</p>

<p align="center">
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-GPL--3.0-blue" alt="License: GPL-3.0" /></a>
  <img src="https://img.shields.io/badge/platform-Windows%20%7C%20Linux-informational" alt="Platforms: Windows and Linux" />
  <img src="https://img.shields.io/badge/built%20with-Rust-orange" alt="Built with Rust" />
  <a href="https://github.com/ehsan18t/wirecatch-oss-dm/actions/workflows/ci.yml"><img src="https://github.com/ehsan18t/wirecatch-oss-dm/actions/workflows/ci.yml/badge.svg" alt="CI status" /></a>
</p>

<p align="center">
  <a href="#highlights">Highlights</a> ·
  <a href="#features">Features</a> ·
  <a href="#how-it-compares">How it compares</a> ·
  <a href="#command-line">Command line</a> ·
  <a href="#privacy-and-security">Privacy</a> ·
  <a href="#get-wirecatch">Get Wirecatch</a>
</p>

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/images/main-window-dark.png" />
  <img src="docs/images/main-window-light.png" alt="Wirecatch main window showing active, scheduled, paused, and completed downloads, with queues in the sidebar." />
</picture>

## Highlights

- **Verified resume.** Interrupted downloads resume from saved progress after a restart or crash when the server supports range requests. If the file on the server has changed, the download starts over instead of combining two versions.
- **Expired link refresh.** Replace an expired link by hand, or capture it again from the browser, and the download continues from its existing progress, including when the new link is a stream instead of a file.
- **Rule-based queues.** New downloads are routed to queues by site, file name, file type, or protocol. Each queue can have its own schedule, speed limit, folder, and proxy.
- **Streams and video sites.** HLS and DASH downloads are built in. The optional Media add-on merges separate audio and video tracks, adds YouTube, Vimeo, TikTok, and other sites, and provides format conversion, subtitles, and chapters.
- **Errors with a fix.** A failed download states the cause and offers the matching action, such as refreshing the link or choosing another folder.
- **No accounts or telemetry.** The browser connects to the app without a network port, and downloads from private windows leave no history, logs, or partial files behind.

## Features

<details>
<summary><strong>Downloads</strong>: multiple connections, resume, link refresh, FTP and FTPS</summary>

- 8 connections per file by default, up to 32 in the app and 64 on the command line, when the server supports range requests. Remaining parts are reassigned as connections finish, so a slow connection does not delay completion.
- Downloads and streams from the same host share one backoff: they honour `Retry-After`, reduce connections when the server limits them, and show when they are waiting or reconnecting.
- Partial files stay hidden until the download completes.
- Downloads can move to another folder while running, without overwriting existing files.
- Each download keeps an activity log in **Details**: errors, retries, pauses, refreshed links, and moves.
- The download database is repaired automatically at startup if it is damaged.
- Protocols: HTTP, HTTPS, FTP, and FTPS (`ftps://` and `ftpes://`).

</details>

<details>
<summary><strong>Browser extension</strong>: Chrome, Edge, Brave, Vivaldi, Opera, Arc, and Firefox</summary>

- Takes over browser downloads with the browser's cookies and referrer, so downloads that require a sign-in work. A handed-over download is never lost or downloaded twice.
- **Download with Wirecatch** in the context menu, a download button on videos, and a keyboard shortcut for the page's media.
- The toolbar popup lists the streams and video files detected on the current tab.
- Capture filters: excluded sites, allowed file types, minimum file size, and default queue.
- Files the browser marks as dangerous stay in the browser, and the Windows Mark of the Web is preserved.
- A dialog for each download to set the file name, folder, queue, or video quality and follow progress. It can be turned off in favour of a notification, which opens the download in the app when clicked.
- Settings are managed in the desktop app and apply to every connected browser.

</details>

<details>
<summary><strong>Video, audio, and streams</strong>: HLS and DASH, video sites, conversion, and subtitles</summary>

- HLS (including encrypted streams) and DASH, with segments downloaded in parallel and joined without re-encoding. Streams with separate audio and video tracks are merged by the Media add-on.
- The optional Media add-on (`yt-dlp`, `ffmpeg`, and `Deno`) installs in one click and supports YouTube, Vimeo, TikTok, and custom sites.
- Paste a video page's link into the app to list its available formats.
- Sites that require a real browser are read through the connected browser, with site scripts run in Deno. The media itself still downloads over parallel connections.
- Keep the original format, or convert video to MP4, MKV, or WebM and audio to MP3, M4A, Opus, FLAC, or WAV.
- Embedded or separate (`.srt`) subtitles, optional auto-generated captions, chapters, and audio cover art.
- Hardware-accelerated encoding for conversions. The original file is kept if a conversion fails.
- A preferred resolution and format, with fallback to the nearest lower or higher quality.

</details>

<details>
<summary><strong>Queues, rules, and schedules</strong>: per-queue limits, folders, proxies, and routing rules</summary>

- Each queue can override the app defaults for simultaneous downloads, connections per download, speed limit, folder, and proxy, and sets its own completion notifications.
- Routing rules match by site, file name (wildcards or regular expressions), file type, or protocol. **Test a link** shows which queue a link would join and why.
- Schedules with a start time, an optional stop time, and days of the week, plus **Start now** and per-queue pause.
- Drag downloads to reorder them or move them between queues. Waiting downloads show why they have not started.

<table>
  <tr>
    <td><img src="docs/images/queue-rules.png" alt="Queue routing rules sending videos to the Videos queue, with a link tested against them." /></td>
    <td><img src="docs/images/queue-schedule.png" alt="Queue schedule set to run daily from 1:00 AM to 7:00 AM." /></td>
  </tr>
</table>

</details>

<details>
<summary><strong>Proxies and network</strong>: proxy profiles per queue, local network protection</summary>

- System proxy, direct connection, or named HTTP, HTTPS, SOCKS4, SOCKS4a, SOCKS5, and SOCKS5h profiles, for the whole app or per queue.
- Copy the system proxy settings, test a profile, and set bypass rules for the whole app or per queue.
- Links on public web pages cannot reach addresses on the local network, such as a router or NAS, unless the site is allowed in Settings.

</details>

<details>
<summary><strong>Desktop app</strong>: native Rust app, portable settings, signed updates</summary>

- Native Rust application with a GPU-rendered interface (GPUI). No Electron and no embedded browser.
- Export settings, queues, schedules, folder rules, and proxy profiles to one file, and import it on Windows or Linux with a preview before anything changes.
- Updates install from within the app and are verified against a signed manifest.

</details>

## How it compares

|                                                   | Wirecatch |       IDM       | Free Download Manager | AB Download Manager |  JDownloader 2   |   Motrix    |
| ------------------------------------------------- | :-------: | :-------------: | :-------------------: | :-----------------: | :--------------: | :---------: |
| Free                                              |     ✓     |      Paid       |           ✓           |          ✓          |        ✓         |      ✓      |
| Open source                                       |     ✓     |        ✗        |           ✗           |          ✓          |      Partly      |      ✓      |
| Windows and Linux                                 |     ✓     |  Windows only   |           ✓           |          ✓          |        ✓         |      ✓      |
| Native app (no Electron or JVM)                   | ✓ (Rust)  |     ✓ (C++)     |        ✓ (Qt)         |         JVM         |       JVM        |  Electron   |
| No ads, bundled offers, or analytics              |     ✓     |        ✓        |       Analytics       |          ✓          | Installer offers |      ✓      |
| Browser connection without an open port           |     ✓     |        ✓        |           ✓           |          ✗          |   Vendor cloud   |      ✓      |
| Private-window downloads leave no history or logs |     ✓     |        ✗        |           ✗           |          ✗          |        ✗         |      ✗      |
| HLS and DASH built in                             |     ✓     |        ✓        |        Partly         |       Partly        |     Plugins      |   Plugin    |
| Video sites (YouTube, Vimeo, TikTok)              |  Add-on   |        ✓        |        Add-on         |          ✗          |     Plugins      |   Plugin    |
| Media conversion and subtitles                    |     ✓     |        ✗        |     Convert only      |          ✗          | Separate `.srt`  |      ✗      |
| Queues and schedules                              |     ✓     |        ✓        |       Scheduler       |          ✓          |    Scheduler     |  Scheduler  |
| Rule-based queue routing                          |     ✓     |        ✗        |           ✗           |          ✗          |    Packagizer    |      ✗      |
| Proxy profiles and per-queue proxies              |     ✓     |   Global only   |      Global only      |    Global / host    |    Proxy list    | Global only |
| Refresh an expired link, keeping progress         |     ✓     |        ✓        |        Manual         |       Manual        |      Manual      |   Manual    |
| Export and import settings and queues             |     ✓     | Registry / list |           ✓           |          ✗          |        ✓         |      ✗      |
| Standalone command-line downloader                |     ✓     |   Flags only    |           ✗           |      CLI / IPC      |        ✗         |     CLI     |

<sub>As of November 2026, based on each project's official website, documentation, store listing, or repository (Motrix version 2). Spotted an inaccuracy? Please open an issue.</sub>

Wirecatch does not support BitTorrent, live streams, or DRM-protected media. For BitTorrent, consider Free Download Manager or Motrix.

## Command line

The `wirecatch` command uses the same download engine as the desktop app. `Ctrl+C` pauses cleanly, and running the same command again resumes when the server supports it.

```bash
wirecatch https://example.com/file.iso
wirecatch -o ~/Downloads -c 16 https://example.com/file.iso
wirecatch -H "Cookie: session=abc" -n release.iso https://example.com/download
wirecatch --proxy socks5h://proxy.example:1080 https://example.com/file.iso
```

It supports HTTP, HTTPS, FTP, and FTPS, and sends custom headers only to the link's own site, never to a site it redirects to. Builds with the `http3` Cargo feature (off by default) add an experimental `--http3` option. The download engine is also available as a standalone Rust crate: see [crates/engine/README.md](crates/engine/README.md).

## Privacy and security

- No accounts, analytics, telemetry, ads, or bundled offers.
- The browser extension connects through Native Messaging and a per-user named pipe (Windows) or Unix domain socket (Linux), with no network port. On Windows, the app does not start with **Run as administrator**.
- Cookies and sign-in headers are sent only to the site they belong to, never across a redirect to another site. Saved sign-ins for unfinished downloads expire after 7 days.
- Logs redact passwords, cookies, and tokens.
- Downloads from private windows are kept in memory only, never written to the database or logs, and their partial files are deleted when the last private window closes.
- Media add-on programs are verified against SHA-256 checksums. `yt-dlp` updates require a valid signature and wait 48 hours before installing, and `yt-dlp` reaches the internet only through a guard that blocks local network addresses.

See [PRIVACY.md](PRIVACY.md) for what is stored locally and which network requests are made. To report a vulnerability, follow [SECURITY.md](SECURITY.md) rather than opening a public issue.

## Get Wirecatch

Download the latest release from the [Releases page](https://github.com/ehsan18t/wirecatch-oss-dm/releases):

- **Windows** (x64, ARM64): `.exe`
- **Linux** (x64, ARM64): `.deb`, `.rpm`, or `.AppImage`

Install the browser extension from the Chrome Web Store or Firefox Add-ons.

To build from source, see [CONTRIBUTING.md](CONTRIBUTING.md). To build the installers, see [crates/desktop/PACKAGING.md](crates/desktop/PACKAGING.md).

## Contributing

Bug reports, feature requests, and pull requests are welcome. See [CONTRIBUTING.md](CONTRIBUTING.md) for setup, checks, and how to submit a change.

## License

Wirecatch is licensed under the [GNU General Public License v3.0 only](LICENSE). The optional Media add-on components (`yt-dlp`, `ffmpeg`, and `Deno`) are separate projects distributed under their own licenses.

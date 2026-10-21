# Wirecatch

**The fast, free download manager you can trust.** Wirecatch speeds up your downloads, catches them straight from your browser, grabs video and streams, and keeps queues, schedules and speed limits in plain sight. Open source, private, and built for Windows and Linux.

<!-- Hero screenshot: the main window with a few downloads and a queue, a download window in front. -->

- **Fast:** splits every file across up to 64 connections and keeps them all busy until the last byte.
- **Never loses a download:** pause, crash or power cut, it picks up exactly where it stopped. A download caught from the browser ends up in one place, never two, never none.
- **Video included:** HLS and DASH streams built in, and sites like YouTube through an optional one-click add-on.
- **Free for real:** no ads, no bundled offers, no paid tier, no tracking. GPL-3.0.

> Wirecatch is in early development (0.1.0). Installers and store listings are on the way.

## Why Wirecatch

| | Wirecatch | IDM | Free Download Manager | AB Download Manager | JDownloader 2 | Motrix |
|---|:---:|:---:|:---:|:---:|:---:|:---:|
| Free | ✓ | Paid | ✓ | ✓ | ✓ | ✓ |
| Open source | ✓ | ✗ | ✗ | ✓ | Partly | ✓ |
| Windows and Linux | ✓ | Windows only | ✓ | ✓ | ✓ | ✓ |
| No ads, bundled offers or analytics | ✓ | ✓ | Analytics | ✓ | Installer offers | ? |
| Browser link with no open network port | ✓ | ✓ | ✓ | ✗ | Vendor cloud | ✓ |
| HLS and DASH streams built in | ✓ | ✓ | Partly | Partly | Plugins | Plugin |
| Video sites like YouTube | ✓ | ✓ | Add-on | ✗ | Plugins | Plugin |
| Queues and schedules | ✓ | ✓ | Scheduler | ✓ | ? | ? |
| Proxy settings | ✓ | ✓ | ✓ | ✓ | ✓ | ? |

<sub>As of October 2026, from each project's own website, store listing or repository. "?" means we could not confirm it. Motrix refers to its version 2. Spotted a mistake? Please open an issue.</sub>

What sets Wirecatch apart:

- **Correct when things go wrong.** Resume survives a crash or a power cut, and if the file changed on the server, Wirecatch starts over cleanly instead of stitching together a broken file.
- **Nothing else can reach it.** The browser talks to Wirecatch through a private channel only your user account can open. There is no network port for other programs or other computers to find.
- **Your cookies stay where they belong.** Sign-ins and cookies go only to the site they came from, never to a site the link redirects to.
- **Safer media.** The video add-on's programs are verified before they run, and their updates are checked against a signature.
- **Honest about what it downloads.** Live streams and DRM-protected video are not downloaded.

Need torrents? Wirecatch doesn't do them. Free Download Manager and Motrix do.

## Features

### Downloads

- Multi-connection downloads that rebalance as parts finish, so a slow tail never holds you up.
- Pause and resume anything the server allows, even after a restart.
- Refresh an expired link and keep everything already downloaded.
- Automatic retries that respect what the server asks for.
- HTTP, HTTPS, FTP and FTPS, with HTTP/3 as an option.
- Proxy support: use your system's proxy, or set an HTTP or SOCKS proxy of your own.
- Never overwrites a file: a free name like `file (1).zip` is used instead.
- Files sorted into folders by type: videos, music, archives and more.

### Browser

- Works with Chrome, Edge, Brave, Vivaldi, Opera, Arc, Firefox and their relatives.
- Takes over your browser's downloads automatically, with your cookies, so downloads behind a login work.
- Right-click any link, image, video or audio and choose **Download with Wirecatch**.
- A download button over videos on the page.
- Installers and other programs stay with the browser by default, so its own safety checks still apply.
- One set of settings for every browser and the app.

### A window for each download

- A small window opens for every download from the browser: check the name, folder and queue, then **Download now** or **Download later**.
- Pick the video quality right there.
- Watch progress, speed and time left, then **Open** or **Show in folder** when it's done.
- Prefer no windows? Downloads can start straight away with a quiet notice instead.

### Video and audio

- HLS (including encrypted) and DASH streams, downloaded in parallel and joined without re-encoding.
- Sites like YouTube through the optional Media add-on, installed in one click the first time you need it.
- Save as MP4, MKV or WebM, or audio as MP3, M4A, Opus, FLAC or WAV, with subtitles, chapters and cover art.
- Hardware encoding when a conversion is needed, and a heads-up before it happens.

### Queues and schedules

- Queues in the sidebar, each with its own schedule, speed limit and number of downloads at once.
- Drag downloads between queues.
- Every waiting download tells you why it is waiting.

### The app

- Paste a link anywhere in the window to add it.
- Command palette (Ctrl+Shift+P) and searchable settings.
- Errors that explain themselves and offer the fix as a button.
- Light, dark or system theme, and a choice of accent colours.
- Keeps downloading from the tray, starts with your computer if you like, and tells you when a download finishes.
- Fully usable with the keyboard, and smooth with thousands of downloads.
- Export your settings and queues to a file and import them on another computer, even from Windows to Linux.

### Command line

The `wirecatch` command downloads a link from a terminal with the same engine:

```bash
wirecatch https://example.com/file.iso
wirecatch -o ~/Downloads -c 16 https://example.com/file.iso
```

## Privacy

- No telemetry, no analytics, no account.
- Your downloads and settings stay on your computer, readable only by you.
- Logs never contain passwords, cookies or tokens.
- Saved sign-ins for unfinished downloads are forgotten after a week.

## Installation

Installers for Windows (`.exe`) and Linux (`.deb`, `.rpm` and AppImage) will be on the [releases page](https://github.com/ehsan18t/wirecatch-oss-dm/releases). The browser extension will be in the Chrome Web Store and on Firefox Add-ons.

Want to try it today? [Build it from source](CONTRIBUTING.md).

## Security

Found a security problem? Please report it privately, as described in [SECURITY.md](SECURITY.md), never in a public issue.

## License

Wirecatch is free software under the [GNU General Public License v3.0 only](LICENSE). The Media add-on's programs (yt-dlp, ffmpeg and Deno) come from their own projects under their own licenses.

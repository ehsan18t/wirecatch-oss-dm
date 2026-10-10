# Roadmap

## Phases

1. **Engine and CLI.** Segmented HTTP(S) downloads, dynamic splitting, resume, retries. Benchmarked locally. (current)
2. **Background Engine, Bridge and Extension.** Engine runs as a background process with SQLite state; the Bridge (native messaging host) and the WXT Extension catch browser downloads in Chromium and Firefox; settings sync (D5).
3. **Desktop UI.** Tauri 2 window: download list, queues in the sidebar, schedules, speed limits, command palette, settings import and export (D6).
4. **Streams.** HLS and DASH detection in the Extension, parallel segment download in the Engine, ffmpeg muxing (fetched on demand).
5. **Media Add-on.** yt-dlp Extractor Add-on with one-click install and quiet updates (D2).
6. **Remaining v1 protocols.** FTP/FTPS and HTTP/3 (D10).

## Phase 1 acceptance checklist

Each item is observable from the CLI or the test suite.

1. `wirecatch <url>` downloads a file using several parallel connections, and the result is byte-identical to the source.
2. When a server does not support byte ranges, the download falls back to one connection and still completes correctly.
3. When a connection finishes its part, it takes over half of the largest remaining part (dynamic splitting), so no single slow tail holds up the end.
4. Interrupting a download (Ctrl+C or a killed process) and running the same command again resumes from where it stopped instead of starting over.
5. If the file changed on the server between runs (different size, ETag or Last-Modified), the download restarts cleanly instead of producing a corrupt file.
6. A dropped connection or a server error (5xx, 429, timeout) is retried with backoff, and the download still completes.
7. While downloading, data goes to `<name>.wcpart` next to a `<name>.wcstate` resume file; on success it is renamed to `<name>` and the state file is removed.
8. The file name comes from `Content-Disposition`, then the URL, and is made safe for Windows and Linux. An existing file is never overwritten; a free name like `file (1).zip` is used instead.
9. A local benchmark shows the speedup of several connections over one against a server that limits speed per connection.
10. The disk space for the file is reserved up front without writing it twice (sparse file on NTFS, `fallocate` on Linux).

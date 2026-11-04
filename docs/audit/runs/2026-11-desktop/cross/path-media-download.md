# Cross-module flow: path-media-download

A video or audio download from capture and the quality choice through yt-dlp or the stream download, joining and conversion, to the final file. Live checks used the installed add-on's yt-dlp 2026.10.26 and ffmpeg 9.0.2 against a local server; no product code changed.

## Files read

Whole, without their test modules where a `#[cfg(test)]` block ends the file:

crates/core/src/manager/media.rs
crates/core/src/manager/media_run/mod.rs
crates/core/src/manager/media_run/page.rs
crates/core/src/manager/media_run/save.rs
crates/core/src/manager/media_run/placement.rs
crates/core/src/manager/media_run/workdir.rs
crates/core/src/manager/media_run/extras.rs
crates/core/src/manager/media_run/network.rs
crates/records/src/media/mod.rs
crates/records/src/media/output.rs
crates/media/src/choice.rs
crates/media/src/convert.rs
crates/media/src/describe.rs
crates/media/src/merge.rs
crates/media/src/output/arguments.rs
crates/media/src/output/variants.rs
crates/media/src/ytdlp/mod.rs
crates/media/src/ytdlp/download.rs
crates/media/src/ytdlp/info.rs
crates/streams/src/ffmpeg.rs
crates/streams/src/download/mod.rs
crates/streams/src/download/join.rs

In part: `crates/media/src/output/subtitles.rs` (to `iso639_2`), `crates/core/src/manager/file_exists.rs` (answers, `replace_at`), `crates/core/src/manager/run.rs` (`launch`, `RunGuard`, how a media run ends), `crates/core/src/manager/downloads.rs` (`refresh_link`), `crates/core/src/manager/mod.rs` (`stop_run`), `crates/core/src/manager/state.rs` (`partial_paths`), `crates/desktop/src/download_windows/media_form.rs` (`answer`), `crates/streams/src/model.rs`, `crates/streams/src/extension.rs`.

Left out to stay within the size limit, relying on their contracts: `crates/streams/src/hls.rs`, `dash/`, `plan.rs`, `select.rs`, `workdir.rs`, `download/fetch.rs`, `crates/media/src/ffmpeg.rs`, `output/encoders.rs`, `output/metadata.rs`, `ytdlp/cookies.rs`, the rest of `media_form.rs`, `media_output.rs`, `added_pages.rs` and `captures.rs`.

## Findings

### Xpath-media-download-1 [P1] Sources the ffmpeg gate refuses are still offered for joining and conversion, and fail after the whole download

- Where: `crates/media/src/choice.rs` `from_page`, `crates/media/src/output/variants.rs` `audio_variants` and `video_variants`, `crates/streams/src/ffmpeg.rs` `InputFormat::detect`, `crates/media/src/describe.rs` `ffmpeg_common`
- Category: broken feature
- Failure: a page offers audio as Ogg, FLAC or WAV, or a video as FLV or AVI. The person picks "Save as MP3" (or anything else that runs ffmpeg, or a video paired with such audio). The whole file downloads, then every run ends with "not a real video or audio file" and a Refresh link button. The file is good, no link will change it (Xpath-media-download-6), and Keep the original is not offered, though it would work for a single file.
- Evidence: `extension::MEDIA` lists `ogg`, `flac`, `wav` and `flv`, and `safe_ext` passes them, so the offer layer treats them as ordinary. `InputFormat` knows only MPEG-TS, MP4, Matroska, WebM, AAC, MP3, AC-3 and E-AC-3: `OggS` and `fLaC` heads (checked on real files) and RIFF or `FLV` heads match no branch of `from_head`, so `detect` returns `None`. `merge::input_format` turns that into `FfmpegError::UnknownFormat`. `save::inputs_for` returns it, `not_converted` wraps it, and `describe::conversion` calls `ffmpeg_common` first, which answers `unknown_format` with `FixAction::RefreshLink` instead of `KeepOriginal`. The whitelist itself is deliberate (D21.3); nothing tells the choice layer about it. Only `from_page` could check it, by format `ext`, before offering. Original with nothing added runs no ffmpeg for a single file and works.
- Confidence: certain

### Xpath-media-download-2 [P1] A format that needs cookies always fails when the engine fetches it, though yt-dlp could download it

- Where: `crates/media/src/ytdlp/info.rs` `Format`, `crates/core/src/manager/media_run/page.rs` `part_options`, `page_attempt`, `run_page`
- Category: wrong implementation
- Failure: a site whose media addresses need the session's cookies (signed cookies on a CDN, a signed-in page, a cookie set during extraction). yt-dlp reads the page and lists plain `https` formats, so `ourselves` is true and the engine fetches them with no cookie. The server answers 401 or 403, each refresh gets the same result, and the row ends with "kept refusing". yt-dlp is never tried, although it holds those cookies and would succeed.
- Evidence: `yt-dlp -J --cookies` against a local server gave each format `"cookies": "sid=abc123; Domain=127.0.0.1; Path=/"` and `http_headers` without `Cookie` (User-Agent, Accept, Accept-Language, Sec-Fetch-Mode). `Format` has no `cookies` field, and `part_options` sets `options.headers` from `http_headers` only. The comment on `a_format_never_prints_its_header_values`, "yt-dlp hands each format the page's cookies in its headers", does not hold for the pinned version, and the glossary in `docs/DECISIONS.md` ("direct URLs plus required headers") assumes the same. `run_page` ends with `refused_after_refreshing`; only a format missing or of `Transport::YtDlp` goes to `ytdlp::download`. The page's cookies staying off the engine's request is stated as intent in `part_options`, but the format's own scoped cookies are what yt-dlp itself would send.
- Confidence: likely (the code path and yt-dlp's output are certain; which sites need it was not measured)

### Xpath-media-download-3 [P2] A page yt-dlp describes at the top level loses its protocol and headers

- Where: `crates/media/src/ytdlp/info.rs` `Info`, `Info::all_formats`, `Format::transport`
- Category: bug
- Failure: an extractor that returns one `url` and no `formats`. If it is an HLS address, it counts as `Transport::File`: the engine saves the playlist's text as `clip.mp4`, and with Original there is no format check, so the row ends Completed holding a text file. If the file needs a `Referer`, the header is dropped and the server refuses it (Refused, then the failure after refreshing). A protocol such as RTMP goes to the engine instead of yt-dlp.
- Evidence: `yt-dlp --load-info-json` on such a video printed no `formats`, with `protocol: "m3u8_native"`, `format_id: "0"` and `http_headers` including `Referer` at the top level. `Info` reads only `url`, `ext` and `format_id` there, and `all_formats` fills the rest from `Format::default()`. `transport` then reads a missing protocol as `"https"`.
- Confidence: certain

### Xpath-media-download-4 [P2] A page with several videos is reported as having none

- Where: `crates/media/src/ytdlp/info.rs` `Info`, `crates/media/src/choice.rs` `from_page`, `nothing`, `crates/core/src/manager/media.rs` `analyse`
- Category: wrong implementation
- Failure: a post or page with two or more videos. yt-dlp finds them, but the person is told "found no video or audio it can download... start the video, then try again", with a Retry that always ends the same way. A link added in the app with `page_otherwise` downloads the web page as a file instead. A real playlist address also has every entry read first, until the time limit or the output limit.
- Evidence: a local page with two `<video>` tags, through `yt-dlp --no-playlist -J`, printed `_type: "playlist"` and `entries` of 2, each with its formats, and no `formats`, `url` or `format_id` at the top. `--no-playlist` only applies where a link names both a video and a list. `Info` reads no `_type` or `entries`, so `all_formats` is empty and `usable_formats` returns `nothing()`. Neither `docs/ROADMAP.md` nor `docs/DECISIONS.md` records this as a limit.
- Confidence: certain

### Xpath-media-download-5 [P2] Answering Replace for a file with another extension abandons the finished video and downloads it again

- Where: `crates/core/src/manager/media_run/placement.rs` `place`, `final_path`, `crates/core/src/manager/file_exists.rs` `replace_at`, `crates/core/src/manager/media_run/mod.rs` `run`
- Category: bug
- Failure: planned `clip.mp4`, the finished file is `.mkv` (a stream or a Retry at best quality whose container differs from the forecast), `clip.mkv` is in the folder, and the setting is Ask. The file waits in `clip.mp4.wcmedia` and the person answers Replace. The run does not place it: the page is read and the whole video downloaded again into `clip.mkv.wcmedia`. If the link has since expired the download fails instead. The old folder, with the complete file, stays on disk for good.
- Evidence: `place` builds `Found { path: wanted }` from `final_path`, so `Found.path` differs from the row's path. `replace_at` calls `settle_at(entry, path)`, which sets `row.path` to it. `run` derives `work_dir(&job.output)` from the row's path, so `waiting_to_place` looks in a new folder and finds no `placing.done`. `Entry::partial_paths` also derives from the row's path, so no later Remove finds the old folder. Keep both does not move the row's path and works.
- Confidence: certain

### Xpath-media-download-6 [P2] The Refresh link fix cannot work on media, whose parts are reused whatever the link is

- Where: `crates/media/src/describe.rs` `unknown_format`, `stream`, `crates/core/src/manager/downloads.rs` `refresh_link`, `crates/core/src/manager/media_run/workdir.rs` `Downloaded::read`, `crates/core/src/manager/media_run/page.rs` `fetch_part`
- Category: wrong implementation
- Failure: a part saved as an error page gives `UnknownFormat`, whose message says the link no longer leads to the video and offers Refresh link. The person gives a fresh link, the run reuses the same bytes, and the same error returns. Only removing the download gets out. A stream with the same paths behaves the same way, since its fingerprint ignores hosts and signing values. Given a link to a different video, a page download with both parts finished silently saves the old video under the new link, and with only one part finished it joins the video of one page to the audio of the other (when the format ids match, as they do across YouTube videos).
- Evidence: `refresh_link` only changes `row.url`, the headers and the status, and leaves the work folder. `page_attempt` starts from `Downloaded::read`, before reading the page, and `fetch_part` trusts a `.done` marker that names only a file. Neither records a link, although `placing.done` does (`link_digest`), so one file in the folder is bound to its link and the others are not.
- Confidence: certain for the loop; likely for different videos, which needs a person to paste another page's link

### Xpath-media-download-7 [P2] A typed extension decides the join's container, and a refusal has no way out

- Where: `crates/core/src/manager/media_run/save.rs` `joined_container`, `join_original`, `crates/core/src/manager/media.rs` `insert_chosen`, `crates/media/src/merge.rs` `join`, `crates/streams/src/download/join.rs` `mux`
- Category: wrong implementation
- Failure: a quality made of H.264 and AAC, Original, with `clip.webm` typed as the file name. Both parts download, then every join fails: "ffmpeg could not join the video and audio... Try again". Try again repeats it, and Keep the original is refused because the job has no output. The same happens with no typed name if yt-dlp named a codec wrongly and `kept_ext` trusted it.
- Evidence: the plain resolve keeps `output: None` and `keep_ext: None`, so `join_original` gets `ext: None`, and `joined_container` takes the planned file's extension. The comment in `insert_chosen` promises the reverse: "A typed name keeps its words; the finished file gets the extension of the format it is in". `media_form.rs` `answer` sends the field as typed. Real ffmpeg with those inputs and `-c copy -f webm`: "Only VP8 or VP9 or AV1 video and Vorbis or Opus audio and WebVTT subtitles are supported for WebM", exit non-zero. `merge::join` tries one container, while `mux` in the streams crate falls back to Matroska for this very reason ("MP4 can still refuse codecs the stream did not name, or named wrongly"). The yt-dlp fallback's `--merge-output-format` comes from the same `joined_container`.
- Confidence: certain

### Xpath-media-download-8 [P2] A page's HLS part runs outside the run's file work, so a new run can meet it

- Where: `crates/core/src/manager/media_run/page.rs` `fetch_part`, `crates/core/src/manager/run.rs` `RunGuard`, `crates/streams/src/download/join.rs` `concat`
- Category: bug
- Failure: a page format downloaded as HLS is stopped while its segments are being joined on a slow disk, past `stop_timeout`. The task is aborted and the run reported ended while the blocking `concat` is still writing. The next run opens the same `track0` file, sets its length back and appends from the last noted batch, with two writers on one file. A track saved this way is then trusted by its `J` line.
- Evidence: the file branch of `fetch_part` sets `request.file_work = job.file_work.clone()`, and `stream_request` does the same for a stream job. The HLS branch does not, so `StreamRequest::new` gives it its own `FileWork::new()`. `RunGuard` waits for the run's `file_work.finished()` for the reason its doc states: "until then a new run or a deletion could meet it". `Manager::stop_run` aborts the task after `stop_timeout`, and an aborted `blocking` call leaves its `spawn_blocking` running. A deletion can meet it the same way.
- Confidence: likely (by reading; not reproduced, and it needs a stop that outlasts the timeout)

### Xpath-media-download-9 [P2] A saved format that the page no longer lists can never succeed, and the words point elsewhere

- Where: `crates/core/src/manager/media_run/page.rs` `page_attempt`, `crates/media/src/ytdlp/info.rs` `selection`, `crates/media/src/describe.rs` `ytdlp`
- Category: wrong implementation
- Failure: a download paused or queued for later whose format id is gone when the page is read again (a site's list changes, ids such as `hls-2148` move). The comment says "the page changed: yt-dlp downloads the same selection", but that selection names the missing ids exactly, so yt-dlp stops with a format-not-available error. The row says to check the video in the browser and update the add-on, with Retry. Retry repeats it, and nothing lets the person choose another quality. Only removing it and starting over works.
- Evidence: `ourselves` is false when `found.len() != wanted.len()`, then `ytdlp::download` runs with `b*[format_id="..."]` built from the stored ids. The job is never re-resolved against the new list, `YtDlpError::Refused` always gets Retry, and streams word the same case differently (`NoSuchTrack` offers a refresh).
- Confidence: likely (by reading; yt-dlp's refusal of a missing exact id was not run)

### Xpath-media-download-10 [P2] A conversion demands audio from a video whose audio yt-dlp did not describe

- Where: `crates/media/src/output/arguments.rs` `add_maps`, `crates/media/src/ytdlp/info.rs` `Format::has_audio`, `crates/media/src/output/variants.rs` `original_audio_action`
- Category: bug
- Failure: a silent video whose format has no `acodec`, saved in another format or with subtitles put inside. The job says the audio is copied or re-encoded, ffmpeg refuses to start, and the row reports a failed conversion. Keep the original saves it, without the format or subtitles asked for.
- Evidence: `has_audio` is `is_none_or(|c| c != "none")`, so unknown counts as present. `add_maps` then maps `{v}:a:0` with no `?`. Real ffmpeg on a video with no audio: "Stream map '' matches no streams". The stream layer's `copy_arguments` uses `v?` and `a?` for this reason, so the two ffmpeg argument builders disagree about what an input may lack.
- Confidence: certain for the ffmpeg failure; likely for how often yt-dlp leaves `acodec` out on such a video

### Xpath-media-download-11 [P2] A stream whose placing fails is downloaded again, though the file is in the work folder

- Where: `crates/core/src/manager/media_run/mod.rs` `run_stream`, `crates/core/src/manager/media_run/placement.rs` `finish`, `crates/streams/src/download/join.rs` `join`
- Category: inefficiency
- Failure: a finished stream sits in `<name>.wcmedia/`, and `place` returns an error (the destination folder denies the rename, say). The row fails and Try again fetches every segment again, while the finished file is still there and then replaced or kept under a new name.
- Evidence: `join` deletes the `.wcstream` folder, with its `done.log`, before returning `Completed`, and `finish` runs after that. Only a page job writes `downloaded.json`, and only the Ask path writes `placing.done`. So a stream's next run finds nothing to continue from. A page job in the same state goes straight back to `save`.
- Confidence: certain

## Suggestions

### Xpath-media-download-S1 Try yt-dlp once when the engine's fetch of a format keeps being refused

- Today: `run_page` retries the engine with fresh addresses, then fails, even though yt-dlp may hold what the site wants (cookies, a client it prefers).
- Better: after the last refresh, run `ytdlp::download` with the same selection before showing the error. How other download managers handle this was not checked.
- Value: fewer failed videos on sites that treat a plain client differently, whatever the cause.

### Xpath-media-download-S2 One test that ties what can be offered to what ffmpeg may read

- Today: `extension::MEDIA`, `Container::from_ext` and `InputFormat` are separate lists, and no test compares them, so Xpath-media-download-1 went unseen.
- Better: a test that, for every extension in `MEDIA`, states whether `InputFormat` reads it, and a rule in `from_page` that leaves out or marks "Original only" whatever it does not.
- Value: the quality window never offers a choice that is sure to fail after the download.

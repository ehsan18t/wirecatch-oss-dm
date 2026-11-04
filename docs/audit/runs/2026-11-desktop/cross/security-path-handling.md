# Cross-module flow: security-path-handling

Every name or folder from outside followed to where it becomes a path on disk.

Followed and found sound: a server's `Content-Disposition` and a link's name (decoded, then `sanitize`, with folder parts, stream colons, control and invisible characters, trailing dots and spaces and Windows devices all handled, also after a cut); a browser's suggested name and video title (`naming::file_name`, `wirecatch_media::file_name`); a typed name; the bridge's `NewDownload`, which carries no folder; yt-dlp's result path (`printed_output`, `confirmed_output`), format extension (`safe_ext`), subtitle extension (`pick_file` allows only four) and language (`sidecar_path`); the names read back from a work folder (`plain_name`, `waiting_to_place`, `fetch_part`); stream work files, whose names hold nothing from a manifest; a yt-dlp tag in a path (`file_safe`); and plain and placeholder folders in a settings file (`..`, devices, verbatim and network paths, links through `real_path`). The final file cannot leave its folder through a name. Symlinks at these paths can only come from a program of the same user, which D41.2 places outside the boundary.

## Files read

Whole, or all the code with tests skimmed:

crates/engine/src/naming.rs
crates/engine/src/target.rs
crates/core/src/naming.rs
crates/core/src/manager/settle.rs
crates/core/src/manager/captures.rs
crates/core/src/manager/downloads.rs
crates/core/src/manager/run.rs
crates/core/src/manager/media_run/mod.rs
crates/core/src/manager/media_run/placement.rs
crates/core/src/manager/media_run/save.rs
crates/core/src/manager/media_run/page.rs
crates/core/src/manager/media_run/extras.rs
crates/core/src/manager/media_run/workdir.rs
crates/media/src/lib.rs
crates/media/src/ytdlp/download.rs
crates/media/src/output/subtitles.rs
crates/settings/src/folders.rs
crates/settings/src/settings_file.rs
crates/files/src/discard.rs

In part, the sections this flow passes through:

crates/core/src/manager/media.rs (naming and `insert_chosen`)
crates/core/src/manager/state.rs (`partial_paths`)
crates/core/src/manager/app_update.rs (`fetch_app_update`)
crates/core/src/manager/addons/install.rs (`release_file`, `file_safe`, the resolver)
crates/media/src/ytdlp/mod.rs (`common_args`)
crates/media/src/ytdlp/cookies.rs (the file's name)
crates/shell/src/api.rs (`file_to_open`, `item_to_reveal`, `is_program`)
crates/updates/src/manifest.rs (reading the signed list)

Left out, known only through their contracts: `crates/core/src/manager/relocate.rs`, `file_exists.rs` and `restore.rs`, which take names already made safe. `crates/addons/src/archive.rs` belongs to the add-on unpacking flow. `crates/streams/src/workdir.rs` and `download/join.rs` were searched for path joins, not read whole.

## Findings

### Xsecurity-path-handling-1 [P0] Removing a finished download with its file deletes a whole folder now at that path

- Where: `crates/core/src/manager/run.rs`, `delete_files`; `crates/core/src/manager/downloads.rs`, `finish_removing`
- Category: bug
- Failure: a download finishes as `data`, a name with no extension taken from the link or the server. The person renames it `data.zip` and extracts it with "Extract here", which makes a folder `data` at the saved path. They then remove the row with its file, or delete its queue with files. The folder and everything in it is deleted for good, with no Recycle Bin. Any folder that later takes a finished row's saved path goes the same way.
- Evidence: for a `Completed` row with `RemoveFiles::Delete`, `finish_removing` pushes `entry.path()` into `files`. `delete_files` then asks `path.is_dir()` and calls `wirecatch_files::discard::dir`, which is `std::fs::remove_dir_all`. The folder branch exists for the work folders in `partial_paths`, but the finished path shares it, and nothing checks that the path is still a file. The callers are `main_window/confirm.rs` and `main_window/queue_delete.rs`. By reading only: not run, so stage 2 should prove it with a test on `Manager::remove`.
- Confidence: certain

### Xsecurity-path-handling-2 [P1] On Windows a settings file can send downloads into a folder whose programs Wirecatch runs

- Where: `crates/settings/src/folders.rs`, `protected_folders` (Windows) and `KnownFolders::is_protected`; `crates/settings/src/settings_file.rs`, `localize_folders`
- Category: wrong implementation
- Failure: an imported file sets `default_folder` or a category folder to `{Home}/AppData/Local/Wirecatch/addons/media/ffmpeg`, and can also turn `ask_before_download` off. `resolve_local` accepts it, and the preview does not mark it as outside the person's folders. A page then starts a download whose server names it after a DLL `ffmpeg.exe` looks for in its own folder first. The browser capture saves it beside the program, and the next conversion may load it, running the site's code as the person. The Mark of the Web does not stop a DLL load. The same applies to folders of other programs under `%LOCALAPPDATA%\Programs`.
- Evidence: the Windows list holds only the Start menus, `WindowsApps`, Windows and the program folders, while the Linux rule refuses every hidden folder in home, which covers the add-on's `~/.local/share/wirecatch`. So the same attack is refused on Linux and passes on Windows. `PathError::Protected` and the doc of `protected` state the aim: no folder where "a file saved there runs by itself". A test accepts `{Home}/AppData/Roaming/Other` as a normal place; no decision in `docs/DECISIONS.md` records that choice. `is_outside_user_folders` is false because the path is inside `{Home}`. `addons::tools` checks only the files it recorded, so an extra file is not noticed. ffmpeg is the shared build (`avcodec-63.dll` and others beside `ffmpeg.exe`). It needs a person to import a crafted file, which is why it is P1 and not P0. The folder rule is certain from the code; that a particular DLL name loads was not tried.
- Confidence: likely

### Xsecurity-path-handling-3 [P2] A video title with a yt-dlp field sends the file to another folder and the download fails

- Where: `crates/media/src/ytdlp/download.rs`, `download_args`; the title reaches the path through `crates/media/src/lib.rs`, `file_name`, and `work_dir`
- Category: bug
- Failure: a site's title holds text such as `Save 50%(limited) sale`, and its format is one yt-dlp must download itself. `sanitize` keeps `%`, `(` and `)`, so the work folder's name is passed inside the `-o` template and yt-dlp expands it. The file is saved in `Save 50NAale.mp4.wcmedia`, beside the real work folder. `printed_output` and `largest_result` find nothing in the real one, so the download ends with `NothingSaved`. The other folder is not in `partial_paths`, so cancelling or removing the download leaves the whole file behind.
- Evidence: `args.push(work.join("media.%(ext)s"))` does not escape `work`. Run with the installed yt-dlp and `--simulate --print filename`, with the title as the folder's name: `%(id)s` gives `a.mp4.wcmedia`, `Save 50%(limited) sale` gives `Save 50NAale.mp4.wcmedia`, `a%%b` gives `a%b.mp4.wcmedia`. It does not climb out of the parent folder: `%(id&..)s` gives `...mp4.wcmedia`, `%(webpage_url)j`, `l` and `S` give a name with the slashes removed, and `sanitize` has already taken separators out of the title. This is the template fault contract 16 lists as 16-1. What this flow adds is that a site's title reaches it and that the file stranded lies outside what removal deletes.
- Confidence: certain

## Suggestions

### Xsecurity-path-handling-S1 Keep every name the app uses for its own files away from downloads

- Today: `naming::checked` refuses or changes names ending `.wcpart` and `.wcstate`, but not `.wcstate.tmp`, `.wcmedia` or `.wcstream`. A file named `x.mp4.wcmedia` makes `finished_size` treat a finished `x.mp4` as unfinished, and stops a video named `x.mp4` from making its work folder.
- Better: treat all five endings alike in `checked`, from one list. aria2 likewise keeps its `.aria2` control name tied to the download it belongs to.
- Value: a site cannot, even by accident, make one download look unfinished or fail because of another's name.

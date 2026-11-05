//! Putting the finished file and its subtitles in place, under a free name when the planned one is taken.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use tracing::warn;
use wirecatch_engine::Url;
use wirecatch_media::describe;
use wirecatch_protocol::app::{UserError, WhenFileExists};

use super::{Job, disk, workdir};
use crate::headers;
use crate::manager::run::MediaDone;
use crate::manager::state::{Found, OnExisting};

/// What a finished file does when a file in the folder already has its name (D96).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Taken {
    Replace,
    KeepExisting,
    KeepBoth,
    Ask,
}

impl Taken {
    /// The person's answer for this download, else the setting.
    pub(super) fn of(answered: Option<OnExisting>, setting: WhenFileExists) -> Self {
        match (answered, setting) {
            (Some(OnExisting::Replace), _) | (None, WhenFileExists::Replace) => Taken::Replace,
            (Some(OnExisting::KeepBoth), _) | (None, WhenFileExists::KeepBoth) => Taken::KeepBoth,
            (None, WhenFileExists::KeepExisting) => Taken::KeepExisting,
            (None, WhenFileExists::Ask) => Taken::Ask,
        }
    }
}

/// Where [`place_noting`] put a finished file.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Placed {
    At(PathBuf),
    /// The file already in the folder is kept as the download, and the new one discarded.
    Kept {
        path: PathBuf,
        size: u64,
    },
    /// The name is taken and the person is asked; the new file waits in the work folder.
    Asks(Found),
}

/// Reserves a path for a finished file (`true` to reserve, `false` to give a
/// reservation back); reserving returns `false` when another download holds it.
pub(super) type Reserve = Arc<dyn Fn(&Path, bool) -> bool + Send + Sync>;

/// Moves the finished file from the work folder to its place and removes the
/// work folders. The file keeps its own extension; if that differs from the
/// planned one and the name is taken, a free name is used. Subtitle files
/// (`sidecars`, each with its language) go next to it as
/// `<name>.<language>.srt`, never replacing a file.
pub(super) async fn finish(
    file: &Path,
    job: &Job,
    work: &Path,
    sidecars: &[(PathBuf, String)],
) -> Result<MediaDone, UserError> {
    let output = &job.output;
    let (file, planned, work) = (file.to_path_buf(), output.clone(), work.to_path_buf());
    let (url, referrer) = (
        job.url.to_string(),
        headers::referrer(&job.origin.headers).map(str::to_string),
    );
    let reserve = job.reserve.clone();
    let placing = job.placing.clone();
    let sidecars = sidecars.to_vec();
    let taken = job.taken;
    let moved = job
        .file_work
        .spawn_blocking(move || -> std::io::Result<MediaDone> {
            let (target, bytes) =
                if let Some(target) = placed_before(&file, &work, workdir::PLACING_TO) {
                    let bytes = std::fs::metadata(&target)?.len();
                    (target, bytes)
                } else {
                    // On disk before the work folder goes, so a power cut cannot lose both it and its sources.
                    wirecatch_files::whole_file::flush(&file)?;
                    // Marked before it is put in place, so it is never there unmarked.
                    wirecatch_files::zone::mark(&file, &url, referrer.as_deref());
                    placing();
                    remember_placing(&work, &url, &file, &sidecars)?;
                    let note = |how: Moving, to: &Path| note_move(&work, how, to);
                    match place_noting(&file, &planned, &reserve, taken, &note)? {
                        Placed::At(target) => {
                            let bytes = std::fs::metadata(&target)?.len();
                            (target, bytes)
                        }
                        Placed::Kept { path, size } => (path, size),
                        // The work folder stays, with the file in it, for the answer.
                        Placed::Asks(found) => return Ok(MediaDone::FileExists(found)),
                    }
                };
            let placed = place_subtitles(&sidecars, &target, &work, &url, referrer.as_deref());
            wirecatch_files::discard::dir(&work);
            Ok(MediaDone::Completed {
                path: target,
                bytes,
                sidecars: placed,
            })
        })
        .await
        .map_err(|e| describe::internal(&e))?;
    moved.map_err(|e| disk(&e, output))
}

/// Puts each subtitle file next to `video`, and returns where they went. One that cannot be placed is logged, as the video is whole without it.
fn place_subtitles(
    sidecars: &[(PathBuf, String)],
    video: &Path,
    work: &Path,
    url: &str,
    referrer: Option<&str>,
) -> Vec<PathBuf> {
    let anywhere: Reserve = Arc::new(|_: &Path, _| true);
    let mut placed = Vec::new();
    for (subtitle, language) in sidecars {
        let name = crate::naming::file_name_of(subtitle);
        if let Some(target) = placed_before(subtitle, work, &workdir::placing_subtitle_to(&name)) {
            placed.push(target);
            continue;
        }
        wirecatch_files::zone::mark(subtitle, url, referrer);
        let beside = sidecar_path(video, language);
        let note = |how: Moving, to: &Path| note_subtitle_move(work, &name, how, to);
        match place_noting(subtitle, &beside, &anywhere, Taken::KeepBoth, &note) {
            Ok(Placed::At(path)) => placed.push(path),
            Ok(Placed::Kept { .. } | Placed::Asks(_)) => {}
            Err(e) => warn!(error = %e, "could not put a subtitle file next to the video"),
        }
    }
    placed
}

pub(super) fn link_digest(url: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hex = String::with_capacity(64);
    for byte in Sha256::digest(url.as_bytes()) {
        for nibble in [byte >> 4, byte & 0xf] {
            hex.extend(char::from_digit(u32::from(nibble), 16));
        }
    }
    hex
}

pub(super) fn remember_placing(
    work: &Path,
    url: &str,
    file: &Path,
    sidecars: &[(PathBuf, String)],
) -> std::io::Result<()> {
    let name = |path: &Path| crate::naming::file_name_of(path);
    let mut lines = vec![link_digest(url), name(file)];
    lines.extend(
        sidecars
            .iter()
            .map(|(path, language)| format!("{}\t{language}", name(path))),
    );
    std::fs::write(work.join(workdir::PLACING), lines.join("\n"))
}

/// How a finished file is about to reach its name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Moving {
    /// Onto a free name: a file there afterwards is this one.
    ToFreeName,
    /// Over the file at that name, or keeping it and discarding this one: a file there says nothing until this one is gone.
    OntoTakenName,
}

impl Moving {
    const FREE: &str = "free";
    const TAKEN: &str = "taken";
}

pub(super) fn note_move(work: &Path, how: Moving, to: &Path) -> std::io::Result<()> {
    note_move_in(work, workdir::PLACING_TO, how, to)
}

/// Notes the move of the subtitle file named `subtitle`.
pub(super) fn note_subtitle_move(
    work: &Path,
    subtitle: &str,
    how: Moving,
    to: &Path,
) -> std::io::Result<()> {
    note_move_in(work, &workdir::placing_subtitle_to(subtitle), how, to)
}

fn note_move_in(work: &Path, note: &str, how: Moving, to: &Path) -> std::io::Result<()> {
    let how = match how {
        Moving::ToFreeName => Moving::FREE,
        Moving::OntoTakenName => Moving::TAKEN,
    };
    let name = crate::naming::file_name_of(to);
    std::fs::write(work.join(note), format!("{how}\n{name}"))
}

/// How the last move was noted in `note` and the file now at its name, when there is one.
fn noted_move(work: &Path, note: &str) -> Option<(String, PathBuf)> {
    let note = std::fs::read_to_string(work.join(note)).ok()?;
    let (how, name) = note.split_once('\n')?;
    let target = work.parent()?.join(name);
    (workdir::plain_name(name) && target.is_file()).then(|| (how.to_owned(), target))
}

/// Where an earlier run put `file`, when it died between that and removing `work`. A file still in `work` was linked to its free name and not yet discarded only if the two are one file: a note stays after a move that failed, and another file can take the name later.
fn placed_before(file: &Path, work: &Path, note: &str) -> Option<PathBuf> {
    let (how, target) = noted_move(work, note)?;
    if !file.exists() {
        return Some(target);
    }
    let linked = how == Moving::FREE && same_file::is_same_file(file, &target).unwrap_or(false);
    linked.then(|| {
        wirecatch_files::discard::file(file);
        target
    })
}

/// The file, and its subtitles, an earlier run of the download of `url` left in `work` to be put
/// in place, or put there and died before tidying up. Only plain names inside `work` are taken.
pub(super) async fn waiting_to_place(
    work: &Path,
    url: &Url,
) -> Option<(PathBuf, Vec<(PathBuf, String)>)> {
    let listed = tokio::fs::read_to_string(work.join(workdir::PLACING))
        .await
        .ok()?;
    let mut lines = listed.lines();
    if lines.next()? != link_digest(url.as_str()) {
        return None;
    }
    let inside = |name: &str| {
        let path = work.join(name);
        (path.parent() == Some(work) && path.is_file()).then_some(path)
    };
    let name = lines.next()?.trim();
    let file = inside(name).or_else(|| {
        let file = work.join(name);
        (file.parent() == Some(work) && noted_move(work, workdir::PLACING_TO).is_some())
            .then_some(file)
    })?;
    let subtitle = |name: &str| {
        inside(name).or_else(|| {
            let file = work.join(name);
            (file.parent() == Some(work)
                && noted_move(work, &workdir::placing_subtitle_to(name)).is_some())
            .then_some(file)
        })
    };
    let sidecars = lines
        .filter_map(|line| line.split_once('\t'))
        .filter_map(|(name, language)| Some((subtitle(name)?, language.to_string())))
        .collect();
    Some((file, sidecars))
}

/// Where a subtitle file in `language` goes next to `file`:
/// `<file's name>.<language>.srt`, with the language reduced to letters,
/// digits and dashes.
pub(super) fn sidecar_path(file: &Path, language: &str) -> PathBuf {
    let stem = file
        .file_stem()
        .map_or_else(|| "video".into(), |s| s.to_string_lossy().into_owned());
    let language: String = language
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
        .take(wirecatch_settings::MAX_LANGUAGE_CHARS)
        .collect();
    let language = if language.is_empty() {
        "und".into()
    } else {
        language
    };
    file.with_file_name(format!("{stem}.{language}.srt"))
}

/// Where a finished file goes: the planned path, with the file's own extension
/// when that is a different media extension (the container actually written).
/// An extension that is not a media one is never taken from the file: the
/// site, not the user, chose it.
pub(super) fn final_path(file: &Path, planned: &Path) -> PathBuf {
    let own = file
        .extension()
        .and_then(|e| e.to_str())
        .filter(|e| wirecatch_media::is_media_ext(e))
        .map(str::to_ascii_lowercase);
    match own {
        Some(ext)
            if !planned
                .extension()
                .and_then(|p| p.to_str())
                .is_some_and(|p| p.eq_ignore_ascii_case(&ext)) =>
        {
            wirecatch_streams::extension::with_extension(planned, &ext)
        }
        _ => planned.to_path_buf(),
    }
}

/// Moves `file` to the free name `candidate`; `false` when another file took it first.
fn move_to_free_name(file: &Path, candidate: &Path) -> std::io::Result<bool> {
    // A hard link first, not a rename: a scanner holding the file on Windows refuses a rename, not a link. Neither replaces a file that appeared since the check.
    match std::fs::hard_link(file, candidate) {
        Ok(()) => {
            wirecatch_files::discard::file(file);
            Ok(true)
        }
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Ok(false),
        // A drive without hard links (FAT, exFAT).
        Err(_) => match wirecatch_engine::naming::rename_no_replace(file, candidate) {
            Ok(()) => Ok(true),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Ok(false),
            Err(e) => Err(e),
        },
    }
}

/// Free names tried for a finished file before placing it fails.
pub(super) const MAX_NAME_ATTEMPTS: u64 = 10_000;

/// Moves the finished `file` to `planned` (with the file's own media
/// extension, see [`final_path`]). When a file in the folder has that name, `taken` says what
/// happens; keeping both, or a name another download holds, goes on to `name (1).ext` and so on,
/// and never replaces a file. Calls `note` before each move of the file. Blocks on the disk.
pub(super) fn place_noting(
    file: &Path,
    planned: &Path,
    reserve: &Reserve,
    taken: Taken,
    note: &dyn Fn(Moving, &Path) -> std::io::Result<()>,
) -> std::io::Result<Placed> {
    let wanted = final_path(file, planned);
    let dir = wanted.parent().unwrap_or(Path::new(""));
    let name = crate::naming::file_name_of(&wanted);
    // Not `finished_size`: this download's own work folder sits beside the name.
    if let Some(size) = std::fs::metadata(&wanted)
        .ok()
        .filter(std::fs::Metadata::is_file)
        .map(|m| m.len())
    {
        // Only the planned path is this download's own: another name, from the file's real type, may be another download's.
        let other_name = wanted != planned;
        match taken {
            Taken::KeepBoth => {}
            Taken::Replace | Taken::KeepExisting if other_name && !reserve(&wanted, true) => {}
            // Rename replaces a file at the target on both systems.
            Taken::Replace => {
                let noted = note(Moving::OntoTakenName, &wanted);
                if let Err(e) = noted.and_then(|()| std::fs::rename(file, &wanted)) {
                    if other_name {
                        reserve(&wanted, false);
                    }
                    return Err(e);
                }
                return Ok(Placed::At(wanted));
            }
            Taken::KeepExisting => {
                if let Err(e) = note(Moving::OntoTakenName, &wanted) {
                    if other_name {
                        reserve(&wanted, false);
                    }
                    return Err(e);
                }
                wirecatch_files::discard::file(file);
                return Ok(Placed::Kept { path: wanted, size });
            }
            Taken::Ask => {
                let same = std::fs::metadata(file).is_ok_and(|m| m.len() == size);
                return Ok(Placed::Asks(Found {
                    saved_at: crate::manager::file_exists::saved_at(&wanted),
                    path: wanted,
                    size,
                    then: WhenFileExists::Ask,
                    same,
                }));
            }
        }
    }
    for n in 0..MAX_NAME_ATTEMPTS {
        let candidate = crate::naming::candidate(dir, &name, n);
        if !crate::naming::is_free_on_disk(&candidate) {
            continue;
        }
        // The planned path is this download's own; any other is reserved, and
        // given back if it cannot be used after all.
        let reserved = candidate != planned;
        if reserved && !reserve(&candidate, true) {
            continue;
        }
        if let Err(e) = note(Moving::ToFreeName, &candidate) {
            if reserved {
                reserve(&candidate, false);
            }
            return Err(e);
        }
        let moved = move_to_free_name(file, &candidate);
        match moved {
            Ok(true) => return Ok(Placed::At(candidate)),
            Ok(false) | Err(_) if reserved => {
                reserve(&candidate, false);
                moved?;
            }
            other => {
                other?;
            }
        }
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::AlreadyExists,
        "no free name for the video",
    ))
}

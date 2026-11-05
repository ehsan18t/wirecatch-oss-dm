//! Where a download's file goes, and whether an earlier run of it continues.
//!
//! [`Destination::choose`] decides from the request and the server's first
//! answer without touching the disk; [`Destination::prepare`] then does the
//! file system work (folders, the resume file, the part file), which blocks and
//! so runs on the blocking pool.

use std::io;
use std::path::{Path, PathBuf};

use tracing::{info, warn};
use url::Url;

use crate::abandon::Abandoned;
use crate::download::{DownloadRequest, RemoteFile};
use crate::error::{Chain, Error, IoAction, IoContext, Result};
use crate::http::Validators;
use crate::segment::SegmentTable;
use crate::state::{LoadError, ResumeState};
use crate::storage::{Finish, PartLock};
use crate::transport::Opened;
use crate::url::without_password;
use crate::{ExistingFile, naming, storage};

/// Where the file goes, before anything is on disk.
pub(crate) enum Destination {
    /// Exactly this path: the caller's own, or the one its resolver picked. An
    /// earlier run there is continued whatever its URL was. A file at the path
    /// when the download finishes is replaced or kept as the caller said.
    Exact(PathBuf, ExistingFile),
    /// `name` in `dir`, or the first free name after it (`name (1)`...), taken
    /// by creating its part file ([`naming::create_free_part`]), so no other
    /// download can take it; nothing is at the final name until the download
    /// finishes, and a file another program saved there meanwhile is never
    /// replaced ([`Finish::FreeName`]). An earlier run under `name` or a number
    /// of it is continued only if it had the same URL.
    InFolder { dir: PathBuf, name: String },
}

/// What the server said about the file, as far as continuing goes.
pub(crate) struct Described {
    pub ranged: bool,
    pub total: Option<u64>,
    pub validators: Validators,
}

impl Described {
    pub fn of(opened: &Opened) -> Self {
        Self {
            ranged: opened.ranged,
            total: opened.total,
            validators: opened.validators.clone(),
        }
    }
}

/// The paths of one download.
pub(crate) struct Target {
    /// The finished file.
    pub file: PathBuf,
    /// The file the data goes to until then.
    pub part: PathBuf,
    /// The resume file.
    pub state: PathBuf,
    /// What happens to a file already at `file` when the download finishes.
    pub finish: Finish,
    /// Keeps other runs off the part file until this is dropped, at the end of the run.
    lock: PartLock,
}

impl Target {
    /// The finished file's name, for showing.
    pub fn name(&self) -> String {
        self.file
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
    }
}

/// A destination ready on disk: its part file exists, sized when the size is
/// known.
pub(crate) struct Prepared {
    pub target: Target,
    /// The ranges still to fetch.
    pub table: SegmentTable,
    /// Whether an earlier run is continued.
    pub resumed: bool,
    /// Bytes an earlier run already has.
    pub resumed_from: u64,
}

impl Destination {
    /// Where the file goes: the caller's exact path, the path the caller's
    /// resolver picks from the server's answer, or a free name in the folder.
    /// Does no I/O itself; the resolver is the caller's, which must not block.
    pub fn choose(request: &DownloadRequest, opened: &Opened) -> Result<Self> {
        match (&request.exact_path, &request.resolve_path) {
            (Some(path), _) => Ok(Self::Exact(path.clone(), request.if_exists)),
            (None, Some(resolve)) => {
                let remote = RemoteFile {
                    suggested_name: suggested_name(None, opened, &request.url),
                    total: opened.total,
                    content_type: opened.content_type.clone(),
                    final_url: opened.final_url.clone(),
                    resumable: opened.ranged,
                };
                let path = resolve(&remote).map_err(Error::TargetRejected)?;
                Ok(Self::Exact(path, request.if_exists))
            }
            (None, None) => Ok(Self::InFolder {
                dir: request.dest_dir.clone(),
                name: suggested_name(request.file_name.clone(), opened, &request.url),
            }),
        }
    }

    /// Makes the destination ready: creates its folder, continues an earlier run that can be continued and otherwise replaces what is left of it with a new part file. `saved_url` is what the resume file records for this download. Blocks.
    ///
    /// Creating the folder can be slow (a network drive), and the owner may drop the download meanwhile and start another run with the same files, so `abandoned` is checked again before any file is deleted or created.
    pub fn prepare(
        self,
        saved_url: &str,
        file: &Described,
        min_split: u64,
        abandoned: &Abandoned,
    ) -> Result<Prepared> {
        let plan = match self {
            Self::Exact(path, existing) => {
                if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
                    std::fs::create_dir_all(parent).context(IoAction::CreateFolder, parent)?;
                }
                let finish = match existing {
                    ExistingFile::Refuse => Finish::Refuse,
                    _ => Finish::Replace,
                };
                let (target, resume) = plan_exact(path, finish, file)?;
                Plan::Planned(target, resume.map(Box::new))
            }
            Self::InFolder { dir, name } => {
                std::fs::create_dir_all(&dir).context(IoAction::CreateFolder, &dir)?;
                plan_in_folder(dir, name, saved_url, file)?
            }
        };
        #[cfg(test)]
        abandoned.slow_step_done();
        abandoned.check()?;
        let target = match plan {
            Plan::Planned(target, Some(saved)) => {
                let table = SegmentTable::from_remaining(&saved.remaining, min_split);
                // `can_resume` checked that the ranges fit inside the size.
                let kept = file
                    .total
                    .unwrap_or(0)
                    .saturating_sub(table.remaining_bytes());
                info!(kept, "resuming");
                return Ok(Prepared {
                    target,
                    table,
                    resumed: true,
                    resumed_from: kept,
                });
            }
            Plan::Planned(mut target, None) => {
                // Whatever is left there cannot continue: start over. The part file is emptied, not deleted: its lock would stop a new one from being made on Windows. A resume file that stayed would count the new part file's zeros as downloaded.
                remove_resume_file(&target.state)?;
                storage::create_part(&target.part, file.total)
                    .context(IoAction::CreateFile, &target.part)?;
                if !target.lock.is_held() {
                    target.lock = PartLock::open(&target.part).unwrap_or_default();
                }
                target
            }
            Plan::FreeName { dir, name } => {
                let (path, part, lock) = naming::create_free_part(&dir, &name)
                    .context(IoAction::CreateFile, &dir.join(&name))?;
                let mut target = self::target(path, Finish::FreeName { dir, name });
                target.lock = lock;
                if let Err(e) = storage::size_part(&target.part, &part, file.total) {
                    drop(part);
                    target.lock = PartLock::default();
                    // Nothing was downloaded under the name: give it back.
                    discard(&target.part);
                    return Err(e).context(IoAction::CreateFile, &target.part);
                }
                target
            }
        };
        let table = SegmentTable::new(file.total, min_split);
        Ok(Prepared {
            target,
            table: if file.ranged {
                table
            } else {
                table.without_splitting()
            },
            resumed: false,
            resumed_from: 0,
        })
    }
}

/// The file name to use: the caller's, else the server's `Content-Disposition`,
/// else the URL's last segment, made safe for the file system (`download` if
/// nothing usable is left).
fn suggested_name(chosen: Option<String>, probe: &Opened, url: &Url) -> String {
    naming::sanitize(
        &chosen
            .or_else(|| probe.file_name.clone())
            .or_else(|| naming::from_url(&probe.final_url))
            .or_else(|| naming::from_url(url))
            .unwrap_or_default(),
    )
}

/// The paths for `final_path`, finished as `finish` says.
fn target(file: PathBuf, finish: Finish) -> Target {
    let (part, state) = naming::part_paths(&file);
    Target {
        file,
        part,
        state,
        finish,
        lock: PartLock::default(),
    }
}

/// [`Destination::Exact`]: the URL is not compared, only what the server says about the file. The path is the caller's, who owns who runs there, so a held lock does not stop this run.
fn plan_exact(
    path: PathBuf,
    finish: Finish,
    file: &Described,
) -> Result<(Target, Option<ResumeState>)> {
    let mut target = target(path, finish);
    target.lock = PartLock::open(&target.part).unwrap_or_default();
    let resume = match saved_state(&target.state)? {
        Some(saved) if continues(&saved, &target, file)? => Some(saved),
        _ => None,
    };
    Ok((target, resume))
}

/// What a run of `url` left at `path` when it has all the data: the resume file lists nothing left and the part file has the saved size. Another link's is not this download's, so it goes the way of any resume. Blocks.
pub(crate) fn complete_part(
    path: &Path,
    url: &str,
    existing: ExistingFile,
) -> Result<Option<CompletePart>> {
    let (part, state) = naming::part_paths(path);
    let Some(saved) = saved_state(&state)? else {
        return Ok(None);
    };
    let Some(len) = part_len(&part)? else {
        return Ok(None);
    };
    if !saved.remaining.is_empty()
        || saved.total.is_some_and(|total| total != len)
        || !same_download(&saved.url, url)
    {
        return Ok(None);
    }
    let finish = match existing {
        ExistingFile::Refuse => Finish::Refuse,
        _ => Finish::Replace,
    };
    let mut target = target(path.to_path_buf(), finish);
    target.lock = PartLock::open(&target.part).unwrap_or_default();
    Ok(Some(CompletePart { target, len, saved }))
}

/// See [`complete_part`].
pub(crate) struct CompletePart {
    pub target: Target,
    pub len: u64,
    pub saved: ResumeState,
}

/// What [`Destination::prepare`] does once the folder is there.
enum Plan {
    /// These files, continuing the earlier run when there is a resume state
    /// (boxed: it is most of the plan's size and rarely there).
    Planned(Target, Option<Box<ResumeState>>),
    /// The first free name of `name` in `dir` ([`naming::create_free_part`]).
    FreeName { dir: PathBuf, name: String },
}

/// [`Destination::InFolder`]: an earlier run of the same URL, under `name` or a
/// number of it, continued or started over; else what a run left that nothing
/// can continue; else the first free name. A run still going is not an earlier
/// one, and another link's partial file is that link's: both keep their files.
fn plan_in_folder(dir: PathBuf, name: String, url: &str, file: &Described) -> Result<Plan> {
    let found = {
        let mut names = naming::candidates(&dir, &name);
        let mut free_in_a_row = 0;
        loop {
            let finish = Finish::FreeName {
                dir: dir.clone(),
                name: name.clone(),
            };
            match look_at_name(target(names.next_name(), finish), url, file)? {
                Look::Take(plan) => break Some(plan),
                Look::Free if free_in_a_row == FREE_NAMES_TO_LOOK_PAST => break None,
                Look::Free => free_in_a_row += 1,
                Look::Occupied => free_in_a_row = 0,
            }
        }
    };
    Ok(found.unwrap_or(Plan::FreeName { dir, name }))
}

/// How many free names a look for an earlier run goes on past: the file that made an earlier run take a number may have been moved since. Bounded, so a run further along than this starts anew.
const FREE_NAMES_TO_LOOK_PAST: usize = 8;

/// What one name of a download into a folder offers it.
enum Look {
    Take(Plan),
    /// Nothing is there.
    Free,
    Occupied,
}

fn look_at_name(mut candidate: Target, url: &str, file: &Described) -> Result<Look> {
    let finished = candidate.file.exists();
    let Some(saved) = saved_state(&candidate.state)? else {
        return Ok(look_past_lost_state(candidate, finished));
    };
    if finished || !same_download(&saved.url, url) {
        return Ok(Look::Occupied);
    }
    let Some(lock) = PartLock::open(&candidate.part) else {
        return Ok(Look::Occupied);
    };
    candidate.lock = lock;
    if continues(&saved, &candidate, file)? {
        return Ok(Look::Take(Plan::Planned(candidate, Some(Box::new(saved)))));
    }
    // Same download, but it cannot continue (file changed, or no range support):
    // start over under the same name.
    info!("previous partial download cannot be resumed; starting over");
    Ok(Look::Take(Plan::Planned(candidate, None)))
}

/// A name whose resume file is missing or unreadable. A part file there that no
/// run holds can never be continued, so it is taken over; without a real lock
/// (a file system that has none) it may be a run's that has yet to save.
fn look_past_lost_state(mut candidate: Target, finished: bool) -> Look {
    if finished {
        return Look::Occupied;
    }
    let part_exists = match std::fs::symlink_metadata(&candidate.part) {
        Ok(_) => true,
        Err(e) if e.kind() == io::ErrorKind::NotFound => false,
        // Unknown, so not to be emptied.
        Err(_) => return Look::Occupied,
    };
    if !part_exists {
        return if std::fs::symlink_metadata(&candidate.state).is_ok() {
            Look::Take(Plan::Planned(candidate, None))
        } else {
            Look::Free
        };
    }
    match PartLock::open(&candidate.part) {
        Some(lock) if lock.is_held() => {
            info!(part = %candidate.part.display(), "a part file without a usable resume file; starting over");
            candidate.lock = lock;
            Look::Take(Plan::Planned(candidate, None))
        }
        _ => Look::Occupied,
    }
}

/// Whether `saved` can continue as the download `file` describes, with its part
/// file at `target` still the size of the whole file.
fn continues(saved: &ResumeState, target: &Target, file: &Described) -> Result<bool> {
    Ok(saved.can_resume(file.ranged, file.total, &file.validators)
        && part_len(&target.part)? == file.total)
}

/// The resume file at `path`, if there is one this engine can use. A damaged one is proof its data cannot be trusted, so it is logged and the download starts over. A read that failed proves nothing about the file (a virus scanner may hold it), so it ends the run with the files as they are.
fn saved_state(path: &Path) -> Result<Option<ResumeState>> {
    match ResumeState::load(path) {
        Ok(state) => Ok(Some(state)),
        Err(LoadError::Read(e)) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(LoadError::Read(e)) => Err(e).context(IoAction::ReadResume, path),
        Err(e) => {
            warn!(path = %path.display(), error = %Chain(&e), "the resume file cannot be used; starting over");
            Ok(None)
        }
    }
}

/// The length of the part file at `path`, if there is one.
fn part_len(path: &Path) -> Result<Option<u64>> {
    match std::fs::metadata(path) {
        Ok(metadata) => Ok(Some(metadata.len())),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e).context(IoAction::OpenFile, path),
    }
}

fn remove_resume_file(path: &Path) -> Result<()> {
    match std::fs::remove_file(path) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => {
            Err(e).context(IoAction::DeleteResume, path)
        }
        _ => Ok(()),
    }
}

/// Deletes a part or resume file of a finished download or a given-back name, if there is one. A failure is only logged: the download's data is safe by then.
pub(crate) fn discard(path: &Path) {
    match std::fs::remove_file(path) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => {
            warn!(path = %path.display(), error = %Chain(&e), "could not delete a file that is no longer needed");
        }
    }
}

/// Whether a resume file's URL is `url`. Resume files written before
/// passwords were left out of them still match, so they are continued once
/// and rewritten without the password straight away.
fn same_download(saved: &str, url: &str) -> bool {
    saved == url || Url::parse(saved).is_ok_and(|saved| without_password(&saved).as_str() == url)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::abandon::Abandonment;

    fn in_folder(dir: &Path) -> Destination {
        Destination::InFolder {
            dir: dir.to_path_buf(),
            name: "f.bin".into(),
        }
    }

    /// What a server said about a ten-byte file that takes ranges.
    fn ten_bytes() -> Described {
        Described {
            ranged: true,
            total: Some(10),
            validators: Validators::default(),
        }
    }

    /// Downloads given one name in one folder each take their own part file, and nothing appears under the final names until a download finishes.
    #[test]
    fn downloads_of_one_name_in_one_folder_never_share_a_file() {
        let dir = wirecatch_test_support::private_dir("prepare-reserve").unwrap();
        let abandonment = Abandonment::new();
        let first = in_folder(&dir)
            .prepare("https://a/f.bin", &ten_bytes(), 1, &abandonment.flag())
            .unwrap();
        let second = in_folder(&dir)
            .prepare("https://b/f.bin", &ten_bytes(), 1, &abandonment.flag())
            .unwrap();
        assert_eq!(first.target.file, dir.join("f.bin"));
        assert_eq!(second.target.file, dir.join("f (1).bin"));
        assert_eq!(
            wirecatch_test_support::files_in(&dir),
            ["f (1).bin.wcpart", "f.bin.wcpart"]
        );
        dir.remove().unwrap();
    }

    #[test]
    fn a_paused_download_into_a_folder_is_continued() {
        let dir = wirecatch_test_support::private_dir("prepare-continue").unwrap();
        let abandonment = Abandonment::new();
        let file = ten_bytes();
        let url = "https://a/f.bin";
        let first = in_folder(&dir)
            .prepare(url, &file, 1, &abandonment.flag())
            .unwrap();
        ResumeState::new(
            url,
            file.total,
            file.ranged,
            &file.validators,
            first.table.remaining(),
        )
        .save(&first.target.state, &abandonment.flag())
        .unwrap();
        let path = first.target.file.clone();
        drop(first);
        let again = in_folder(&dir)
            .prepare(url, &file, 1, &abandonment.flag())
            .unwrap();
        assert!(again.resumed);
        assert_eq!(again.target.file, path);
        dir.remove().unwrap();
    }

    fn save_state(prepared: &Prepared, url: &str, file: &Described, abandonment: &Abandonment) {
        ResumeState::new(
            url,
            file.total,
            file.ranged,
            &file.validators,
            prepared.table.remaining(),
        )
        .save(&prepared.target.state, &abandonment.flag())
        .unwrap();
    }

    #[test]
    fn another_links_partial_file_is_left_alone() {
        let dir = wirecatch_test_support::private_dir("prepare-other-link").unwrap();
        let abandonment = Abandonment::new();
        let file = ten_bytes();
        let theirs = in_folder(&dir)
            .prepare("https://a/f.bin", &file, 1, &abandonment.flag())
            .unwrap();
        save_state(&theirs, "https://a/f.bin", &file, &abandonment);
        let (part, state) = (theirs.target.part.clone(), theirs.target.state.clone());
        drop(theirs);
        std::fs::write(&part, b"their data").unwrap();
        let saved = std::fs::read(&state).unwrap();

        let ours = in_folder(&dir)
            .prepare("https://b/f.bin", &file, 1, &abandonment.flag())
            .unwrap();
        assert_eq!(ours.target.file, dir.join("f (1).bin"));
        assert_eq!(std::fs::read(&part).unwrap(), b"their data");
        assert_eq!(std::fs::read(&state).unwrap(), saved);
        drop(ours);
        dir.remove().unwrap();
    }

    /// The first run took a number because the name was taken: the next run finds it there.
    #[test]
    fn a_run_under_a_numbered_name_is_continued() {
        let dir = wirecatch_test_support::private_dir("prepare-numbered").unwrap();
        std::fs::write(dir.join("f.bin"), b"someone else's").unwrap();
        let abandonment = Abandonment::new();
        let file = ten_bytes();
        let url = "https://a/f.bin";
        let first = in_folder(&dir)
            .prepare(url, &file, 1, &abandonment.flag())
            .unwrap();
        assert_eq!(first.target.file, dir.join("f (1).bin"));
        save_state(&first, url, &file, &abandonment);
        drop(first);
        let again = in_folder(&dir)
            .prepare(url, &file, 1, &abandonment.flag())
            .unwrap();
        assert!(again.resumed);
        assert_eq!(again.target.file, dir.join("f (1).bin"));
        drop(again);

        // The file that took the name moved away: the run is still under its number.
        std::fs::remove_file(dir.join("f.bin")).unwrap();
        let moved = in_folder(&dir)
            .prepare(url, &file, 1, &abandonment.flag())
            .unwrap();
        assert!(moved.resumed);
        assert_eq!(moved.target.file, dir.join("f (1).bin"));
        drop(moved);
        dir.remove().unwrap();
    }

    /// A second copy of a command that is still running gets a name of its own, and leaves the first one's files alone.
    #[test]
    fn a_running_download_keeps_its_files_from_a_second_copy() {
        let dir = wirecatch_test_support::private_dir("prepare-second-copy").unwrap();
        let abandonment = Abandonment::new();
        let file = ten_bytes();
        let url = "https://a/f.bin";
        let first = in_folder(&dir)
            .prepare(url, &file, 1, &abandonment.flag())
            .unwrap();
        ResumeState::new(
            url,
            file.total,
            file.ranged,
            &file.validators,
            first.table.remaining(),
        )
        .save(&first.target.state, &abandonment.flag())
        .unwrap();
        let second = in_folder(&dir)
            .prepare(url, &file, 1, &abandonment.flag())
            .unwrap();
        assert!(!second.resumed);
        assert_eq!(second.target.file, dir.join("f (1).bin"));
        drop((first, second));
        let third = in_folder(&dir)
            .prepare(url, &file, 1, &abandonment.flag())
            .unwrap();
        assert!(third.resumed, "the run that ended is continued");
        assert_eq!(third.target.file, dir.join("f.bin"));
        drop(third);
        dir.remove().unwrap();
    }

    /// A download whose part file cannot be sized gives its name back.
    #[test]
    fn a_part_file_that_cannot_be_made_leaves_nothing_behind() {
        let dir = wirecatch_test_support::private_dir("prepare-no-room").unwrap();
        let abandonment = Abandonment::new();
        let file = Described {
            total: Some(u64::MAX),
            ..ten_bytes()
        };
        let error = in_folder(&dir)
            .prepare("https://a/f.bin", &file, 1, &abandonment.flag())
            .err()
            .expect("no file system takes a part file of u64::MAX bytes");
        assert!(matches!(error, Error::Io { .. }), "{error:?}");
        assert_eq!(wirecatch_test_support::files_in(&dir), Vec::<String>::new());
        dir.remove().unwrap();
    }

    /// Dropped while its folder was being created, a download deletes and creates nothing: the files may already be a restarted run's.
    #[test]
    fn a_download_dropped_while_its_folder_is_made_touches_no_file() {
        let dir = wirecatch_test_support::private_dir("prepare-dropped").unwrap();
        let path = dir.join("f.bin");
        let (part, state) = naming::part_paths(&path);
        std::fs::write(&part, b"other run").unwrap();
        std::fs::write(&state, b"other run").unwrap();
        let abandonment = Abandonment::new();
        let abandoned = abandonment.flag();
        abandoned.drop_after_slow_step();
        let file = Described {
            ranged: true,
            total: Some(10),
            validators: Validators::default(),
        };
        let prepared = Destination::Exact(path, ExistingFile::Replace).prepare(
            "https://x/f",
            &file,
            1,
            &abandoned,
        );
        assert!(matches!(prepared, Err(Error::Cancelled)));
        assert_eq!(std::fs::read(&part).unwrap(), b"other run");
        assert_eq!(std::fs::read(&state).unwrap(), b"other run");
        drop(abandonment);
        dir.remove().unwrap();
    }

    /// A folder where the resume file should be: reading it fails the way a held file does.
    #[test]
    fn a_resume_file_that_cannot_be_read_ends_the_run_and_keeps_the_data() {
        let dir = wirecatch_test_support::private_dir("unreadable-state").unwrap();
        let path = dir.join("f.bin");
        let (part, state) = naming::part_paths(&path);
        std::fs::create_dir(&state).unwrap();
        std::fs::write(&part, b"0123456789").unwrap();
        let abandonment = Abandonment::new();
        for destination in [
            Destination::Exact(path.clone(), ExistingFile::Replace),
            in_folder(&dir),
        ] {
            let error = destination
                .prepare("https://x/f", &ten_bytes(), 1, &abandonment.flag())
                .err()
                .expect("a read that failed is not a fresh start");
            assert!(
                matches!(
                    error,
                    Error::Io {
                        action: IoAction::ReadResume,
                        ..
                    }
                ),
                "{error:?}"
            );
            assert_eq!(std::fs::read(&part).unwrap(), b"0123456789");
        }
        dir.remove().unwrap();
    }

    #[test]
    fn a_damaged_resume_file_is_logged_and_starts_over() {
        let dir = wirecatch_test_support::private_dir("damaged-state").unwrap();
        let state = dir.join("f.bin.wcstate");
        std::fs::write(&state, b"not a resume file").unwrap();
        let (logs, _capturing) = wirecatch_test_support::logs::Logs::capture();
        assert!(saved_state(&state).unwrap().is_none());
        let events = logs.events();
        assert!(
            events
                .iter()
                .any(|e| e.message == "the resume file cannot be used; starting over"),
            "{}",
            logs.text()
        );
        dir.remove().unwrap();
    }

    /// Readable but not deletable, while the part file has the wrong size: the new part file's zeros must never meet the old ranges.
    #[cfg(windows)]
    #[test]
    fn a_stale_resume_file_that_cannot_be_deleted_stops_the_start_over() {
        use std::os::windows::fs::OpenOptionsExt as _;
        const FILE_SHARE_READ: u32 = 1;

        let dir = wirecatch_test_support::private_dir("stale-state").unwrap();
        let path = dir.join("f.bin");
        let (part, state) = naming::part_paths(&path);
        let abandonment = Abandonment::new();
        let file = ten_bytes();
        ResumeState::new(
            "https://x/f",
            file.total,
            file.ranged,
            &file.validators,
            Vec::new(),
        )
        .save(&state, &abandonment.flag())
        .unwrap();
        std::fs::write(&part, b"cut").unwrap();
        let held = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ)
            .open(&state)
            .unwrap();
        let error = Destination::Exact(path.clone(), ExistingFile::Replace)
            .prepare("https://x/f", &file, 1, &abandonment.flag())
            .err()
            .expect("the old resume file could not be deleted");
        assert!(
            matches!(
                error,
                Error::Io {
                    action: IoAction::DeleteResume,
                    ..
                }
            ),
            "{error:?}"
        );
        assert_eq!(std::fs::read(&part).unwrap(), b"cut");
        drop(held);
        dir.remove().unwrap();
    }
}

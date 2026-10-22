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
use crate::storage::Finish;
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
    /// replaced ([`Finish::FreeName`]). An earlier run under `name` is
    /// continued only if it had the same URL.
    InFolder { dir: PathBuf, name: String },
}

/// What the server said about the file, as far as continuing goes.
pub(crate) struct Described {
    pub ranged: bool,
    pub total: Option<u64>,
    pub validators: Validators,
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
                let (target, resume) = plan_exact(path, finish, file);
                Plan::Planned(target, resume.map(Box::new))
            }
            Self::InFolder { dir, name } => {
                std::fs::create_dir_all(&dir).context(IoAction::CreateFolder, &dir)?;
                plan_in_folder(dir, name, saved_url, file)
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
            Plan::Planned(target, None) => {
                // Whatever is left there cannot continue: start over.
                discard(&target.part);
                discard(&target.state);
                storage::create_part(&target.part, file.total)
                    .context(IoAction::CreateFile, &target.part)?;
                target
            }
            Plan::FreeName { dir, name } => {
                let (path, part) = naming::create_free_part(&dir, &name)
                    .context(IoAction::CreateFile, &dir.join(&name))?;
                let target = self::target(path, Finish::FreeName { dir, name });
                if let Err(e) = storage::size_part(&part, file.total) {
                    drop(part);
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
    }
}

/// [`Destination::Exact`]: the URL is not compared, only what the server says about the file.
fn plan_exact(path: PathBuf, finish: Finish, file: &Described) -> (Target, Option<ResumeState>) {
    let target = target(path, finish);
    let resume = saved_state(&target.state).filter(|saved| continues(saved, &target, file));
    (target, resume)
}

/// What [`Destination::prepare`] does once the folder is there.
enum Plan {
    /// These files, continuing the earlier run when there is a resume state
    /// (boxed: it is most of the plan's size and rarely there).
    Planned(Target, Option<Box<ResumeState>>),
    /// The first free name of `name` in `dir` ([`naming::create_free_part`]).
    FreeName { dir: PathBuf, name: String },
}

/// [`Destination::InFolder`]: `name`, continuing an earlier run of the same URL
/// there or starting it over, else the first free name.
fn plan_in_folder(dir: PathBuf, name: String, url: &str, file: &Described) -> Plan {
    let preferred = target(
        dir.join(&name),
        Finish::FreeName {
            dir: dir.clone(),
            name: name.clone(),
        },
    );
    if let Some(saved) = saved_state(&preferred.state)
        && same_download(&saved.url, url)
        && !preferred.file.exists()
    {
        if continues(&saved, &preferred, file) {
            return Plan::Planned(preferred, Some(Box::new(saved)));
        }
        // Same download, but it cannot continue (file changed, or no range support):
        // start over under the same name.
        info!("previous partial download cannot be resumed; starting over");
        return Plan::Planned(preferred, None);
    }
    Plan::FreeName { dir, name }
}

/// Whether `saved` can continue as the download `file` describes, with its part
/// file at `target` still the size of the whole file.
fn continues(saved: &ResumeState, target: &Target, file: &Described) -> bool {
    saved.can_resume(file.ranged, file.total, &file.validators)
        && part_len(&target.part) == file.total
}

/// The resume file at `path`, if there is one that can be read. One that
/// cannot is only logged: the download starts over and replaces it.
fn saved_state(path: &Path) -> Option<ResumeState> {
    match ResumeState::load(path) {
        Ok(state) => Some(state),
        Err(LoadError::Read(e)) if e.kind() == io::ErrorKind::NotFound => None,
        Err(e) => {
            warn!(path = %path.display(), error = %Chain(&e), "the resume file cannot be read; starting over");
            None
        }
    }
}

/// The length of the part file at `path`, if it can be read.
fn part_len(path: &Path) -> Option<u64> {
    match std::fs::metadata(path) {
        Ok(metadata) => Some(metadata.len()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => None,
        Err(e) => {
            warn!(path = %path.display(), error = %Chain(&e), "the part file cannot be read; starting over");
            None
        }
    }
}

/// Deletes a part or resume file that is of no more use, if there is one. A
/// failure is only logged: a part file left behind is emptied when the
/// download starts over, and a resume file is replaced or never matches again.
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
        let again = in_folder(&dir)
            .prepare(url, &file, 1, &abandonment.flag())
            .unwrap();
        assert!(again.resumed);
        assert_eq!(again.target.file, first.target.file);
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

    /// A logged error carries the system's reason, not only what the engine was doing.
    #[test]
    fn a_resume_file_that_cannot_be_read_is_logged_with_the_reason() {
        let dir = wirecatch_test_support::private_dir("unreadable-state").unwrap();
        // A folder where the resume file should be: opening works, reading does not.
        let state = dir.join("f.bin.wcstate");
        std::fs::create_dir(&state).unwrap();
        let reason = std::fs::read(&state).unwrap_err().to_string();
        let (logs, _capturing) = wirecatch_test_support::logs::Logs::capture();
        assert!(saved_state(&state).is_none());
        let logged = logs.text();
        assert!(logged.contains(&reason), "{logged}");
        dir.remove().unwrap();
    }
}

//! Running a media download: a stream through `wirecatch-streams`, or a page's
//! formats found again with yt-dlp (their addresses expire) and downloaded by
//! the engine or `wirecatch-streams`, then joined with ffmpeg. Formats neither
//! can download are left to yt-dlp itself.
//!
//! Parts live in `<file>.wcmedia/` until the file is complete, so pausing,
//! failing or restarting keeps them; a part finished in an earlier run is not
//! downloaded again.

mod extras;
mod network;
mod page;
mod placement;
mod save;
#[cfg(test)]
mod tests;
mod workdir;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use wirecatch_addons::Check;
use wirecatch_engine::{CancellationToken, DownloadOptions, FileWork, Url};
use wirecatch_media::MediaTools;
use wirecatch_media::describe;
use wirecatch_media::output::OutputJob;
use wirecatch_media::ytdlp::FormatId;
use wirecatch_media::{MediaJob, MediaProgress, Pick};
use wirecatch_protocol::DownloadId;
use wirecatch_protocol::app::{FixAction, UserError};
use wirecatch_sites::local_network::{self, ChosenBy};
use wirecatch_streams::{Choice, StreamProgress, StreamRequest};

use self::page::run_page;
use self::placement::{Reserve, Taken, finish, waiting_to_place};
use super::Manager;
use super::media::media_options;
use super::policy::RunPolicy;
use super::run::{MediaDone, RunEnd};
use super::state::{Gauge, State};
use crate::clock::Clock;
use crate::headers::Origin;

/// Everything a media run needs, copied out of the manager's state.
struct Job {
    output: PathBuf,
    url: Url,
    origin: Origin,
    /// Who chose the link, which decides the rule for the local network.
    chosen_by: ChosenBy,
    tools: Option<MediaTools>,
    options: DownloadOptions,
    temp_dir: PathBuf,
    timeout: Duration,
    /// The pauses before each automatic try with fresh addresses; see
    /// [`ManagerConfig::media_refresh_delays`](crate::ManagerConfig::media_refresh_delays).
    refresh_delays: Arc<[Duration]>,
    /// Tells the time of the next try, shown while waiting for it.
    clock: Arc<dyn Clock>,
    /// Reserves a path for the finished file among the paths other downloads
    /// hold; `false` if one of them holds it already.
    reserve: Reserve,
    /// The run's, shared by its engine downloads.
    file_work: FileWork,
    /// What happens when its name is taken by a file in the folder (D96).
    taken: Taken,
}

impl Manager {
    /// Starts media download `id` (the caller checked every limit).
    pub(super) fn start_media_run(
        &self,
        state: &mut State,
        id: DownloadId,
        run_id: u64,
        answered: CancellationToken,
        job: MediaJob,
        policy: RunPolicy,
    ) {
        let Some(entry) = state.downloads.get(&id) else {
            return;
        };
        let url = match Url::parse(&entry.row.url) {
            Ok(url) => url,
            Err(e) => {
                self.fail_to_start(state, id, wirecatch_messages::invalid_link(&e.to_string()));
                return;
            }
        };
        if local_network::refuses_address(entry.chosen_by, &url) {
            let error = wirecatch_messages::private_network(entry.chosen_by);
            self.fail_to_start(state, id, error);
            return;
        }
        let mut options = media_options(
            &entry.origin,
            entry.chosen_by.private_network(),
            policy.proxy,
        );
        options.connections = policy.connections;
        options.rate_limiters = policy.limiters;
        options.retry_backoff = policy.retry_backoff;
        let tools = state.addon.tools.clone();
        let manager = Arc::downgrade(&self.inner);
        let work = Job {
            output: entry.path().to_path_buf(),
            url,
            origin: entry.origin.clone(),
            chosen_by: entry.chosen_by,
            tools: state.addon.tools.clone(),
            options,
            temp_dir: self.inner.temp_dir.clone(),
            timeout: self.inner.media_timeout,
            refresh_delays: self.inner.media_refresh_delays.clone(),
            clock: self.inner.clock.clone(),
            reserve: {
                let manager = Arc::downgrade(&self.inner);
                Arc::new(move |path: &Path, take: bool| {
                    Manager::from_weak(&manager).is_some_and(|m| {
                        let mut state = m.lock();
                        if !take {
                            state.claimed.release(path);
                            true
                        } else if state.claimed.contains(path) {
                            false
                        } else {
                            state.claimed.reserve(path);
                            true
                        }
                    })
                })
            },
            file_work: FileWork::new(),
            taken: Taken::of(entry.on_existing, state.settings.when_file_exists),
        };
        self.launch(state, id, run_id, answered, move |cancel, file_work| {
            let work = Job { file_work, ..work };
            let progress = Arc::new(MediaProgress::default());
            let gauge = Gauge::Media(progress.clone());
            (gauge, async move {
                let end = run(work, job, &progress, &cancel).await;
                if tools.is_some_and(|t| t.is_broken())
                    && let Some(manager) = Manager::from_weak(&manager)
                {
                    // A program that would not start: check every file closely.
                    manager.spawn_addon_rescan(Check::Hashes);
                }
                RunEnd::Media(end)
            })
        });
    }
}

async fn run(
    job: Job,
    what: MediaJob,
    progress: &MediaProgress,
    cancel: &CancellationToken,
) -> Result<MediaDone, UserError> {
    let work = wirecatch_media::work_dir(&job.output);
    if let Some((file, sidecars)) = waiting_to_place(&work, &job.url).await {
        return finish(&file, &job, &work, &sidecars).await;
    }
    let result = match what {
        MediaJob::Stream { video, audio } => run_stream(&job, video, audio, progress, cancel).await,
        MediaJob::Page {
            video,
            audio,
            output,
            keep_ext,
        } => {
            let page = PageJob {
                video,
                audio,
                output: output.map(|o| *o),
                keep_ext,
            };
            run_page(&job, &page, progress, cancel).await
        }
        MediaJob::Unreadable => return Err(wirecatch_media::unreadable()),
    };
    match result {
        Err(_) if cancel.is_cancelled() => Ok(MediaDone::Paused),
        other => other,
    }
}

fn choice(pick: Pick) -> Choice {
    match pick {
        Pick::Auto => Choice::Auto,
        Pick::Track(id) => Choice::Track(id),
        Pick::Skip => Choice::Skip,
    }
}

async fn run_stream(
    job: &Job,
    video: Pick,
    audio: Pick,
    progress: &MediaProgress,
    cancel: &CancellationToken,
) -> Result<MediaDone, UserError> {
    // The stream is saved in the work folder first and put in place by
    // `finish`, which marks it as downloaded from the internet before it is
    // ever at its final name.
    let work = wirecatch_media::work_dir(&job.output);
    tokio::fs::create_dir_all(&work)
        .await
        .map_err(|e| disk(&e, &job.output))?;
    let staged = work.join(job.output.file_name().unwrap_or_else(|| "stream".as_ref()));
    let request = stream_request(job, &staged, video, audio);
    let stream = Arc::new(StreamProgress::default());
    progress.add_stream(stream.clone());
    match wirecatch_streams::download(request, stream, cancel.clone()).await {
        Ok(wirecatch_streams::Outcome::Completed { path, .. }) => {
            finish(&path, job, &work, &[]).await
        }
        Ok(_) | Err(wirecatch_streams::Error::Cancelled) => Ok(MediaDone::Paused),
        Err(e) => Err(describe::stream(
            &e,
            job.output.parent().unwrap_or(Path::new("")),
            job.tools.as_ref(),
            job.chosen_by,
        )),
    }
}

/// What stream job `job` asks of `wirecatch-streams`, saving at `staged`.
fn stream_request(job: &Job, staged: &Path, video: Pick, audio: Pick) -> StreamRequest {
    let mut request = StreamRequest::new(job.url.clone(), staged, job.chosen_by.private_network());
    request.video = choice(video);
    request.audio = choice(audio);
    request.ffmpeg = job.tools.as_ref().map(|t| t.ffmpeg.clone());
    request.options = job.options.clone();
    request.file_work = job.file_work.clone();
    request
}

/// A part's role, which names its file in the work folder.
const ROLES: [&str; 2] = ["video", "audio"];

/// Why one attempt at a page's video did not finish.
#[derive(Debug)]
enum PageError {
    /// The server refused a format's address or said it expired (`status`,
    /// when it answered with one). Reading the page again gives fresh
    /// addresses, which usually work at once.
    Refused {
        status: Option<u16>,
    },
    Failed(UserError),
}

impl From<UserError> for PageError {
    fn from(error: UserError) -> Self {
        PageError::Failed(error)
    }
}

/// What a page's video downloads and how it is saved.
struct PageJob {
    video: Option<FormatId>,
    audio: Option<FormatId>,
    output: Option<OutputJob>,
    keep_ext: Option<String>,
}

/// The error once every automatic try with fresh addresses was refused too.
fn refused_after_refreshing(status: Option<u16>) -> UserError {
    let answered = status
        .map(|s| format!(" (it answered {s})"))
        .unwrap_or_default();
    UserError {
        message: format!(
            "The video's server kept refusing to send the video{answered}, even though Wirecatch tried again automatically several times with fresh addresses from the page. Sites sometimes refuse one video for a while. Try again later; what was already downloaded is kept."
        ),
        fix: Some(FixAction::Retry),
    }
}

fn refresh(message: &str) -> UserError {
    UserError {
        message: format!("{message} Try again; Wirecatch reads the page again."),
        fix: Some(FixAction::Retry),
    }
}

fn disk(error: &std::io::Error, near: &Path) -> UserError {
    let folder = near.parent().unwrap_or(near);
    wirecatch_messages::disk(error, folder)
}

//! Media errors in words for a person, each with the fix to offer (D3, rule 4): the one place the media layer's typed errors become [`UserError`]s, and where an add-on program that could not be started marks the add-on for a fresh look.

#![forbid(unsafe_code)]

use std::path::Path;

use wirecatch_protocol::app::{FixAction, UserError};
use wirecatch_streams::process::RunError;
use wirecatch_streams::{Error as StreamError, Role};

use super::ffmpeg::FfmpegError;
use super::output::{OutputJob, VideoAction};
use super::ytdlp::YtDlpError;
use crate::addons::MediaTools;
use crate::user_error;

fn retry(message: String) -> UserError {
    UserError {
        message,
        fix: Some(FixAction::Retry),
    }
}

/// A program of the add-on could not be started: the add-on is looked at again, and the person told to reinstall it.
fn cannot_start(tools: &MediaTools, program: &str, error: &std::io::Error) -> UserError {
    tools.mark_broken();
    retry(format!(
        "The media add-on's {program} could not be started ({error}). Reinstall the media add-on in Settings, Add-ons, then try again."
    ))
}

/// Why yt-dlp could not read a page or download from it.
pub(crate) fn ytdlp(error: &YtDlpError, tools: &MediaTools) -> UserError {
    match error {
        YtDlpError::Folder { source, .. } => retry(format!(
            "Wirecatch could not prepare a folder for the media add-on ({source}). Check that the disk has free space, then try again."
        )),
        YtDlpError::Cookies(source) => retry(format!(
            "Wirecatch could not prepare the page's sign-in for the media add-on ({source}). Check that the disk has free space, then try again."
        )),
        YtDlpError::Start(source) => cannot_start(tools, "yt-dlp", source),
        YtDlpError::Cancelled => retry("Reading the page was stopped. Try again.".into()),
        YtDlpError::TimedOut(limit) => retry(format!(
            "The media add-on took more than {} seconds to read this page. Check your connection, then try again.",
            limit.as_secs()
        )),
        YtDlpError::Stalled(_) => retry("The media add-on stopped answering. Try again.".into()),
        YtDlpError::Refused { reason } => retry(format!(
            "The media add-on could not get the video from this page ({reason}). Check that the video plays in your browser; if it does, update the media add-on in Settings, Add-ons, then try again."
        )),
        YtDlpError::BadOutput(e) => {
            tracing::warn!(error = %e, "yt-dlp printed something that is not a page description");
            retry("The media add-on could not read this page. Update the media add-on in Settings, Add-ons, then try again.".into())
        }
        // The page is as long the next time, so trying again does not help.
        YtDlpError::OutputTooLarge => UserError {
            message: "The media add-on described this page at a length Wirecatch does not read, so this video cannot be downloaded from it.".into(),
            fix: None,
        },
        YtDlpError::Task(error) => retry(format!(
            "Part of the media add-on's run stopped unexpectedly ({error}). Try again."
        )),
        YtDlpError::NothingSaved => {
            retry("The media add-on finished without saving the video. Try again.".into())
        }
    }
}

/// Why a stream download or inspection failed, with the fix where one helps. An ffmpeg of the add-on (`tools`) that could not be started marks the add-on for a fresh look.
pub(crate) fn stream(error: &StreamError, folder: &Path, tools: Option<&MediaTools>) -> UserError {
    use StreamError as E;
    if let (E::FfmpegMissing(_) | E::FfmpegRun(RunError::Start { .. }), Some(tools)) =
        (error, tools)
    {
        tools.mark_broken();
    }
    let with = |fix: Option<FixAction>| UserError {
        message: error.to_string(),
        fix,
    };
    match error {
        E::Request(e) => user_error::describe(e, folder, false),
        E::Io(e) => user_error::disk(e, folder),
        // A track that is not a real video or audio file: the link most likely does not point at the video any more.
        E::UnknownFormat { role } => unknown_format(*role),
        // A fresh link lists as many segments, as long.
        E::Live
        | E::Drm(_)
        | E::Unsupported(_)
        | E::PrivateNetwork
        | E::NothingChosen
        | E::TooManySegments
        | E::TooLarge => with(None),
        E::NotAStream
        | E::Manifest { .. }
        | E::Key { .. }
        | E::Decrypt { .. }
        | E::NoSuchTrack { .. } => with(Some(FixAction::RefreshLink)),
        _ => with(Some(FixAction::Retry)),
    }
}

/// A downloaded track is in no format ffmpeg may read: the link most likely does not point at the video any more, so a fresh one is the fix.
pub(crate) fn unknown_format(role: Role) -> UserError {
    UserError {
        message: StreamError::UnknownFormat { role }.to_string(),
        fix: Some(FixAction::RefreshLink),
    }
}

/// What every ffmpeg step words the same way, or `None` for a failure each step words itself.
fn ffmpeg_common(error: &FfmpegError, tools: &MediaTools) -> Option<UserError> {
    Some(match error {
        FfmpegError::Start(source) => cannot_start(tools, "ffmpeg", source),
        FfmpegError::UnknownFormat { role } => unknown_format(*role),
        FfmpegError::Unreadable { role, source } => retry(format!(
            "Wirecatch could not read the downloaded {} to join it ({source}). Try again; the downloaded parts are kept.",
            role.name()
        )),
        _ => return None,
    })
}

/// Why joining separately downloaded video and audio failed.
pub(crate) fn join(error: &FfmpegError, tools: &MediaTools) -> UserError {
    ffmpeg_common(error, tools).unwrap_or_else(|| match error {
        FfmpegError::Stalled(_) | FfmpegError::TimedOut(_) => {
            retry("ffmpeg stopped joining the video and audio. Try again.".into())
        }
        other => retry(format!(
            "ffmpeg could not join the video and audio ({other}). Try again; the downloaded parts are kept."
        )),
    })
}

/// Why saving a download as `output` says failed: the download is kept, Try again converts again, and Keep the original saves it as it was downloaded.
pub(crate) fn conversion(error: &FfmpegError, output: &OutputJob, tools: &MediaTools) -> UserError {
    if let Some(common) = ffmpeg_common(error, tools) {
        return common;
    }
    let what = if output.container.is_video() || output.video != VideoAction::None {
        "video"
    } else {
        "audio"
    };
    let message = if output.original {
        format!(
            "Wirecatch downloaded the {what}, but could not add the subtitles or chapters to it ({error}). The download is kept: Try again only does this step again, and Keep the original saves it without them."
        )
    } else {
        format!(
            "Wirecatch downloaded the {what}, but could not save it as {} ({error}). The download is kept: Try again only converts it again, and Keep the original saves it as it was downloaded ({}).",
            output.container.label(),
            output.original_ext.to_ascii_uppercase()
        )
    };
    UserError {
        message,
        fix: Some(FixAction::KeepOriginal),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use wirecatch_engine::Error as EngineError;

    fn tools() -> MediaTools {
        MediaTools::for_test(Path::new("ffmpeg"))
    }

    #[test]
    fn a_program_that_cannot_start_marks_the_add_on_for_a_fresh_look() {
        let tools = tools();
        let error = join(
            &FfmpegError::Start(std::io::Error::from(std::io::ErrorKind::NotFound)),
            &tools,
        );
        assert!(tools.is_broken());
        assert_eq!(error.fix, Some(FixAction::Retry));
        let tools = super::tests::tools();
        ytdlp(
            &YtDlpError::Start(std::io::Error::from(std::io::ErrorKind::NotFound)),
            &tools,
        );
        assert!(tools.is_broken());
    }

    #[test]
    fn every_media_error_reads_as_sentences_with_a_fix() {
        let tools = tools();
        let panicked = || {
            tokio::runtime::Builder::new_current_thread()
                .build()
                .unwrap()
                .block_on(async { tokio::spawn(async { panic!("a task that fails") }).await })
                .unwrap_err()
        };
        let ytdlp_errors = [
            (YtDlpError::Cancelled, Some(FixAction::Retry)),
            (
                YtDlpError::TimedOut(Duration::from_secs(60)),
                Some(FixAction::Retry),
            ),
            (
                YtDlpError::Stalled(Duration::from_secs(60)),
                Some(FixAction::Retry),
            ),
            (
                YtDlpError::Refused {
                    reason: "Video unavailable".into(),
                },
                Some(FixAction::Retry),
            ),
            (YtDlpError::NothingSaved, Some(FixAction::Retry)),
            (YtDlpError::Task(panicked()), Some(FixAction::Retry)),
            // The page is as long the next time.
            (YtDlpError::OutputTooLarge, None),
        ];
        for (error, fix) in &ytdlp_errors {
            let user = ytdlp(error, &tools);
            assert!(user.message.ends_with('.'), "{}", user.message);
            assert_eq!(&user.fix, fix, "{}", user.message);
        }
        let ffmpeg_errors = [
            FfmpegError::Stalled(Duration::from_secs(60)),
            FfmpegError::Task {
                role: Role::Video,
                error: panicked(),
            },
            FfmpegError::Path(wirecatch_streams::ffmpeg::file_argument(Path::new("")).unwrap_err()),
        ];
        let job = crate::media::output::test_support::job(
            crate::media::output::Container::Mp4,
            VideoAction::Copy,
            crate::media::output::AudioAction::Copy,
        );
        for error in &ffmpeg_errors {
            let joined = join(error, &tools);
            assert!(joined.message.ends_with('.'), "{}", joined.message);
            assert_eq!(joined.fix, Some(FixAction::Retry), "{}", joined.message);
            let converted = conversion(error, &job, &tools);
            assert!(converted.message.ends_with('.'), "{}", converted.message);
            assert_eq!(
                converted.fix,
                Some(FixAction::KeepOriginal),
                "{}",
                converted.message
            );
        }
        let unknown = join(&FfmpegError::UnknownFormat { role: Role::Audio }, &tools);
        assert_eq!(unknown.fix, Some(FixAction::RefreshLink));
        assert!(!tools.is_broken());
    }

    #[test]
    fn every_stream_error_reads_as_sentences_with_a_fix_where_one_helps() {
        use wirecatch_streams::Error as E;
        let folder = Path::new("/downloads");
        let errors = [
            (E::Live, None),
            (E::Drm(wirecatch_streams::DrmSystem::Widevine), None),
            (E::NotAStream, Some(FixAction::RefreshLink)),
            (
                E::Manifest {
                    host: Some("cdn.example".into()),
                    line: Some(3),
                    cause: wirecatch_streams::ManifestError::NoSegments,
                },
                Some(FixAction::RefreshLink),
            ),
            // A fresh link would list as many, or as long.
            (E::TooManySegments, None),
            (E::TooLarge, None),
            (E::NeedsFfmpeg, Some(FixAction::Retry)),
            (E::Ffmpeg("bad".into()), Some(FixAction::Retry)),
            (E::PrivateNetwork, None),
            (
                E::Unsupported(wirecatch_streams::Feature::SeveralPeriods),
                None,
            ),
            (E::Cancelled, Some(FixAction::Retry)),
            (
                E::UnknownFormat { role: Role::Video },
                Some(FixAction::RefreshLink),
            ),
        ];
        for (error, fix) in errors {
            let user = stream(&error, folder, None);
            assert_eq!(user.fix, fix, "{error:?}");
            assert!(user.message.ends_with('.'), "{}", user.message);
            assert!(!user.message.contains('\u{2014}'));
        }
        let network = stream(
            &E::Request(EngineError::HttpStatus {
                status: 404,
                url: "u".into(),
            }),
            folder,
            None,
        );
        assert_eq!(network.fix, Some(FixAction::RefreshLink));
    }

    #[test]
    fn a_stream_whose_ffmpeg_cannot_start_marks_the_add_on_for_a_fresh_look() {
        let folder = Path::new("/downloads");
        let start = || RunError::Start {
            program: "ffmpeg".into(),
            source: std::io::Error::from(std::io::ErrorKind::PermissionDenied),
        };
        for error in [
            StreamError::FfmpegMissing("ffmpeg".into()),
            StreamError::FfmpegRun(start()),
        ] {
            let tools = tools();
            stream(&error, folder, Some(&tools));
            assert!(tools.is_broken(), "{error:?}");
        }
        // ffmpeg that started and then stalled is not a broken add-on.
        let tools = tools();
        stream(
            &StreamError::FfmpegRun(RunError::Stalled(Duration::from_secs(300))),
            folder,
            Some(&tools),
        );
        assert!(!tools.is_broken());
    }
}

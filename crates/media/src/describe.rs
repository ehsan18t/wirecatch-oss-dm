//! Media errors in words for a person, each with the fix to offer (D3, rule 4): the one place the media layer's typed errors become [`UserError`]s, and where an add-on program that could not be started marks the add-on for a fresh look.

#![forbid(unsafe_code)]

use std::path::Path;

use wirecatch_protocol::app::{FixAction, UserError};
use wirecatch_streams::process::RunError;
use wirecatch_streams::{Error as StreamError, Role};

use super::choice::format_name;
use super::ffmpeg::FfmpegError;
use super::output::{OutputJob, VideoAction};
use super::ytdlp::{YtDlpError, needs_sign_in};
use crate::MediaTools;
use wirecatch_sites::local_network::ChosenBy;

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
        "The media add-on's {program} could not be started ({error}). Reinstall the media add-on in Settings > Media, then try again."
    ))
}

/// Why yt-dlp could not read a page or download from it.
pub fn ytdlp(error: &YtDlpError, tools: &MediaTools) -> UserError {
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
        YtDlpError::Refused { reason } | YtDlpError::AddressRefused { reason, .. } => {
            retry(ytdlp_refusal(reason).to_owned())
        }
        YtDlpError::BadOutput(e) => {
            tracing::warn!(error = %e, "yt-dlp printed something that is not a page description");
            retry("The media add-on could not read this page. Update the media add-on in Settings > Media, then try again.".into())
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

/// What yt-dlp's own words `reason` mean for a person; the words themselves are in the log. It reports causes only as text.
fn ytdlp_refusal(reason: &str) -> &'static str {
    let reason = reason.to_ascii_lowercase();
    let says = |words: &[&str]| words.iter().any(|w| reason.contains(w));
    if says(&["http error 5"]) {
        "The site is having trouble right now. Wait a while, then try again."
    } else if says(&["http error 404", "http error 410"]) {
        "This page could not be found. Check the link in your browser, then try again."
    } else if says(&["http error 401", "http error 403"]) {
        "The site refused the request for this video. Check that the video plays in your browser, then try again."
    } else if says(&["http error 429", "too many requests"]) {
        "The site has received too many requests from this computer. Wait a while, then try again."
    } else if needs_sign_in(Some(&reason)) {
        "This video asks you to sign in. Sign in on the site in your browser, then try again."
    } else if says(&["in your country", "in your region", "from your location"]) {
        "This video is not offered where you are."
    } else if says(&[
        "video unavailable",
        "is unavailable",
        "has been removed",
        "no longer available",
    ]) {
        "This video is no longer available on the site."
    } else if says(&["unsupported url", "no video formats"]) {
        "The media add-on found no video on this page. Check that the video plays in your browser; if it does, update the media add-on in Settings > Media, then try again."
    } else if says(&[
        "timed out",
        "unable to connect",
        "getaddrinfo",
        "connection",
    ]) {
        "The media add-on could not reach the site. Check your connection, then try again."
    } else {
        "The media add-on could not get the video from this page. Check that the video plays in your browser; if it does, update the media add-on in Settings > Media, then try again."
    }
}

/// What ffmpeg's own words `reason` mean, after a colon, when they say something a person can use; the words themselves are in the log.
fn ffmpeg_cause(reason: &str) -> &'static str {
    let reason = reason.to_ascii_lowercase();
    let says = |words: &[&str]| words.iter().any(|w| reason.contains(w));
    if says(&["no space left"]) {
        ": the disk is full"
    } else if says(&["permission denied"]) {
        ": it has no permission to write the file"
    } else if says(&["invalid data found", "moov atom not found"]) {
        ": the downloaded file is damaged"
    } else if says(&["not currently supported in container"]) {
        ": this file type cannot hold what was downloaded"
    } else {
        ""
    }
}

/// How `error`, from a step of ffmpeg's, ends the sentence that says the step failed.
fn ffmpeg_failure(error: &FfmpegError) -> String {
    match error {
        FfmpegError::Failed { reason } => ffmpeg_cause(reason).to_owned(),
        other => format!(" ({other})"),
    }
}

/// Why a stream download or inspection failed, with the fix where one helps. An ffmpeg of the add-on (`tools`) that could not be started marks the add-on for a fresh look. `chosen_by` chose the stream's link, which words a refusal to reach the local network.
#[must_use]
pub fn stream(
    error: &StreamError,
    folder: &Path,
    tools: Option<&MediaTools>,
    chosen_by: ChosenBy,
) -> UserError {
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
        E::Request(e) => wirecatch_messages::describe(e, folder, wirecatch_messages::Worked::Never),
        E::Io(e) => wirecatch_messages::disk(e, folder),
        // A track that is not a real video or audio file: the link most likely does not point at the video any more.
        E::UnknownFormat { role } => unknown_format(*role),
        E::PrivateNetwork => wirecatch_messages::private_network(chosen_by),
        E::Ffmpeg(said) => retry(format!(
            "ffmpeg could not join the video{}. The downloaded parts are kept; try again.",
            ffmpeg_cause(said)
        )),
        // A fresh link lists as many segments, as long.
        E::Live
        | E::Drm(_)
        | E::Unsupported(_)
        | E::NothingChosen
        | E::TooManySegments
        | E::TooLarge => with(None),
        E::NotAStream
        | E::Manifest { .. }
        | E::Key { .. }
        | E::Decrypt { .. }
        | E::NotMedia { .. }
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
#[must_use]
pub fn join(error: &FfmpegError, tools: &MediaTools) -> UserError {
    ffmpeg_common(error, tools).unwrap_or_else(|| match error {
        FfmpegError::Stalled(_) | FfmpegError::TimedOut(_) => {
            retry("ffmpeg stopped joining the video and audio. Try again.".into())
        }
        other => retry(format!(
            "ffmpeg could not join the video and audio{}. Try again; the downloaded parts are kept.",
            ffmpeg_failure(other)
        )),
    })
}

/// Why saving a download as `output` says failed: the download is kept, Try again converts again, and Keep the original saves it as it was downloaded.
#[must_use]
pub fn conversion(error: &FfmpegError, output: &OutputJob, tools: &MediaTools) -> UserError {
    if let Some(common) = ffmpeg_common(error, tools) {
        return common;
    }
    let what = if output.container.is_video() || output.video != VideoAction::None {
        "video"
    } else {
        "audio"
    };
    let failure = ffmpeg_failure(error);
    let message = if output.original {
        format!(
            "Wirecatch downloaded the {what}, but could not add the subtitles or chapters to it{failure}. The download is kept: Try again only does this step again, and Keep the original saves it without them."
        )
    } else {
        format!(
            "Wirecatch downloaded the {what}, but could not save it as {}{failure}. The download is kept: Try again only converts it again, and Keep the original saves it as it was downloaded ({}).",
            output.container.label(),
            format_name(&output.original_ext)
        )
    };
    UserError {
        message,
        fix: Some(FixAction::KeepOriginal),
    }
}

/// A media download reached a step that needs the add-on, which is not installed.
#[must_use]
pub fn addon_missing() -> UserError {
    retry("This needs the media add-on, which is not installed. Install it from Settings > Media, then try again.".into())
}

/// A video waited for the add-on longer than the app waits.
#[must_use]
pub fn addon_waited_too_long() -> UserError {
    retry("This video waited for the media add-on for a long time, so Wirecatch stopped waiting. Install the media add-on in Settings > Media, then try again.".into())
}

/// A video waited for the add-on, whose install failed.
#[must_use]
pub fn addon_not_installed() -> UserError {
    retry("This video needs the media add-on, which could not be installed. Install the media add-on in Settings > Media, then try again.".into())
}

/// The format addresses kept being refused (`status`, when the server answered with one), with fresh ones from the page each time.
#[must_use]
pub fn refused_after_refreshing(status: Option<u16>) -> UserError {
    let answered = status
        .map(|s| format!(" (it answered {s})"))
        .unwrap_or_default();
    retry(format!(
        "The video's server kept refusing to send the video{answered}, even though Wirecatch tried again automatically several times with fresh addresses from the page. Sites sometimes refuse one video for a while. Try again later; what was already downloaded is kept."
    ))
}

/// The page stopped listing a download's formats and offers nothing else of the `quality` chosen. Trying again cannot help.
#[must_use]
pub fn quality_gone(quality: &str) -> UserError {
    UserError {
        message: format!(
            "This page no longer offers this video at {quality}. Remove this download and add the video again from its page to choose another quality."
        ),
        fix: None,
    }
}

/// The page gave a format an address that is not a valid link.
#[must_use]
pub fn invalid_format_address() -> UserError {
    retry("The page gave an address for the video that is not valid. Try again; Wirecatch reads the page again.".into())
}

/// The name of a format's server did not resolve, so where it leads could not be checked (D39).
#[must_use]
pub fn format_server_not_found() -> UserError {
    retry("The server of this video's file could not be found, so Wirecatch could not check where it is and did not download it. Check your connection, then try again.".into())
}

/// A step of the media run stopped without an answer (`detail`), which is a defect in the app.
#[must_use]
pub fn internal(detail: &dyn std::fmt::Display) -> UserError {
    retry(format!(
        "Something went wrong inside Wirecatch ({detail}). Try again."
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use wirecatch_engine::Error as EngineError;
    use wirecatch_streams::process::StderrTail;

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

    /// What the add-on's programs print goes to the log; a person reads what it means.
    #[test]
    fn a_program_failure_shows_no_raw_output() {
        let tools = tools();
        for (said, means) in [
            (
                "[generic] 1: Unable to download webpage: HTTP Error 404: Not Found (caused by <HTTPError 404: 'Not Found'>)",
                "could not be found",
            ),
            ("[site] x: HTTP Error 403: Forbidden", "refused"),
            ("[youtube] x: Sign in to confirm your age", "sign in"),
            (
                "[youtube] x: Private video. Sign in if you have access",
                "sign in",
            ),
            ("[youtube] x: Video unavailable", "no longer available"),
            ("HTTP Error 503: Service Unavailable", "having trouble"),
            (
                "[site] x: This video is not available in your country",
                "where you are",
            ),
            (
                "[youtube] x: Video unavailable. It is not available in your country",
                "where you are",
            ),
            (
                "Unsupported URL: https://example.com/page",
                "found no video",
            ),
            ("[site] x: No video formats found!", "found no video"),
            (
                "Unable to download webpage: <urlopen error timed out>",
                "connection",
            ),
            ("HTTP Error 429: Too Many Requests", "too many requests"),
            (
                "something no one has seen before",
                "could not get the video",
            ),
        ] {
            let shown = ytdlp(
                &YtDlpError::Refused {
                    reason: said.into(),
                },
                &tools,
            )
            .message;
            assert!(shown.to_lowercase().contains(means), "{said}: {shown}");
            for raw in [
                "[generic]",
                "[site]",
                "[youtube]",
                "caused by",
                "HTTPError",
                "urlopen",
                "https://",
            ] {
                assert!(!shown.contains(raw), "{said}: {shown}");
            }
            assert!(shown.ends_with('.'), "{shown}");
        }
    }

    /// ffmpeg's last line is rarely the cause: the table reads all it kept.
    #[test]
    fn the_cause_is_found_anywhere_in_what_ffmpeg_wrote() {
        let tools = tools();
        let error = FfmpegError::Failed {
            reason: [
                "[mp4 @ 0x1] Could not find tag for codec rawvideo in stream #0, codec not currently supported in container",
                "[out#0/mp4 @ 0x2] Could not write header (incorrect codec parameters ?): Invalid argument",
                "Error opening output file a.mp4",
                "Conversion failed!",
                "Error while filtering",
                "Nothing was written into output file, because at least one of its streams received no packets.",
            ]
            .join(" / "),
        };
        let shown = join(&error, &tools).message;
        assert!(shown.contains("cannot hold"), "{shown}");
    }

    #[test]
    fn a_stream_join_and_a_conversion_show_no_raw_output() {
        let tools = tools();
        let said = "[mp4 @ 0x1] Error writing trailer: No space left on device";
        let joined = stream(
            &StreamError::Ffmpeg(said.into()),
            Path::new("folder"),
            Some(&tools),
            ChosenBy::Website,
        );
        assert!(!joined.message.contains("@ 0x"), "{}", joined.message);
        assert!(
            joined.message.contains("disk is full"),
            "{}",
            joined.message
        );
        let job = OutputJob {
            container: crate::output::Container::Mp4,
            video: VideoAction::Copy,
            audio: crate::output::AudioAction::Copy,
            subtitles: Vec::new(),
            subtitle_files: false,
            extras: crate::output::Extras::default(),
            duration_ms: None,
            original_ext: "mkv".into(),
            original: false,
        };
        let failed = FfmpegError::Failed {
            reason: said.into(),
        };
        let converted = conversion(&failed, &job, &tools);
        assert!(!converted.message.contains("@ 0x"), "{}", converted.message);
        assert!(
            converted.message.contains("disk is full"),
            "{}",
            converted.message
        );
    }

    #[test]
    fn a_failed_join_or_conversion_shows_no_raw_output() {
        let tools = tools();
        for (said, means) in [
            (
                "[out#0/mp4 @ 0x1234] Error writing trailer: No space left on device",
                "disk is full",
            ),
            (
                "[mp4 @ 0x1234] Could not open file: Permission denied",
                "permission",
            ),
            (
                "[mov @ 0x1234] moov atom not found: Invalid data found when processing input",
                "damaged",
            ),
            (
                "[mp4 @ 0x1] Could not find tag for codec vp9, codec not currently supported in container",
                "cannot hold",
            ),
            ("[aac @ 0x1] something no one has seen", ""),
        ] {
            let error = FfmpegError::Failed {
                reason: said.into(),
            };
            let shown = join(&error, &tools).message;
            assert!(shown.contains(means), "{said}: {shown}");
            assert!(!shown.contains("@ 0x"), "{said}: {shown}");
            assert!(shown.ends_with('.'), "{shown}");
        }
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
        let job = crate::output::test_support::job(
            crate::output::Container::Mp4,
            VideoAction::Copy,
            crate::output::AudioAction::Copy,
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
        let run_errors = [
            addon_missing(),
            addon_waited_too_long(),
            addon_not_installed(),
            refused_after_refreshing(Some(403)),
            refused_after_refreshing(None),
            invalid_format_address(),
            format_server_not_found(),
            internal(&"task panicked"),
        ];
        for error in &run_errors {
            assert!(error.message.ends_with('.'), "{}", error.message);
            assert!(!error.message.contains('\u{2014}'), "{}", error.message);
            assert_eq!(error.fix, Some(FixAction::Retry), "{}", error.message);
        }
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
            (
                E::NotMedia {
                    number: 2,
                    total: 3,
                },
                Some(FixAction::RefreshLink),
            ),
            // A fresh link would list as many, or as long.
            (E::TooManySegments, None),
            (E::TooLarge, None),
            (E::NeedsFfmpeg, Some(FixAction::Retry)),
            (E::Ffmpeg("bad".into()), Some(FixAction::Retry)),
            (E::PrivateNetwork, Some(FixAction::RefreshLink)),
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
            let user = stream(&error, folder, None, ChosenBy::Website);
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
            ChosenBy::Website,
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
            let _described = stream(&error, folder, Some(&tools), ChosenBy::Website);
            assert!(tools.is_broken(), "{error:?}");
        }
        // ffmpeg that started and then stalled is not a broken add-on.
        let tools = tools();
        let _described = stream(
            &StreamError::FfmpegRun(RunError::Stalled {
                quiet: Duration::from_secs(300),
                stderr: StderrTail::default(),
            }),
            folder,
            Some(&tools),
            ChosenBy::Website,
        );
        assert!(!tools.is_broken());
    }
}

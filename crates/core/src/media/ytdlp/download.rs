//! Letting yt-dlp download formats Wirecatch cannot download itself, and finding the file it saved.

#![forbid(unsafe_code)]

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use tracing::debug;
use wirecatch_engine::CancellationToken;
use wirecatch_engine::numeric::f64_to_u64;
use wirecatch_streams::process::{self, Output, RunError, RunOptions, STALL_TIMEOUT};

use super::cookies::CookiesFile;
use super::{Attempt, Context, YtDlpError, common_args, refused, without_cookies};
use crate::media::MediaProgress;
use crate::media::output::Container;

/// The longest extension a finished file of yt-dlp's may have.
const MAX_RESULT_EXT: usize = 10;

/// How a download by yt-dlp itself ended without failing.
#[derive(Debug)]
pub(crate) enum Fetched {
    Done(PathBuf),
    Cancelled,
}

/// The fields of yt-dlp's progress lines after [`PROGRESS_MARK`], in the order [`progress_report`] reads them.
const PROGRESS_FIELDS: &str =
    "%(progress.downloaded_bytes)s %(progress.total_bytes)s %(progress.total_bytes_estimate)s";

/// Prefix of the progress lines asked of yt-dlp.
const PROGRESS_MARK: &str = "wcprogress";

/// Lets yt-dlp download format selection `format` into `work` (for streams
/// Wirecatch cannot download itself), reporting progress into `progress`.
///
/// On YouTube it runs without the browser's cookies first, like [`extract`],
/// and with them only if yt-dlp failed that way; the parts it saved are kept.
pub(crate) async fn download(
    ctx: &Context<'_>,
    format: &str,
    merge_into: Option<Container>,
    work: &Path,
    progress: &MediaProgress,
    cancel: &CancellationToken,
) -> Result<Fetched, YtDlpError> {
    if let Some(anonymous) = without_cookies(ctx) {
        match download_once(&anonymous, format, merge_into, work, progress, cancel).await {
            Ok(fetched) => return Ok(fetched),
            Err(Attempt::NeedsSignIn(error)) => {
                debug!(%error, "downloading needs the browser's sign-in; trying with it");
            }
            Err(Attempt::Failed(error)) => return Err(error),
        }
    }
    download_once(ctx, format, merge_into, work, progress, cancel)
        .await
        .map_err(Attempt::into_error)
}

async fn download_once(
    ctx: &Context<'_>,
    format: &str,
    merge_into: Option<Container>,
    work: &Path,
    progress: &MediaProgress,
    cancel: &CancellationToken,
) -> Result<Fetched, Attempt> {
    let cookies = CookiesFile::create(ctx).await?;
    let args = download_args(ctx, cookies.as_ref(), format, merge_into, work);
    let mut printed = None::<PathBuf>;
    let result = process::run(
        &ctx.tools.ytdlp,
        &args,
        work,
        RunOptions::lines()
            .with_idle_timeout(STALL_TIMEOUT)
            .watching(work.to_path_buf())
            .with_environment(super::proxy_environment(ctx)),
        cancel,
        |output, line| {
            if let Some(report) = line
                .trim()
                .strip_prefix(PROGRESS_MARK)
                .filter(|_| output == Output::Stdout)
            {
                if let Some((done, total)) = progress_report(report) {
                    progress.set_own(done, total);
                }
            } else if let Some(path) = printed_output(output, line, work) {
                printed = Some(path);
            }
        },
    )
    .await;
    CookiesFile::delete(cookies).await;
    let ran = match result {
        Ok(ran) => ran,
        Err(RunError::Cancelled) => return Ok(Fetched::Cancelled),
        Err(e) => return Err(YtDlpError::from_run(e).into()),
    };
    if !ran.success() {
        return Err(refused(&ran));
    }
    let work = work.to_path_buf();
    let found = tokio::task::spawn_blocking(move || {
        printed
            .and_then(|p| confirmed_output(&p, &work))
            .or_else(|| largest_result(&work))
    })
    .await
    .map_err(YtDlpError::Task)?;
    found
        .map(Fetched::Done)
        .ok_or_else(|| YtDlpError::NothingSaved.into())
}

/// yt-dlp's arguments to download `format` of the page into `work` as
/// `media.<ext>`, reporting progress and printing the finished file's path.
fn download_args(
    ctx: &Context<'_>,
    cookies: Option<&CookiesFile>,
    format: &str,
    merge_into: Option<Container>,
    work: &Path,
) -> Vec<OsString> {
    let mut args = common_args(ctx, cookies);
    if let Some(container) = merge_into {
        args.push("--merge-output-format".into());
        args.push(container.ext().into());
    }
    let progress_template = format!("download:{PROGRESS_MARK} {PROGRESS_FIELDS}");
    for arg in [
        "-f",
        format,
        "--no-simulate",
        "--newline",
        "--progress",
        "--progress-template",
        &progress_template,
        "--print",
        "after_move:filepath",
        "-o",
    ] {
        args.push(arg.into());
    }
    args.push(work.join("media.%(ext)s").into_os_string());
    args.push("--".into());
    args.push(ctx.page.as_str().into());
    args
}

/// The bytes done and, when known, the total (exact, else estimated) from a
/// progress report yt-dlp printed after [`PROGRESS_MARK`]. Byte counts are
/// read as yt-dlp prints them; a report whose bytes done are not a count is
/// ignored rather than read as zero.
fn progress_report(report: &str) -> Option<(u64, Option<u64>)> {
    let mut numbers = report
        .split_whitespace()
        .map(|n| n.parse::<f64>().ok().and_then(f64_to_u64));
    let (done, total, estimate) = (
        numbers.next().flatten()?,
        numbers.next().flatten(),
        numbers.next().flatten(),
    );
    Some((done, total.or(estimate).filter(|t| *t > 0)))
}

/// The path of the finished file in a line yt-dlp printed (`--print
/// after_move:filepath`): from standard output only (a site's text in a
/// warning on standard error is never taken for it), a full path straight
/// inside `work` named `media.<ext>`, as the output template asks, with no
/// `..` in it. Anything else is not taken for the result.
fn printed_output(output: Output, line: &str, work: &Path) -> Option<PathBuf> {
    if output != Output::Stdout {
        return None;
    }
    let path = PathBuf::from(line.trim());
    let plain = path.is_absolute()
        && !path
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir));
    (plain && path.parent() == Some(work) && is_result_name(&path)).then_some(path)
}

/// Whether `path`'s file name is `media.<ext>`, the name the output template
/// gives the finished file (`ext` being letters and digits only, so no partial
/// or fragment file).
fn is_result_name(path: &Path) -> bool {
    path.file_name()
        .and_then(|n| n.to_str())
        .and_then(|n| n.strip_prefix("media."))
        .is_some_and(|ext| {
            (1..=MAX_RESULT_EXT).contains(&ext.len())
                && ext.bytes().all(|b| b.is_ascii_alphanumeric())
        })
}

/// `path` if it really is a file straight inside `work`, once links and the
/// like are resolved on both. Blocks on the disk.
fn confirmed_output(path: &Path, work: &Path) -> Option<PathBuf> {
    let real = std::fs::canonicalize(path).ok()?;
    let work = std::fs::canonicalize(work).ok()?;
    (real.parent() == Some(work.as_path()) && is_result_name(&real) && real.is_file())
        .then(|| path.to_path_buf())
}

/// The largest finished file yt-dlp left in `work` (not a partial one), checked
/// like a printed path. Blocks on the disk.
fn largest_result(work: &Path) -> Option<PathBuf> {
    std::fs::read_dir(work)
        .ok()?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| is_result_name(p))
        .filter_map(|p| confirmed_output(&p, work))
        .filter_map(|p| Some((std::fs::metadata(&p).ok()?.len(), p)))
        .max_by_key(|(len, _)| *len)
        .map(|(_, p)| p)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_result_path_is_taken_only_from_stdout_and_only_inside_the_work_folder() {
        let dir = tempfile::tempdir().unwrap();
        let work = dir.path().join("clip.mp4.wcmedia");
        std::fs::create_dir_all(&work).unwrap();
        let good = work.join("media.mp4");
        let line = good.to_string_lossy().into_owned();
        assert_eq!(
            printed_output(Output::Stdout, &line, &work),
            Some(good.clone())
        );
        // Standard error never names the result.
        assert_eq!(printed_output(Output::Stderr, &line, &work), None);
        let outside = dir.path().join("media.mp4");
        let climbing = work.join("..").join("media.mp4");
        let nested = work.join("sub").join("media.mp4");
        for bad in [
            outside.to_string_lossy().into_owned(),
            climbing.to_string_lossy().into_owned(),
            nested.to_string_lossy().into_owned(),
            work.join("media.mp4.part").to_string_lossy().into_owned(),
            work.join("media.f137.mp4").to_string_lossy().into_owned(),
            work.join("secret.txt").to_string_lossy().into_owned(),
            "media.mp4".to_string(),
            "WARNING: see https://example.com".to_string(),
        ] {
            assert_eq!(printed_output(Output::Stdout, &bad, &work), None, "{bad}");
        }
    }

    #[test]
    fn a_result_must_really_be_a_file_in_the_work_folder() {
        let dir = tempfile::tempdir().unwrap();
        let work = dir.path().join("work");
        std::fs::create_dir_all(&work).unwrap();
        // Not there yet.
        assert_eq!(confirmed_output(&work.join("media.mp4"), &work), None);
        std::fs::write(work.join("media.mp4"), b"video").unwrap();
        std::fs::write(work.join("media.mp4.part"), b"partial, but larger").unwrap();
        assert_eq!(
            confirmed_output(&work.join("media.mp4"), &work),
            Some(work.join("media.mp4"))
        );
        assert_eq!(largest_result(&work), Some(work.join("media.mp4")));
        // A folder by the right name is not a file.
        std::fs::create_dir(work.join("media.mkv")).unwrap();
        assert_eq!(confirmed_output(&work.join("media.mkv"), &work), None);
    }

    /// A link named like the result that points elsewhere is refused, whether printed or found by looking in the folder.
    #[cfg(unix)]
    #[test]
    fn a_link_out_of_the_work_folder_is_not_the_result() {
        let dir = tempfile::tempdir().unwrap();
        let work = dir.path().join("work");
        std::fs::create_dir_all(&work).unwrap();
        let secret = dir.path().join("secret.mp4");
        std::fs::write(&secret, b"someone else's file").unwrap();
        std::os::unix::fs::symlink(&secret, work.join("media.mp4")).unwrap();
        assert_eq!(confirmed_output(&work.join("media.mp4"), &work), None);
        assert_eq!(largest_result(&work), None);
    }

    /// Progress reports give the bytes done and the total, exact or else
    /// estimated; a report whose bytes done are not a count is ignored.
    #[test]
    fn progress_reports_are_read_as_counts() {
        assert_eq!(progress_report("10 20 NA"), Some((10, Some(20))));
        assert_eq!(progress_report("10 NA 30.0"), Some((10, Some(30))));
        assert_eq!(progress_report("10 NA NA"), Some((10, None)));
        assert_eq!(progress_report("10 0 0"), Some((10, None)));
        assert_eq!(progress_report("-5 20 NA"), None);
        assert_eq!(progress_report("NA 20 NA"), None);
        assert_eq!(progress_report(""), None);
    }
}

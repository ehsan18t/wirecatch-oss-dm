//! The browser's cookies for yt-dlp: a Netscape cookies file bound to the page's host, readable only by the current user, deleted as soon as yt-dlp is done with it.

#![forbid(unsafe_code)]

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use tracing::{debug, warn};
use wirecatch_engine::Url;

use super::{Context, YtDlpError};

/// How long the cookies given to yt-dlp stay valid: long enough for any download, and yt-dlp skips cookies without an expiry time.
const COOKIE_LIFETIME: Duration = Duration::from_hours(24);

/// A Netscape cookies file holding the browser's cookies for the page, deleted
/// when dropped.
pub(super) struct CookiesFile {
    pub(super) path: PathBuf,
    /// Already deleted, so dropping it does nothing.
    deleted: bool,
}

static COOKIE_FILES: AtomicU64 = AtomicU64::new(0);

impl CookiesFile {
    /// Writes the page's cookies for yt-dlp, off the async threads; `None` when there are no cookies to pass.
    pub(super) async fn create(ctx: &Context<'_>) -> Result<Option<Self>, YtDlpError> {
        let Some(text) = ctx.cookies.and_then(|c| netscape(ctx.page, c)) else {
            return Ok(None);
        };
        let dir = ctx.temp_dir.to_path_buf();
        let written = tokio::task::spawn_blocking(move || Self::write(&dir, &text))
            .await
            .map_err(YtDlpError::Task)?;
        written.map(Some).map_err(YtDlpError::Cookies)
    }

    /// Writes `text` into a new file in `dir` that only the current user can read. Blocks on the disk.
    fn write(dir: &Path, text: &str) -> std::io::Result<Self> {
        std::fs::create_dir_all(dir)?;
        let n = COOKIE_FILES.fetch_add(1, Ordering::Relaxed);
        let path = dir.join(format!("cookies-{}-{n}.txt", std::process::id()));
        let mut options = std::fs::File::options();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(OWNER_ONLY_MODE);
        }
        let mut file = options.open(&path)?;
        // Owned from here, so a failure below still deletes it.
        let cookies = Self {
            path,
            deleted: false,
        };
        crate::owner_only::restrict(&cookies.path)?;
        std::io::Write::write_all(&mut file, text.as_bytes())?;
        Ok(cookies)
    }

    /// Deletes the file as soon as yt-dlp is done with it, off the async threads.
    pub(super) async fn delete(cookies: Option<Self>) {
        let Some(mut cookies) = cookies else { return };
        match tokio::fs::remove_file(&cookies.path).await {
            Ok(()) => cookies.deleted = true,
            // Left to `Drop`, which tries again and logs.
            Err(e) => {
                debug!(path = %cookies.path.display(), error = %e, "could not delete a temporary cookies file yet");
            }
        }
    }
}

/// Read and write for the current user only.
#[cfg(unix)]
const OWNER_ONLY_MODE: u32 = 0o600;

/// Deleted when dropped: the fallback when a run is cancelled, panics or [`CookiesFile::delete`] could not delete it.
impl Drop for CookiesFile {
    fn drop(&mut self) {
        if self.deleted {
            return;
        }
        if let Err(e) = std::fs::remove_file(&self.path) {
            warn!(path = %self.path.display(), error = %e, "could not delete a temporary cookies file");
        }
    }
}

/// The `Cookie` header `header` as a Netscape cookies file for the page's host
/// only (not its other subdomains), or `None` when it holds no cookies.
pub(crate) fn netscape(page: &Url, header: &str) -> Option<String> {
    let host = page.host_str()?;
    let secure = if page.scheme() == "https" {
        "TRUE"
    } else {
        "FALSE"
    };
    let expires = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
        + COOKIE_LIFETIME.as_secs();
    // Line by line, joined at the end.
    let mut lines = vec!["# Netscape HTTP Cookie File".to_string()];
    for pair in header.split(';') {
        let Some((name, value)) = pair.split_once('=') else {
            continue;
        };
        let (name, value) = (name.trim(), value.trim());
        let clean = |s: &str| !s.is_empty() && !s.contains(['\t', '\r', '\n']);
        if !clean(name) || value.contains(['\t', '\r', '\n']) {
            continue;
        }
        lines.push(format!(
            "{host}\tFALSE\t/\t{secure}\t{expires}\t{name}\t{value}"
        ));
    }
    // The header line alone holds no cookies.
    (lines.len() > 1).then(|| {
        let mut text = lines.join("\n");
        text.push('\n');
        text
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cookies_are_bound_to_the_page_host() {
        let page = Url::parse("https://www.example.com/watch?v=1").unwrap();
        let text = netscape(&page, "a=1; b = two words;broken; c=x\ty").unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines[0], "# Netscape HTTP Cookie File");
        assert_eq!(lines.len(), 3);
        let fields: Vec<&str> = lines[1].split('\t').collect();
        assert_eq!(fields[..4], ["www.example.com", "FALSE", "/", "TRUE"]);
        assert_eq!(fields[5..], ["a", "1"]);
        assert!(lines[2].ends_with("\tb\ttwo words"));
        assert!(netscape(&page, "junk").is_none());
    }
}

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use super::convert::EncoderCatalog;
use wirecatch_addons::ToolPaths;

/// The media add-on's programs, as media runs them.
#[derive(Debug, Clone)]
pub(crate) struct MediaTools {
    pub ytdlp: PathBuf,
    pub ffmpeg: PathBuf,
    pub deno: PathBuf,
    pub ytdlp_cache: PathBuf,
    /// Set for every clone when a program could not be started (an antivirus took it, say), so
    /// the manager looks at the files again instead of trusting what it found.
    broken: Arc<AtomicBool>,
    /// Shared by every clone; asked again when the add-on is looked at again.
    encoders: Arc<EncoderCatalog>,
}

impl From<ToolPaths> for MediaTools {
    fn from(paths: ToolPaths) -> Self {
        Self {
            ytdlp: paths.ytdlp,
            ffmpeg: paths.ffmpeg,
            deno: paths.deno,
            ytdlp_cache: paths.ytdlp_cache,
            broken: Arc::default(),
            encoders: Arc::default(),
        }
    }
}

impl PartialEq for MediaTools {
    fn eq(&self, other: &Self) -> bool {
        (&self.ytdlp, &self.ffmpeg, &self.deno) == (&other.ytdlp, &other.ffmpeg, &other.deno)
    }
}

impl Eq for MediaTools {}

impl MediaTools {
    pub fn mark_broken(&self) {
        self.broken.store(true, Ordering::Relaxed);
    }

    pub fn encoders(&self) -> &EncoderCatalog {
        &self.encoders
    }

    pub fn is_broken(&self) -> bool {
        self.broken.load(Ordering::Relaxed)
    }

    #[cfg(test)]
    pub fn for_test(ffmpeg: &std::path::Path) -> MediaTools {
        ToolPaths {
            ytdlp: PathBuf::from("yt-dlp"),
            ffmpeg: ffmpeg.to_path_buf(),
            deno: PathBuf::from("deno"),
            ytdlp_cache: PathBuf::from("yt-dlp-cache"),
        }
        .into()
    }
}

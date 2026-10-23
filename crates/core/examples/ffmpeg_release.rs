//! Prints where the media add-on's ffmpeg comes from on this system, on one line: the archive's pinned address, its pinned SHA-256 and the program's path inside it. CI installs ffmpeg from it, so the tests run the build the app installs.
//!
//! ```text
//! cargo run -p wirecatch-core --example ffmpeg_release
//! ```

use std::io::Write as _;

use wirecatch_core::MediaAddonSources;

fn main() -> std::io::Result<()> {
    let ffmpeg = MediaAddonSources::official().ffmpeg;
    let release = ffmpeg.release();
    let program = ffmpeg
        .files()
        .iter()
        .find(|file| file.to == ffmpeg.program())
        .ok_or_else(|| {
            std::io::Error::other("no file of the ffmpeg archive installs the program")
        })?;
    writeln!(
        std::io::stdout(),
        "{} {} {}",
        release.url(),
        release.sha256(),
        program.from
    )
}

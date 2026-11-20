//! Prints where the media add-on's `ffmpeg` or `deno` comes from on this system, on one line: the archive's pinned address, its pinned SHA-256 and the program's path inside it. CI installs both from it, so the tests run the builds the app installs.
//!
//! ```text
//! cargo run -p wirecatch-core --example media_tool_release -- ffmpeg
//! ```

use std::io::Write as _;

use wirecatch_core::MediaAddonSources;

fn main() -> std::io::Result<()> {
    let sources = MediaAddonSources::official();
    let name = std::env::args_os().nth(1).unwrap_or_default();
    let name = name.to_string_lossy();
    let tool = match name.as_ref() {
        "ffmpeg" => sources.ffmpeg,
        "deno" => sources.deno,
        _ => return Err(std::io::Error::other("name the program: ffmpeg or deno")),
    };
    let release = tool.release();
    let program = tool
        .files()
        .iter()
        .find(|file| file.to == tool.program())
        .ok_or_else(|| {
            std::io::Error::other(format!(
                "no file of the {name} archive installs the program"
            ))
        })?;
    writeln!(
        std::io::stdout(),
        "{} {} {}",
        release.url(),
        release.sha256(),
        program.from
    )
}

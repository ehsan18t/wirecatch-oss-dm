//! A stand-in for the media add-on's programs, used by this crate's tests
//! (`cargo test` builds it). The tests install it as yt-dlp, ffmpeg and Deno
//! through the real add-on installer; it acts according to the name it runs
//! under, and is driven by files in a `fake-tools` folder next to the `addons`
//! folder it is installed in (so a test can prepare them before the install),
//! and tests never reach the internet:
//!
//! - `yt-dlp`: appends each call to `calls.jsonl` (its arguments, and the
//!   cookies file's text and Unix mode at that moment). With `-J` it prints
//!   `info.json`, or fails with the text of `fail.txt` if that exists (or of
//!   `fail-without-cookies.txt`, when it exists and the call has no
//!   `--cookies`), after waiting `delay-ms` milliseconds if that exists. Otherwise it "downloads":
//!   writes a file at its `-o` template (extension `mp4`, or the text of
//!   `download-ext`), prints two progress
//!   lines in the format Wirecatch asks for, and prints the file's path.
//! - `ffmpeg`: with `real-ffmpeg`, runs the program whose path that file
//!   holds with the same arguments, sharing the outputs, and ends as it does,
//!   so a test can use a real ffmpeg without installing a copy of it. Otherwise
//!   it checks every `-i file:<path>` input exists, waits
//!   `ffmpeg-delay-ms` milliseconds if that exists, then writes its arguments,
//!   one per line, into the output (its last argument, `file:<path>`). Asked
//!   for `-progress`, it first reports half of a two-second media done; a
//!   conversion (whose arguments, unlike a join's or a subtitle's, map
//!   chapters) fails with the text of `ffmpeg-fail.txt` if that exists.
//! - `deno`: prints a version.
//!
//! With `spawn-child` present, the `-J` call first starts a copy of itself that
//! writes a growing number to `heartbeat` every 50 ms until it is killed, as a
//! program yt-dlp starts would run.

use std::fmt::Display;
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

/// The exit code when standard output cannot be written, as when its reader
/// has gone.
const OUTPUT_FAILED: u8 = 4;
/// The exit code when the call cannot be logged.
const LOG_FAILED: u8 = 3;

/// How often the stand-in for a program yt-dlp started writes its heartbeat,
/// and how many beats it writes before it gives up waiting to be killed.
const HEARTBEAT: Duration = Duration::from_millis(50);
const MAX_BEATS: u32 = 1200;

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let [flag, heartbeat] = args.as_slice()
        && flag == "--fake-sleeper"
    {
        return beat(Path::new(heartbeat)).await;
    }
    let exe = std::env::current_exe().unwrap_or_default();
    let dir = exe
        .ancestors()
        .find(|a| a.file_name().is_some_and(|n| n == "addons"))
        .and_then(Path::parent)
        .map(|data| data.join("fake-tools"))
        .unwrap_or_default();
    let name = exe
        .file_stem()
        .map(|s| s.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    let ran = if name.starts_with("yt-dlp") {
        ytdlp(&dir, &args).await
    } else if name.starts_with("ffmpeg") {
        ffmpeg(&dir, &args).await
    } else {
        say("deno 2.9.7 (fake)").map(|()| ExitCode::SUCCESS)
    };
    ran.unwrap_or_else(|e| {
        eprintln!("fake tool: cannot write its output: {e}");
        ExitCode::from(OUTPUT_FAILED)
    })
}

/// Writes `line` to standard output. `println!` panics when the reader has
/// gone; this reports it, and the tool fails with [`OUTPUT_FAILED`].
fn say(line: impl Display) -> io::Result<()> {
    writeln!(io::stdout().lock(), "{line}")
}

/// A program yt-dlp started: it writes a growing number to `heartbeat` every
/// [`HEARTBEAT`] until it is killed. The test reads a heartbeat that stops as a
/// kill, so it stops early only when the test's folder is gone; any other
/// failed write skips that beat.
async fn beat(heartbeat: &Path) -> ExitCode {
    let mut beats = tokio::time::interval(HEARTBEAT);
    for beat in 0..MAX_BEATS {
        beats.tick().await;
        match std::fs::write(heartbeat, beat.to_string()) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => return ExitCode::SUCCESS,
            Err(e) => eprintln!("fake tool: cannot write {}: {e}", heartbeat.display()),
        }
    }
    ExitCode::SUCCESS
}

/// Waits the number of milliseconds written in `file`, if there is one.
async fn wait(file: &Path) {
    if let Some(ms) = std::fs::read_to_string(file)
        .ok()
        .and_then(|t| t.trim().parse::<u64>().ok())
    {
        tokio::time::sleep(Duration::from_millis(ms)).await;
    }
}

fn after<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    args.windows(2)
        .find(|w| w[0] == flag)
        .map(|w| w[1].as_str())
}

async fn ytdlp(dir: &Path, args: &[String]) -> io::Result<ExitCode> {
    let cookies = after(args, "--cookies").map(PathBuf::from);
    let text = cookies
        .as_ref()
        .and_then(|p| std::fs::read_to_string(p).ok());
    #[cfg(unix)]
    let mode = cookies.as_ref().and_then(|p| {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(p)
            .ok()
            .map(|m| m.permissions().mode() & 0o777)
    });
    #[cfg(not(unix))]
    let mode: Option<u32> = None;
    let call = serde_json::json!({
        "args": args,
        "cwd": std::env::current_dir().ok(),
        "cookies_path": cookies,
        "cookies": text,
        "cookies_mode": mode,
    });
    let log = dir.join("calls.jsonl");
    let mut lines = std::fs::read_to_string(&log).unwrap_or_default();
    lines.push_str(&call.to_string());
    lines.push('\n');
    if let Err(e) = std::fs::create_dir_all(dir).and_then(|()| std::fs::write(&log, lines)) {
        eprintln!(
            "ERROR: fake yt-dlp cannot log its call in {} ({e})",
            dir.display()
        );
        return Ok(ExitCode::from(LOG_FAILED));
    }

    if args.iter().any(|a| a == "-J") {
        if dir.join("spawn-child").exists()
            && let Ok(me) = std::env::current_exe()
        {
            // Left running on purpose: the test checks it is killed with this process.
            if let Err(e) = std::process::Command::new(me)
                .arg("--fake-sleeper")
                .arg(dir.join("heartbeat"))
                .spawn()
            {
                eprintln!("ERROR: fake yt-dlp cannot start its child ({e})");
            }
        }
        wait(&dir.join("delay-ms")).await;
        let unsigned = cookies
            .is_none()
            .then(|| std::fs::read_to_string(dir.join("fail-without-cookies.txt")).ok())
            .flatten();
        if let Some(reason) =
            unsigned.or_else(|| std::fs::read_to_string(dir.join("fail.txt")).ok())
        {
            eprintln!("WARNING: something minor");
            eprintln!("ERROR: {}", reason.trim());
            return Ok(ExitCode::from(1));
        }
        return match std::fs::read_to_string(dir.join("info.json")) {
            Ok(json) => say(json).map(|()| ExitCode::SUCCESS),
            Err(e) => {
                eprintln!("ERROR: fake yt-dlp has no info.json ({e})");
                Ok(ExitCode::from(2))
            }
        };
    }

    let Some(template) = after(args, "-o") else {
        eprintln!("ERROR: no output template");
        return Ok(ExitCode::from(2));
    };
    let format = after(args, "-f").unwrap_or("best");
    let ext = std::fs::read_to_string(dir.join("download-ext")).unwrap_or_else(|_| "mp4".into());
    let out = template.replace("%(ext)s", ext.trim());
    say("wcprogress 10 20 NA")?;
    say("wcprogress 20 20 NA")?;
    if std::fs::write(&out, format!("downloaded by fake yt-dlp: {format}")).is_err() {
        eprintln!("ERROR: cannot write {out}");
        return Ok(ExitCode::from(1));
    }
    say(out)?;
    Ok(ExitCode::SUCCESS)
}

async fn ffmpeg(dir: &Path, args: &[String]) -> io::Result<ExitCode> {
    if let Ok(real) = std::fs::read_to_string(dir.join("real-ffmpeg")) {
        return run_real(Path::new(real.trim()), args).await;
    }
    let Some(output) = args.last().and_then(|a| a.strip_prefix("file:")) else {
        eprintln!("fake ffmpeg: the last argument is not a file: output");
        return Ok(ExitCode::from(2));
    };
    for pair in args.windows(2) {
        if pair[0] == "-i" {
            let input = pair[1].strip_prefix("file:").unwrap_or(&pair[1]);
            if !Path::new(input).is_file() {
                eprintln!("fake ffmpeg: {input}: No such file or directory");
                return Ok(ExitCode::from(1));
            }
        }
    }
    if args.iter().any(|a| a == "-progress") {
        // Half of a two-second media done, as ffmpeg reports it.
        say("out_time_us=1000000")?;
        say("progress=continue")?;
    }
    wait(&dir.join("ffmpeg-delay-ms")).await;
    let converting = args.iter().any(|a| a == "-map_chapters");
    if converting && let Ok(reason) = std::fs::read_to_string(dir.join("ffmpeg-fail.txt")) {
        eprintln!("{}", reason.trim());
        return Ok(ExitCode::from(1));
    }
    match std::fs::write(output, args.join("\n")) {
        Ok(()) => Ok(ExitCode::SUCCESS),
        Err(e) => {
            eprintln!("fake ffmpeg: {output}: {e}");
            Ok(ExitCode::from(1))
        }
    }
}

/// Runs the program at `real` with `args`, sharing the outputs, and ends as it
/// does.
async fn run_real(real: &Path, args: &[String]) -> io::Result<ExitCode> {
    let status = tokio::process::Command::new(real)
        .args(args)
        .status()
        .await?;
    Ok(status
        .code()
        .and_then(|code| u8::try_from(code).ok())
        .map_or(ExitCode::FAILURE, ExitCode::from))
}

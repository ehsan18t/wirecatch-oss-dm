//! A stand-in for the media add-on's programs, used by this crate's tests
//! (`cargo test` builds it). The tests install it as yt-dlp, ffmpeg and Deno
//! through the real add-on installer; it acts according to the name it runs
//! under, and is driven by files in a `fake-tools` folder next to the `addons`
//! folder it is installed in (so a test can prepare them before the install),
//! and tests never reach the internet:
//!
//! - `yt-dlp`: appends each call to `calls.jsonl` (its arguments, and the
//!   cookies file's text and Unix mode at that moment). With `-J` it prints
//!   `info.json`, or fails with the text of `fail.txt` if that exists, after
//!   waiting `delay-ms` milliseconds if that exists. Otherwise it "downloads":
//!   writes a file at its `-o` template (extension `mp4`, or the text of
//!   `download-ext`), prints two progress
//!   lines in the format Wirecatch asks for, and prints the file's path.
//! - `ffmpeg`: checks every `-i file:<path>` input exists, waits
//!   `ffmpeg-delay-ms` milliseconds if that exists, then writes its arguments,
//!   one per line, into the output (its last argument, `file:<path>`).
//! - `deno`: prints a version.
//!
//! With `spawn-child` present, the `-J` call first starts a copy of itself that
//! writes a growing number to `heartbeat` every 50 ms until it is killed, as a
//! program yt-dlp starts would run.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let [flag, heartbeat] = args.as_slice()
        && flag == "--fake-sleeper"
    {
        // A program yt-dlp started: it beats until it is killed.
        for beat in 0..1200u32 {
            let _ = std::fs::write(heartbeat, beat.to_string());
            std::thread::sleep(Duration::from_millis(50));
        }
        return ExitCode::SUCCESS;
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
    if name.starts_with("yt-dlp") {
        ytdlp(&dir, &args)
    } else if name.starts_with("ffmpeg") {
        ffmpeg(&dir, &args)
    } else {
        println!("deno 2.9.7 (fake)");
        ExitCode::SUCCESS
    }
}

fn wait(file: &Path) {
    if let Some(ms) = std::fs::read_to_string(file)
        .ok()
        .and_then(|t| t.trim().parse::<u64>().ok())
    {
        std::thread::sleep(Duration::from_millis(ms));
    }
}

fn after<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    args.windows(2)
        .find(|w| w[0] == flag)
        .map(|w| w[1].as_str())
}

fn ytdlp(dir: &Path, args: &[String]) -> ExitCode {
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
    let _ = std::fs::create_dir_all(dir);
    let log = dir.join("calls.jsonl");
    let mut lines = std::fs::read_to_string(&log).unwrap_or_default();
    lines.push_str(&call.to_string());
    lines.push('\n');
    if std::fs::write(&log, lines).is_err() {
        return ExitCode::from(3);
    }

    if args.iter().any(|a| a == "-J") {
        if dir.join("spawn-child").exists()
            && let Ok(me) = std::env::current_exe()
        {
            let _ = std::process::Command::new(me)
                .arg("--fake-sleeper")
                .arg(dir.join("heartbeat"))
                .spawn();
        }
        wait(&dir.join("delay-ms"));
        if let Ok(reason) = std::fs::read_to_string(dir.join("fail.txt")) {
            eprintln!("WARNING: something minor");
            eprintln!("ERROR: {}", reason.trim());
            return ExitCode::from(1);
        }
        return match std::fs::read_to_string(dir.join("info.json")) {
            Ok(json) => {
                println!("{json}");
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("ERROR: fake yt-dlp has no info.json ({e})");
                ExitCode::from(2)
            }
        };
    }

    let Some(template) = after(args, "-o") else {
        eprintln!("ERROR: no output template");
        return ExitCode::from(2);
    };
    let format = after(args, "-f").unwrap_or("best");
    let ext = std::fs::read_to_string(dir.join("download-ext")).unwrap_or_else(|_| "mp4".into());
    let out = template.replace("%(ext)s", ext.trim());
    println!("wcprogress 10 20 NA");
    println!("wcprogress 20 20 NA");
    if std::fs::write(&out, format!("downloaded by fake yt-dlp: {format}")).is_err() {
        eprintln!("ERROR: cannot write {out}");
        return ExitCode::from(1);
    }
    println!("{out}");
    ExitCode::SUCCESS
}

fn ffmpeg(dir: &Path, args: &[String]) -> ExitCode {
    let Some(output) = args.last().and_then(|a| a.strip_prefix("file:")) else {
        eprintln!("fake ffmpeg: the last argument is not a file: output");
        return ExitCode::from(2);
    };
    for pair in args.windows(2) {
        if pair[0] == "-i" {
            let input = pair[1].strip_prefix("file:").unwrap_or(&pair[1]);
            if !Path::new(input).is_file() {
                eprintln!("fake ffmpeg: {input}: No such file or directory");
                return ExitCode::from(1);
            }
        }
    }
    wait(&dir.join("ffmpeg-delay-ms"));
    match std::fs::write(output, args.join("\n")) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("fake ffmpeg: {output}: {e}");
            ExitCode::from(1)
        }
    }
}

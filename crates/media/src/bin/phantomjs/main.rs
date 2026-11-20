//! `phantomjs`: the stand-in yt-dlp runs instead of PhantomJS (D281). It answers `-v` with PhantomJS's last version and runs a script in the Deno the app names, through `shim.js`, which gives it the PhantomJS APIs yt-dlp's `PhantomJSwrapper` uses. The script may read and write only the files directly in the run's own folder, which yt-dlp wrote for it; Deno gets no network, environment or programs.

#![forbid(unsafe_code)]

use std::ffi::OsString;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{ExitCode, Stdio};
use std::time::Duration;

use tokio::process::Command;

use wirecatch_media::phantomjs::{DENO_VARIABLE, FOLDER_VARIABLE, VERSION};

const SHIM: &str = include_str!("shim.js");

/// The longest a script may run: past yt-dlp's own limit for any of its extractors (two minutes), which ends the stand-in sooner.
const LIMIT: Duration = Duration::from_secs(150);

/// The exit code when the stand-in cannot run the script at all.
const CANNOT_RUN: u8 = 2;

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    if args.iter().any(|a| a == "-v" || a == "--version") {
        return match writeln!(std::io::stdout().lock(), "{VERSION}") {
            Ok(()) => ExitCode::SUCCESS,
            Err(_) => ExitCode::from(CANNOT_RUN),
        };
    }
    match run(&args).await {
        Ok(code) => code,
        Err(reason) => {
            eprintln!("phantomjs stand-in: {reason}");
            ExitCode::from(CANNOT_RUN)
        }
    }
}

async fn run(args: &[OsString]) -> Result<ExitCode, String> {
    // PhantomJS's own options (`--ssl-protocol=any`) mean nothing without a network.
    let script = args
        .iter()
        .rfind(|a| !a.to_string_lossy().starts_with("--"))
        .map(PathBuf::from)
        .ok_or("no script to run")?;
    let deno = std::env::var_os(DENO_VARIABLE).ok_or("no Deno named")?;
    let folder = PathBuf::from(std::env::var_os(FOLDER_VARIABLE).ok_or("no folder named")?);
    if folder.to_string_lossy().contains(',') {
        return Err("the run's folder has a comma in its path, which Deno cannot be given".into());
    }
    let inside = tokio::fs::canonicalize(&folder)
        .await
        .map_err(|e| format!("the folder: {e}"))?;
    let canonical_script = tokio::fs::canonicalize(&script)
        .await
        .map_err(|e| format!("the script: {e}"))?;
    if !canonical_script.starts_with(&inside) {
        return Err("the script is outside the run's folder".into());
    }
    let shim = folder.join(format!("phantomjs-shim-{}.js", std::process::id()));
    tokio::fs::write(&shim, SHIM)
        .await
        .map_err(|e| format!("could not write the shim: {e}"))?;
    let ran = match own_files(&folder).await {
        Ok(files) => run_deno(Path::new(&deno), &folder, &files, &shim, &script).await,
        Err(e) => Err(format!("could not list the run's folder: {e}")),
    };
    if let Err(e) = tokio::fs::remove_file(&shim).await {
        eprintln!("phantomjs stand-in: could not delete the shim: {e}");
    }
    ran
}

/// The files directly in `folder`, comma-separated for Deno and named under `folder` as given: yt-dlp names them that way, and Deno compares links literally. Folders are left out: yt-dlp's one-file build unpacks its libraries into one.
async fn own_files(folder: &Path) -> std::io::Result<OsString> {
    let mut entries = tokio::fs::read_dir(folder).await?;
    let mut files = OsString::new();
    while let Some(entry) = entries.next_entry().await? {
        let name = entry.file_name();
        if !entry.file_type().await?.is_file() || name.to_string_lossy().contains(',') {
            continue;
        }
        if !files.is_empty() {
            files.push(",");
        }
        files.push(folder.join(name));
    }
    Ok(files)
}

/// Runs the script in Deno with its outputs passed straight through, so yt-dlp reads them as PhantomJS's. Deno stays in this program's process group or job, which the app ends with the run.
async fn run_deno(
    deno: &Path,
    folder: &Path,
    files: &OsString,
    shim: &Path,
    script: &Path,
) -> Result<ExitCode, String> {
    let mut read = OsString::from("--allow-read=");
    read.push(files);
    let mut write = OsString::from("--allow-write=");
    write.push(files);
    let mut command = Command::new(deno);
    command
        .args([
            "run",
            "--no-config",
            "--no-lock",
            "--no-remote",
            "--no-npm",
            "--no-prompt",
        ])
        .args(["--no-code-cache", "--v8-flags=--max-old-space-size=256"])
        .arg(read)
        .arg(write)
        .arg(shim)
        .arg(script)
        .current_dir(folder)
        .env("DENO_DIR", folder.join("deno"))
        .env("DENO_NO_UPDATE_CHECK", "1")
        .env("NO_COLOR", "1")
        .stdin(Stdio::null());
    #[cfg(windows)]
    {
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    let mut child = command
        .spawn()
        .map_err(|e| format!("could not start Deno: {e}"))?;
    match tokio::time::timeout(LIMIT, child.wait()).await {
        Ok(Ok(status)) => Ok(ExitCode::from(
            status
                .code()
                .and_then(|c| u8::try_from(c).ok())
                .unwrap_or(1),
        )),
        Ok(Err(e)) => Err(format!("could not wait for Deno: {e}")),
        Err(_) => {
            if let Err(e) = child.kill().await {
                eprintln!("phantomjs stand-in: could not stop Deno: {e}");
            }
            Err(format!(
                "the script ran longer than {} seconds",
                LIMIT.as_secs()
            ))
        }
    }
}

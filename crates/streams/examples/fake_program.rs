//! A stand-in for the media add-on's programs, used by the tests of `wirecatch_streams::process` (`cargo test` builds it). Its first argument says what it does:
//!
//! - `hang`: writes `ready` to standard output, then waits until it is killed.
//! - `ticks <count>`: writes a line every 100 ms, `count` times.
//! - `write-quietly <path> <count>`: writes `ready`, then appends to the file `path` every 100 ms, `count` times, writing nothing more to its outputs.
//! - `rewrite-quietly <path> <count>`: writes `ready`, writes 1 KiB into the file `path`, then writes the same 1 KiB over it every 100 ms, `count` times, so the file changes without growing (as ffmpeg's `+faststart` pass moves the data within the file), writing nothing more to its outputs.
//! - `long-line <bytes>`: writes one line of `bytes` bytes of `a`, then `€` and `tail`.
//! - `leave-holder <address>`: starts itself as `hold <address>`, sharing standard error, passes on the line `holding` it writes, and exits.
//! - `leave-detached-child` (Unix only): starts itself as `linger`, sharing its outputs, in a process group of its own (as a program that calls `setsid` would), and exits at once.
//! - `start-holder <address>`: starts itself as `hold <address>`, sharing its outputs, then waits until it is killed.
//! - `hold <address>`: connects to the TCP listener at `address`, writes `holding`, and keeps the connection open for a minute, so the listener learns when this process ends.
//! - `linger`: does nothing for a minute, then exits.
//! - `fail`: writes `ERROR: it went wrong` to standard error and exits with failure.

use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::net::TcpStream;
use std::process::{ExitCode, Stdio};
use std::sync::mpsc;
use std::time::Duration;

/// How often `ticks` and `write-quietly` act: the pace of a slow program.
const TICK: Duration = Duration::from_millis(100);

/// How long `linger` lasts: a minute, long past any test's wait.
const LINGER: Duration = Duration::from_secs(60);

/// What `write-quietly` appends each time.
const CHUNK: [u8; 1024] = [0; 1024];

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let number = |i: usize| args.get(i).and_then(|n| n.parse::<u64>().ok()).unwrap_or(0);
    let mut out = std::io::stdout();
    let done = match args.first().map(String::as_str) {
        Some("hang") => writeln!(out, "ready")
            .and_then(|()| out.flush())
            .and_then(|()| hang()),
        Some("ticks") => (0..number(1)).try_for_each(|n| {
            pace();
            writeln!(out, "tick {n}").and_then(|()| out.flush())
        }),
        Some("write-quietly") => {
            let path = args.get(1).cloned().unwrap_or_default();
            writeln!(out, "ready")
                .and_then(|()| out.flush())
                .and_then(|()| {
                    let mut file = std::fs::File::create(&path)?;
                    (0..number(2)).try_for_each(|_| {
                        pace();
                        file.write_all(&CHUNK).and_then(|()| file.flush())
                    })
                })
        }
        Some("rewrite-quietly") => {
            let path = args.get(1).cloned().unwrap_or_default();
            writeln!(out, "ready")
                .and_then(|()| out.flush())
                .and_then(|()| {
                    let mut file = std::fs::File::create(&path)?;
                    file.write_all(&CHUNK)?;
                    (0..number(2)).try_for_each(|_| {
                        pace();
                        file.seek(SeekFrom::Start(0))?;
                        file.write_all(&CHUNK).and_then(|()| file.flush())
                    })
                })
        }
        Some("long-line") => {
            let len = usize::try_from(number(1)).unwrap_or(0);
            writeln!(out, "{}€tail", "a".repeat(len))
        }
        Some("leave-holder") => leave_holder(args.get(1).map_or("", String::as_str)),
        #[cfg(unix)]
        Some("leave-detached-child") => start_detached(&["linger"]),
        Some("start-holder") => {
            let address = args.get(1).map_or("", String::as_str);
            start_self(&["hold", address]).and_then(|()| hang())
        }
        Some("hold") => hold(args.get(1).map_or("", String::as_str)),
        Some("linger") => {
            linger();
            Ok(())
        }
        Some("fail") => {
            eprintln!("ERROR: it went wrong");
            return ExitCode::FAILURE;
        }
        _ => {
            eprintln!("fake program: unknown command {args:?}");
            return ExitCode::from(2);
        }
    };
    match done {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("fake program: {e}");
            ExitCode::FAILURE
        }
    }
}

/// This program again, with `words`, sharing the outputs.
fn self_command(words: &[&str]) -> std::io::Result<std::process::Command> {
    let mut command = std::process::Command::new(std::env::current_exe()?);
    command.args(words);
    Ok(command)
}

/// Starts this program again with `words`, sharing the outputs.
fn start_self(words: &[&str]) -> std::io::Result<()> {
    self_command(words)?.spawn().map(|_| ())
}

/// Starts this program again with `words`, sharing the outputs, in a process group of its own; it must end by itself, since nothing can kill it.
#[cfg(unix)]
fn start_detached(words: &[&str]) -> std::io::Result<()> {
    use std::os::unix::process::CommandExt;
    self_command(words)?.process_group(0).spawn().map(|_| ())
}

/// Starts `hold <address>` and exits once it holds the connection, leaving it running.
fn leave_holder(address: &str) -> std::io::Result<()> {
    let mut holder = self_command(&["hold", address])?
        .stdout(Stdio::piped())
        .spawn()?;
    let output = holder
        .stdout
        .take()
        .ok_or_else(|| std::io::Error::other("the holder's output is not piped"))?;
    let mut line = String::new();
    BufReader::new(output).read_line(&mut line)?;
    let mut out = std::io::stdout();
    write!(out, "{line}")?;
    out.flush()
}

/// Connects to `address`, says so, and holds the connection open for as long as [`linger`] lasts.
fn hold(address: &str) -> std::io::Result<()> {
    let connection = TcpStream::connect(address)?;
    let mut out = std::io::stdout();
    writeln!(out, "holding")?;
    out.flush()?;
    linger();
    drop(connection);
    Ok(())
}

/// Does nothing for [`LINGER`].
fn linger() {
    wait(LINGER);
}

/// Waits on a pipe nobody writes to: until the program is killed.
fn hang() -> std::io::Result<()> {
    let (mut reader, _writer) = std::io::pipe()?;
    let mut byte = [0];
    reader.read(&mut byte).map(|_| ())
}

/// Lets [`TICK`] pass: the pace of a slow program.
fn pace() {
    wait(TICK);
}

/// Lets `time` pass, waiting on a channel nobody sends on with that deadline.
fn wait(time: Duration) {
    let (_sender, receiver) = mpsc::channel::<()>();
    match receiver.recv_timeout(time) {
        Ok(()) | Err(_) => {}
    }
}

//! A stand-in for the media add-on's programs, used by the tests of `wirecatch_streams::process` (`cargo test` builds it). Its first argument says what it does:
//!
//! - `hang`: writes `ready` to standard output, then waits until it is killed.
//! - `ticks <address>`: connects to the TCP listener at `address` and takes one step for each byte it reads there, answering each with a byte once done: a step writes the line `tick <n>`. It exits when the listener closes the connection. The test sets the pace, so how slow the machine is does not change what the run sees.
//! - `write-quietly <path> <address>`: creates the file `path`, then steps as `ticks` does, each step appending 1 KiB to the file and writing nothing to its outputs.
//! - `rewrite-quietly <path> <address>`: writes 1 KiB into the file `path`, then steps as `ticks` does, each step writing the same 1 KiB over it and moving its write time on by a second, so the file changes without growing (as ffmpeg's `+faststart` pass moves the data within the file) and every step shows, however close together the steps are; it writes nothing to its outputs.
//! - `long-line <bytes>`: writes one line of `bytes` bytes of `a`, then `€` and `tail`.
//! - `leave-holder <address>`: starts itself as `hold <address>`, sharing standard error, passes on the line `holding` it writes, and exits.
//! - `leave-detached-child` (Unix only): starts itself as `linger`, sharing its outputs, in a process group of its own (as a program that calls `setsid` would), and exits at once.
//! - `start-holder <address>`: starts itself as `hold <address>`, sharing its outputs, then waits until it is killed.
//! - `hold <address>`: connects to the TCP listener at `address`, writes `holding`, and keeps the connection open until the listener closes it, so the listener learns when this process ends, and it never ends by itself while a test waits for that.
//! - `linger`: does nothing for a minute, then exits.
//! - `fail`: writes `ERROR: it went wrong` to standard error and exits with failure.

use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::net::TcpStream;
use std::process::{ExitCode, Stdio};
use std::sync::mpsc;
use std::time::Duration;

/// How long `linger` lasts: a minute, long enough to outlast the program that started it.
const LINGER: Duration = Duration::from_secs(60);

/// How far `rewrite-quietly` moves its file's write time at each step: far more than any file system's time resolution.
const STEP_TIME: Duration = Duration::from_secs(1);

/// What `write-quietly` appends, and `rewrite-quietly` writes over, at each step.
const CHUNK: [u8; 1024] = [0; 1024];

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let number = |i: usize| args.get(i).and_then(|n| n.parse::<u64>().ok()).unwrap_or(0);
    let mut out = std::io::stdout();
    let done = match args.first().map(String::as_str) {
        Some("hang") => writeln!(out, "ready")
            .and_then(|()| out.flush())
            .and_then(|()| hang()),
        Some("ticks") => steps(args.get(1), |n| {
            writeln!(out, "tick {n}").and_then(|()| out.flush())
        }),
        Some("write-quietly") => std::fs::File::create(args.get(1).map_or("", String::as_str))
            .and_then(|mut file| steps(args.get(2), |_| file.write_all(&CHUNK))),
        Some("rewrite-quietly") => std::fs::File::create(args.get(1).map_or("", String::as_str))
            .and_then(|mut file| {
                file.write_all(&CHUNK)?;
                let mut written = file.metadata()?.modified()?;
                steps(args.get(2), |_| {
                    file.seek(SeekFrom::Start(0))?;
                    file.write_all(&CHUNK)?;
                    written += STEP_TIME;
                    file.set_modified(written)
                })
            }),
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

/// Connects to `address`, says so, and holds the connection open until the listener closes it.
fn hold(address: &str) -> std::io::Result<()> {
    let mut connection = TcpStream::connect(address)?;
    let mut out = std::io::stdout();
    writeln!(out, "holding")?;
    out.flush()?;
    let mut byte = [0];
    while connection.read(&mut byte)? > 0 {}
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

/// Connects to the listener at `address` and takes a `step` for each byte read there, answering each with a byte once it is done, until the listener closes the connection.
fn steps(
    address: Option<&String>,
    mut step: impl FnMut(u64) -> std::io::Result<()>,
) -> std::io::Result<()> {
    let mut connection = TcpStream::connect(address.map_or("", String::as_str))?;
    let mut byte = [0];
    for n in 0.. {
        if connection.read(&mut byte)? == 0 {
            break;
        }
        step(n)?;
        connection.write_all(&byte)?;
    }
    Ok(())
}

/// Lets `time` pass, waiting on a channel nobody sends on with that deadline.
fn wait(time: Duration) {
    let (_sender, receiver) = mpsc::channel::<()>();
    match receiver.recv_timeout(time) {
        Ok(()) | Err(_) => {}
    }
}

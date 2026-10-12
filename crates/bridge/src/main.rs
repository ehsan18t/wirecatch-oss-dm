//! `wirecatch-bridge`: the native messaging host that connects the Wirecatch
//! browser extension to the Wirecatch app.
//!
//! - Started by the browser (native messaging mode): checks the caller, then
//!   relays length-prefixed JSON frames between stdin/stdout and the app's
//!   per-user local socket, starting the app with `--background` when needed. In
//!   this mode stdout carries only frames; every log line goes to stderr.
//! - `wirecatch-bridge install` / `uninstall`: register or remove the host in every supported browser, for the current user or, with `--all-users` (Windows, administrator rights), for every user of the computer.

// Stdout belongs to the browser in native messaging mode, so nothing here prints
// to it: the workspace lints warn on `print!`/`println!` (and the gate denies
// warnings), and the install command writes its report through an explicit handle.

use std::ffi::OsString;
use std::io::{self, Write};
use std::process::ExitCode;
use std::thread;

use tokio::sync::mpsc;

/// Writes one line to stderr, never stdout; see [`log_line`].
macro_rules! log {
    ($($arg:tt)*) => {
        $crate::log_line(format_args!($($arg)*))
    };
}
pub(crate) use log;

/// Writes one log line to stderr, without the panic of `eprintln!` when that fails.
fn log_line(line: std::fmt::Arguments<'_>) {
    stderr_may_be_closed(&writeln!(io::stderr(), "wirecatch-bridge: {line}"));
}

/// Takes the outcome of writing a log line. The browser may have closed stderr, and then
/// nowhere is left to report that.
fn stderr_may_be_closed(written: &io::Result<()>) {
    let (Ok(()) | Err(_)) = written;
}

mod app;
mod caller;
mod config;
mod install;
mod raw_frame;
mod relay;
#[cfg(windows)]
mod shell_parent;

use raw_frame::RawFrame;

/// Frames buffered between the stdio threads and the relay. Small on purpose:
/// a slow side slows the other down instead of growing memory.
const QUEUE_LEN: usize = 4;

/// The flag that registers for every user of the computer.
const ALL_USERS: &str = "--all-users";

const USAGE: &str = "\
wirecatch-bridge connects the Wirecatch browser extension to the Wirecatch app.
Browsers start it on their own; you do not need to run it.

Usage:
  wirecatch-bridge install [--all-users]     Register the bridge with your browsers
  wirecatch-bridge uninstall [--all-users]   Remove that registration

--all-users registers for every user of this computer (Windows; needs
administrator rights). The installer does this.
";

fn main() -> ExitCode {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    let first = args.first().and_then(|arg| arg.to_str());
    let scope = match args.get(1).and_then(|arg| arg.to_str()) {
        Some(ALL_USERS) => Some(install::Scope::Machine),
        None => Some(install::Scope::User),
        Some(_) => None,
    };
    match (first, args.len(), scope) {
        (Some("install"), 1..=2, Some(scope)) => install::run(install::Command::Install, scope),
        (Some("uninstall"), 1..=2, Some(scope)) => install::run(install::Command::Uninstall, scope),
        (Some("--help" | "-h" | "help"), 1, _) => {
            if let Err(e) = io::stdout().write_all(USAGE.as_bytes()) {
                log!("could not print the usage: {e}");
                return ExitCode::FAILURE;
            }
            ExitCode::SUCCESS
        }
        _ => native_messaging_host(&args),
    }
}

fn native_messaging_host(args: &[OsString]) -> ExitCode {
    let config = match config::Config::embedded() {
        Ok(config) => config,
        Err(e) => {
            log!("{e:#}");
            return ExitCode::FAILURE;
        }
    };
    if let Err(reason) = caller::identify(args, &config) {
        log!(
            "refused to start: {reason}. The bridge only serves the Wirecatch extension. Run `wirecatch-bridge --help` for the commands you can use."
        );
        return ExitCode::FAILURE;
    }

    #[cfg(windows)]
    keep_browser_pipes_private();

    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_io()
        .enable_time()
        .build()
    {
        Ok(runtime) => runtime,
        Err(e) => {
            log!("could not start: {e}");
            return ExitCode::FAILURE;
        }
    };

    let (to_relay, incoming) = mpsc::channel(QUEUE_LEN);
    let (outgoing, to_browser) = mpsc::channel(QUEUE_LEN);
    // Blocking threads for the browser pipes: they need no runtime support, and
    // a blocked read never holds up shutdown.
    if let Err(e) = thread::Builder::new()
        .name("stdin".into())
        .spawn(move || read_browser(&to_relay))
    {
        log!("could not start: {e}");
        return ExitCode::FAILURE;
    }
    let writer = match thread::Builder::new()
        .name("stdout".into())
        .spawn(move || write_browser(to_browser))
    {
        Ok(writer) => writer,
        Err(e) => {
            log!("could not start: {e}");
            return ExitCode::FAILURE;
        }
    };

    let ended = runtime.block_on(relay::run(incoming, outgoing, |first| {
        let start = if first.allows_starting_the_app() {
            app::Start::IfNeeded
        } else {
            app::Start::Never
        };
        async move {
            let endpoint = app::endpoint().map_err(|e| {
                log!("{e}");
                app::Unavailable::from_ipc(&e)
            })?;
            let launcher = app::launcher().map_err(|e| {
                log!("{e:#}");
                app::Unavailable::cannot_start(&format!("{e:#}"))
            })?;
            app::connect_or_launch(&endpoint, &launcher, app::START_BUDGET, start).await
        }
    }));
    if ended == relay::Ended::AppClosed {
        // Exiting closes stdout, so the browser reports the port as disconnected. The extension then only checks whether Wirecatch is running; it starts it again only for a download.
        log!("Wirecatch went away; closing so the extension can reconnect");
    }
    // Dropping the runtime drops the app reader task and with it the last sender
    // to the stdout thread, which then finishes what is queued and stops. The
    // stdin thread may still be blocked reading; returning from main ends it.
    drop(runtime);
    if writer.join().is_err() {
        log!(
            "the thread writing to the browser stopped with a panic; the last messages may be lost"
        );
    }
    ExitCode::SUCCESS
}

/// Reads frames from the browser until it closes stdin.
fn read_browser(to_relay: &mpsc::Sender<RawFrame>) {
    let stdin = io::stdin();
    let mut stdin = stdin.lock();
    loop {
        match raw_frame::read_blocking(&mut stdin) {
            Ok(Some(raw_frame::Incoming::Frame(frame))) => {
                if to_relay.blocking_send(frame).is_err() {
                    return;
                }
            }
            Ok(Some(raw_frame::Incoming::Oversized { len })) => {
                // Its id is unknown (the body was not kept), so there is nobody to
                // answer; the session goes on with the next message.
                log!(
                    "skipped a {len} byte message from the extension: messages are limited to {} bytes",
                    wirecatch_protocol::frame::MAX_FRAME_LEN
                );
            }
            Ok(None) => return,
            Err(e) => {
                // A frame cut short or a broken pipe: nothing more can be read.
                log!("stopped reading from the browser: {e}");
                return;
            }
        }
    }
}

/// Writes frames to the browser, one flush per frame.
fn write_browser(mut to_browser: mpsc::Receiver<RawFrame>) {
    let stdout = io::stdout();
    let mut stdout = stdout.lock();
    while let Some(frame) = to_browser.blocking_recv() {
        if let Err(e) = stdout
            .write_all(frame.as_bytes())
            .and_then(|()| stdout.flush())
        {
            log!("stopped writing to the browser: {e}");
            return;
        }
    }
}

/// Marks the stdin, stdout and stderr handles the browser gave us as not
/// inheritable.
///
/// Windows hands every inheritable handle to a child process. Without this, the
/// app started by the bridge would hold copies of the browser's pipe handles, so
/// the browser would not see the pipes close when the bridge exits, and the app
/// could read or write the extension's channel.
#[cfg(windows)]
fn keep_browser_pipes_private() {
    use windows_sys::Win32::Foundation::{
        HANDLE_FLAG_INHERIT, INVALID_HANDLE_VALUE, SetHandleInformation,
    };
    use windows_sys::Win32::System::Console::{
        GetStdHandle, STD_ERROR_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE,
    };
    for which in [STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, STD_ERROR_HANDLE] {
        // SAFETY: GetStdHandle has no preconditions; the handle is only used for
        // the call below and not closed.
        let handle = unsafe { GetStdHandle(which) };
        if handle.is_null() || handle == INVALID_HANDLE_VALUE {
            continue;
        }
        // SAFETY: `handle` is one of this process's standard handles.
        if unsafe { SetHandleInformation(handle, HANDLE_FLAG_INHERIT, 0) } == 0 {
            log!(
                "could not keep a browser pipe private: {}",
                io::Error::last_os_error()
            );
        }
    }
}

//! Running the media add-on's programs (ffmpeg, yt-dlp): the one place Wirecatch starts them, for streams and for `wirecatch-core`.
//!
//! A program is always started with an argument vector, never through a shell; in a folder the caller names; without a console window on Windows; without Python's environment variables. It and the programs it starts form one group (a process group on Unix, a job object on Windows) that is killed as a whole when the run is cancelled, times out, stalls or is dropped, and once the program itself has ended. On Unix a program that leaves the group (`setsid`, or a process group of its own) is outside it and is not killed; the run reads the outputs it may hold open for a short grace only, so it never waits for that program.

mod run;
mod tree;

pub use run::{Output, Ran, RunError, RunOptions, StdoutMode, run};

/// How long a program that reports progress (ffmpeg with `-progress`, yt-dlp downloading) may go without a report, or without its watched output changing, before it counts as stuck: a hung encoder driver, a blocked input. Healthy runs report many times a minute.
pub const STALL_TIMEOUT: std::time::Duration = std::time::Duration::from_mins(5);

/// The most of a program's standard output a run collects, in bytes; past it the output is cut short and [`Ran::stdout_truncated`] is set. yt-dlp's description of a page is a few MB.
pub const MAX_STDOUT: u64 = 64 * wirecatch_engine::units::MIB;

/// What a failed program's reason reads as when it printed none.
pub const NO_REASON: &str = "it gave no reason";

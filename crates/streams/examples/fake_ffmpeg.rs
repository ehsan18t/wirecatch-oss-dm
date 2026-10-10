//! A stand-in for ffmpeg, used by this crate's tests (`cargo test` builds it).
//!
//! It checks that every `-i file:<path>` input exists, then writes the
//! arguments it received, one per line, into the output (its last argument,
//! `file:<path>`). This shows exactly what a real process sees, so the tests
//! can prove that paths with spaces, quotes or leading dashes arrive as single,
//! unchanged arguments. An output name containing `fail-please` makes it fail
//! with a message on stderr, like a real ffmpeg error.

use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(output) = args.last().and_then(|a| a.strip_prefix("file:")) else {
        eprintln!("fake ffmpeg: the last argument is not a file: output");
        return ExitCode::from(2);
    };
    for pair in args.windows(2) {
        if pair[0] == "-i" {
            let input = pair[1].strip_prefix("file:").unwrap_or(&pair[1]);
            if !std::path::Path::new(input).is_file() {
                eprintln!("fake ffmpeg: {input}: No such file or directory");
                return ExitCode::from(1);
            }
        }
    }
    if output.contains("fail-please") {
        eprintln!("[mp4 @ 0x1] fake ffmpeg failed\nConversion failed!");
        return ExitCode::from(1);
    }
    match std::fs::write(output, args.join("\n")) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("fake ffmpeg: {output}: {e}");
            ExitCode::from(1)
        }
    }
}

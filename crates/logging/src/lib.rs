//! Wirecatch's logging: where every binary's log lines go, and how secrets are kept out of them (docs/ENGINEERING.md section 6, D40).
//!
//! # Where lines go
//!
//! A binary calls [`start`] first thing in `main`, then [`Logging::write_to`] once it knows its log folder. Every line then goes
//!
//! - to stderr, as before (a Windows release build of the app has none, and a browser may close the bridge's);
//! - to `<binary>.log` in the log folder: [`LOGS_FOLDER`] in the app's data folder, or the folder [`LOG_FOLDER_VARIABLE`] names, made with the first line written. A file stops at [`FILE_LIMIT`] bytes and becomes `<binary>.1.log`; [`OLD_FILES_KEPT`] old files are kept. Lines logged before the folder is known are held (up to [`EARLY_LIMIT`] bytes) and written first;
//! - and a panic is reported as an error line, redacted like the rest, in place of the standard report, so it reaches the file even in a build that aborts on panic.
//!
//! [`LOG_FILTER_VARIABLE`] chooses the lines, in `tracing`'s filter syntax (`debug`, `info,wirecatch_core=trace`); each binary has its own default. Lines are written as they are logged, without a background thread, so the last ones before a crash are in the file.
//!
//! # Secrets
//!
//! Every field is passed through [`redact::text_for_log`] as it is recorded, before `Debug` escapes it, and every finished line again on its way out, whatever logged it and whatever its level: no cookie, authorization header, password, token, or query of a link reaches stderr or the file. Code still never logs a secret on purpose. Lines carry no colors, since a color code between a name and its value would hide the pair from the redaction.
//!
//! # Features
//!
//! `setup` (default): [`start`] and the log file. Libraries that only use [`redact`] turn default features off.

pub mod redact;

#[cfg(feature = "setup")]
mod fields;
#[cfg(feature = "setup")]
mod file;
#[cfg(feature = "setup")]
mod sink;

#[cfg(feature = "setup")]
pub use setup::*;

#[cfg(feature = "setup")]
mod setup {
    use std::backtrace::Backtrace;
    use std::fmt::Write as _;
    use std::io::{self, Write as _};
    use std::path::{Path, PathBuf};
    use std::sync::{Arc, Once, OnceLock};

    use tracing_subscriber::EnvFilter;
    use tracing_subscriber::fmt::format::Writer;
    use tracing_subscriber::fmt::time::{FormatTime as _, SystemTime};
    use wirecatch_product::Channel;
    pub use wirecatch_product::{LOG_FILTER_VARIABLE, LOG_FOLDER_VARIABLE};

    use crate::fields::RedactedFields;
    use crate::redact;
    use crate::sink::{MakeSink, Sink};

    /// The log folder's name in the app's data folder.
    pub const LOGS_FOLDER: &str = "logs";
    /// The most a log file holds, in bytes, before it becomes the first old file: 5 MiB, days of
    /// lines at the default levels.
    pub const FILE_LIMIT: u64 = 5 * 1024 * 1024;
    /// How many old files of each binary are kept beside its log file.
    pub const OLD_FILES_KEPT: usize = 3;
    /// The most held, in bytes, of what is logged before the log folder is known; the app learns
    /// its data folder only once it starts.
    pub const EARLY_LIMIT: usize = 256 * 1024;

    /// A binary's logging, set up by [`start`].
    #[derive(Debug)]
    pub struct Logging {
        sink: Arc<Sink>,
    }

    /// Sets up logging for the binary `name` (the log file is `<name>.log`): to stderr now, and to
    /// the log file once [`Logging::write_to`] names its folder. `default_filter` chooses the lines
    /// when [`LOG_FILTER_VARIABLE`] is not set, or is not a filter.
    ///
    /// Call it once, first thing in `main`. A second call, or a logger set up before, leaves the
    /// first one in place and says so on stderr.
    #[must_use]
    pub fn start(name: &'static str, default_filter: &'static str) -> Logging {
        let sink = Arc::new(Sink::new(name, true));
        let set = std::env::var(LOG_FILTER_VARIABLE).ok();
        let (filter, problem) = filter(set.as_deref(), default_filter);
        match subscriber(filter, &sink).try_init() {
            Ok(()) => {
                // Set once: a second `start` fails to set up logging above.
                let (Ok(()) | Err(_)) = PANIC_SINK.set(Arc::clone(&sink));
            }
            Err(e) => {
                let note = format!(
                    "{name}: logging was set up already, so these lines do not reach its log file: {e}\n"
                );
                let (Ok(()) | Err(_)) = io::stderr().write_all(note.as_bytes());
            }
        }
        log_panics();
        if let Some(problem) = problem {
            tracing::warn!("{problem}");
        }
        Logging { sink }
    }

    impl Logging {
        /// Writes the log file in `folder` from now on, beginning with what was logged until now; the folder and the file are made with the first line, so a run that logs nothing makes neither. Without a folder, or when the file cannot be written (logged as a warning, or said once on stderr), lines go to stderr only.
        pub fn write_to(&self, folder: Option<&Path>) {
            let Some(folder) = folder else {
                self.sink.without_file();
                tracing::warn!("there is no folder for the log file, so lines go to stderr only");
                return;
            };
            if let Err(e) = self.sink.write_to(folder) {
                tracing::warn!(
                    folder = %folder.display(),
                    error = %e,
                    "could not write the log file, so lines go to stderr only"
                );
            }
        }
    }

    /// The log folder of the app whose data folder is `data_folder`, unless
    /// [`LOG_FOLDER_VARIABLE`] names another.
    #[must_use]
    pub fn folder_in(data_folder: &Path) -> PathBuf {
        chosen_folder(std::env::var_os(LOG_FOLDER_VARIABLE))
            .unwrap_or_else(|| data_folder.join(LOGS_FOLDER))
    }

    /// The log folder of the app of `channel`: [`folder_in`] its data folder in the per-user local data folder, unless [`LOG_FOLDER_VARIABLE`] names another. Known before the app starts, so even a start that fails early is logged. `None` when the system names no local data folder and the variable is not set.
    #[must_use]
    pub fn channel_folder(channel: Channel) -> Option<PathBuf> {
        chosen_folder(std::env::var_os(LOG_FOLDER_VARIABLE)).or_else(|| {
            dirs::data_local_dir().map(|local| local.join(channel.data_folder()).join(LOGS_FOLDER))
        })
    }

    /// The release app's [`channel_folder`], where the binaries that belong to no channel log: the bridge, which every build registers under one name (D22), and the command-line downloader.
    #[must_use]
    pub fn release_folder() -> Option<PathBuf> {
        channel_folder(Channel::Release)
    }

    /// The folder [`LOG_FOLDER_VARIABLE`] names, when it is set to one.
    fn chosen_folder(set: Option<std::ffi::OsString>) -> Option<PathBuf> {
        set.filter(|folder| !folder.is_empty()).map(PathBuf::from)
    }

    /// The filter `set` describes, or `default` and why `set` was not used when it is no filter.
    fn filter(set: Option<&str>, default: &str) -> (EnvFilter, Option<String>) {
        match set.map(str::trim).filter(|set| !set.is_empty()) {
            None => (EnvFilter::new(default), None),
            Some(set) => match EnvFilter::try_new(set) {
                Ok(filter) => (filter, None),
                Err(e) => (
                    EnvFilter::new(default),
                    Some(format!(
                        "{LOG_FILTER_VARIABLE} is not a log filter ({e}), so the default \"{default}\" is used"
                    )),
                ),
            },
        }
    }

    /// The subscriber that formats every line for `sink`, without colors and with each field redacted as it is recorded.
    fn subscriber(
        filter: EnvFilter,
        sink: &Arc<Sink>,
    ) -> tracing_subscriber::fmt::SubscriberBuilder<
        RedactedFields,
        tracing_subscriber::fmt::format::Format,
        EnvFilter,
        MakeSink,
    > {
        tracing_subscriber::fmt()
            .with_env_filter(filter)
            .with_ansi(false)
            .fmt_fields(RedactedFields)
            .with_writer(MakeSink(Arc::clone(sink)))
    }

    /// Where a panic is reported: the sink of the logging [`start`] set up, if it did.
    static PANIC_SINK: OnceLock<Arc<Sink>> = OnceLock::new();

    /// Reports every panic as an error line in place of the standard report, which would print its message to stderr unredacted: redacted, with the backtrace when `RUST_BACKTRACE` asks for one, and written to the sink directly, so it reaches stderr and the file whatever the filter chooses (a filter such as `wirecatch_core=debug` leaves every other target out). Without a sink of its own, to stderr only. Installed once per process.
    fn log_panics() {
        static INSTALLED: Once = Once::new();
        INSTALLED.call_once(|| {
            std::panic::set_hook(Box::new(|panic| {
                let line = redact::text_for_log(&panic_report(panic));
                match PANIC_SINK.get() {
                    Some(sink) => sink.write(line.as_bytes()),
                    None => {
                        let (Ok(()) | Err(_)) = io::stderr().write_all(line.as_bytes());
                    }
                }
            }));
        });
    }

    /// A panic as a log line, shaped like the others: the time, `ERROR`, the thread, where and what, then the backtrace when `RUST_BACKTRACE` asks for one.
    fn panic_report(panic: &std::panic::PanicHookInfo<'_>) -> String {
        let mut line = String::new();
        // Writing to a `String` cannot fail.
        let (Ok(()) | Err(_)) = SystemTime.format_time(&mut Writer::new(&mut line));
        let thread = std::thread::current();
        let thread = thread.name().unwrap_or("unnamed");
        let (Ok(()) | Err(_)) = match panic_backtrace() {
            Some(backtrace) => {
                writeln!(line, " ERROR panic: thread '{thread}' {panic}\n{backtrace}")
            }
            None => writeln!(line, " ERROR panic: thread '{thread}' {panic}"),
        };
        line
    }

    /// The backtrace of a panic, as the standard report decides it: none when `RUST_BACKTRACE` is unset or `0`, every frame for `full`, the short form otherwise. `RUST_LIB_BACKTRACE`, which `Backtrace::capture` reads first, is for errors that capture one, not for panics.
    fn panic_backtrace() -> Option<String> {
        match std::env::var_os("RUST_BACKTRACE")?.to_str() {
            Some("0") => None,
            Some("full") => Some(format!("{:#}", Backtrace::force_capture())),
            _ => Some(Backtrace::force_capture().to_string()),
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::fs;

        /// A sink that writes no stderr, with a subscriber for it, as [`start`] makes them.
        fn logging(filter_set: Option<&str>, default: &str) -> (Logging, impl tracing::Subscriber) {
            let sink = Arc::new(Sink::new("test", false));
            let (filter, _) = filter(filter_set, default);
            let subscriber = subscriber(filter, &sink).finish();
            (Logging { sink }, subscriber)
        }

        fn read(folder: &Path) -> String {
            fs::read_to_string(folder.join("test.log")).unwrap()
        }

        #[test]
        fn lines_logged_before_the_folder_is_known_are_written_first() {
            let folder = tempfile::tempdir().unwrap();
            let (logging, subscriber) = logging(None, "info");
            tracing::subscriber::with_default(subscriber, || {
                tracing::info!("starting");
                logging.write_to(Some(folder.path()));
                tracing::warn!("started");
            });
            let text = read(folder.path());
            let starting = text.find("starting").unwrap();
            let started = text.find("started").unwrap();
            assert!(starting < started, "{text}");
            assert!(text.contains(" WARN "), "{text}");
        }

        /// `wirecatch-bridge --help`, or an installer's `install` under another account, logs nothing and so makes no folder.
        #[test]
        fn no_folder_is_made_until_a_line_is_logged() {
            let root = tempfile::tempdir().unwrap();
            let folder = root.path().join("data").join(LOGS_FOLDER);
            let (logging, subscriber) = logging(None, "info");
            tracing::subscriber::with_default(subscriber, || {
                logging.write_to(Some(&folder));
                tracing::debug!("not chosen by the filter");
                assert!(
                    !root.path().join("data").exists(),
                    "made before anything was logged"
                );
                tracing::info!("the first line");
            });
            assert!(read(&folder).contains("the first line"));
        }

        #[test]
        fn no_line_carries_a_secret_whatever_its_level_or_where_the_secret_is() {
            let folder = tempfile::tempdir().unwrap();
            let (logging, subscriber) = logging(Some("trace"), "info");
            tracing::subscriber::with_default(subscriber, || {
                logging.write_to(Some(folder.path()));
                let span = tracing::info_span!(
                    "download",
                    url = "https://ada:pa55word@example.com/f?sig=S1GNED"
                );
                let _in = span.enter();
                tracing::error!(cookie = "SID=C00KIE", "request failed");
                tracing::warn!("sent Authorization: Bearer T0KEN");
                tracing::info!(headers = ?[("Cookie", "a=C00KIE2")], "headers");
                tracing::debug!("password=PA55 user=ada");
                tracing::trace!(link = %"https://cdn.example.com/v?X-Amz-Signature=S1G2", "fetching");
                // `tracing` records a `&str` with `Debug`, which escapes its quotes.
                let body = r#"{"password":"STRFIELDPW","user":"ada"}"#;
                tracing::info!(body, "posting");
                tracing::info!(body = body.to_string().as_str(), "posting again");
                let creds = Creds {
                    api_key: Some("OPTKEY".into()),
                    token: Some("OPTTOK".into()),
                };
                let login = Login {
                    user: "ada",
                    password: "hun\"TER2",
                };
                tracing::info!(?creds, ?login, "signing in");
                // Read once, since a derived `Debug` does not count as reading a field.
                assert!(
                    creds.api_key.is_some()
                        && creds.token.is_some()
                        && !login.user.is_empty()
                        && !login.password.is_empty()
                );
                let link = url::Url::parse("https://ada:URLPW@h.example/p?sig=URLSIG&st=1#URLFRAG")
                    .unwrap();
                tracing::warn!(?link, "request failed");
                tracing::info!("sent {body:?}");
                tracing::warn!("pretty {creds:#?} {link:#?}");
                // Only the field formatter sees that the whole value is the token's.
                tracing::info!(api_token = %"FIRSTPART SECONDPART", "named");
                let job = tracing::info_span!("job", body = r#"{"token":"SPANTOK"}"#);
                job.in_scope(|| tracing::info!("in a span"));
            });
            let text = read(folder.path());
            for secret in [
                "pa55word",
                "S1GNED",
                "C00KIE",
                "T0KEN",
                "C00KIE2",
                "PA55",
                "S1G2",
                "STRFIELDPW",
                "OPTKEY",
                "OPTTOK",
                "TER2",
                "URLPW",
                "URLSIG",
                "URLFRAG",
                "SPANTOK",
                "SECONDPART",
            ] {
                assert!(!text.contains(secret), "{secret} reached the log:\n{text}");
            }
            for kept in [
                "request failed",
                "example.com/f",
                "user=ada",
                "fetching",
                "posting again",
                "h.example",
                "in a span",
            ] {
                assert!(text.contains(kept), "{kept} is missing:\n{text}");
            }
        }

        #[derive(Debug)]
        struct Creds {
            api_key: Option<String>,
            token: Option<String>,
        }

        #[derive(Debug)]
        struct Login {
            user: &'static str,
            password: &'static str,
        }

        #[test]
        fn the_filter_chooses_the_lines_and_a_bad_one_falls_back_to_the_default() {
            let folder = tempfile::tempdir().unwrap();
            let (logging, subscriber) = logging(Some("warn"), "info");
            tracing::subscriber::with_default(subscriber, || {
                logging.write_to(Some(folder.path()));
                tracing::info!("chatter");
                tracing::warn!("trouble");
            });
            let text = read(folder.path());
            assert!(
                !text.contains("chatter") && text.contains("trouble"),
                "{text}"
            );

            let (_, problem) = filter(Some("info,=[nonsense"), "warn");
            let problem = problem.unwrap();
            assert!(problem.starts_with(LOG_FILTER_VARIABLE), "{problem}");
            assert!(problem.contains("\"warn\""), "{problem}");
            assert!(filter(Some("  "), "warn").1.is_none());
        }

        #[test]
        fn without_a_folder_nothing_is_held() {
            let (logging, subscriber) = logging(None, "info");
            tracing::subscriber::with_default(subscriber, || {
                tracing::info!("one");
                logging.write_to(None);
                tracing::info!("two");
            });
            assert!(matches!(
                *logging.sink.file_state(),
                crate::sink::FileState::None
            ));
        }

        #[test]
        fn the_folder_variable_names_another_folder() {
            assert_eq!(chosen_folder(None), None);
            assert_eq!(chosen_folder(Some("".into())), None);
            assert_eq!(
                chosen_folder(Some("/tmp/logs".into())),
                Some(PathBuf::from("/tmp/logs"))
            );
        }
    }
}

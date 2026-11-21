//! Facts about a terminal failure that can be shared in a support log.

use tracing::warn;
use wirecatch_engine::{Error, Retry, Url};
use wirecatch_protocol::app::UserError;

use super::state::Entry;

#[derive(Debug)]
pub(super) struct MediaFailure {
    pub error: UserError,
    pub details: Box<Details>,
}

impl From<UserError> for MediaFailure {
    fn from(error: UserError) -> Self {
        Details::new("local", "media").with_error(error)
    }
}

#[derive(Debug)]
pub(super) struct Details {
    stage: &'static str,
    category: &'static str,
    pub status: Option<u16>,
    host: Option<String>,
    segment: Option<(usize, usize)>,
    pub extractor: Option<String>,
    pub retry: Option<Retry>,
}

impl Details {
    pub fn new(stage: &'static str, category: &'static str) -> Self {
        Self {
            stage,
            category,
            status: None,
            host: None,
            segment: None,
            extractor: None,
            retry: None,
        }
    }

    pub fn with_error(self, error: UserError) -> MediaFailure {
        MediaFailure {
            error,
            details: Box::new(self),
        }
    }

    pub fn engine(error: &Error, stage: &'static str, retry: Option<Retry>) -> Self {
        let category = match error {
            Error::HttpStatus { .. } => "http",
            Error::FtpReply { .. } => "ftp",
            Error::Network(_) | Error::PrematureEof { .. } => "network",
            Error::Stalled { .. } => "timeout",
            Error::Certificate { .. } => "certificate",
            Error::Io { .. } => "disk",
            Error::RemoteChanged | Error::RangeNotHonored { .. } => "changed_response",
            Error::ProxyLoginRefused => "proxy",
            Error::PrivateNetwork { .. } => "private_network",
            _ => "transfer",
        };
        let mut details = Self::new(stage, category);
        details.retry = retry;
        match error {
            Error::HttpStatus { url, status }
            | Error::FtpReply {
                url, code: status, ..
            } => {
                details.host = host(url);
                details.status = Some(*status);
            }
            _ => {}
        }
        details
    }

    pub fn stream(error: &wirecatch_streams::Error) -> Self {
        use wirecatch_streams::Error as Stream;
        match error {
            Stream::Segment {
                number,
                total,
                cause,
            } => {
                let mut details = Self::engine(cause, "segment", None);
                details.segment = Some((*number, *total));
                details
            }
            Stream::Request(e) => Self::engine(e, "manifest", None),
            Stream::Ffmpeg(_)
            | Stream::FfmpegRun(_)
            | Stream::FfmpegMissing(_)
            | Stream::Path(_) => Self::new("merge", "ffmpeg"),
            Stream::Io(_) => Self::new("local", "disk"),
            _ => Self::new("stream", "media"),
        }
    }

    pub fn write(&self, entry: &Entry) {
        if entry.private_session.is_some() {
            return;
        }
        let source = Url::parse(&entry.row.url).ok();
        let kind = match &entry.media_job {
            Some(wirecatch_media::MediaJob::Stream { .. }) => "stream",
            Some(_) => "page",
            None if source
                .as_ref()
                .is_some_and(|url| matches!(url.scheme(), "ftp" | "ftps" | "ftpes")) =>
            {
                "ftp"
            }
            None => "file",
        };
        let source_host = source.as_ref().and_then(Url::host_str);
        let extractor = self.extractor.as_deref().filter(|name| {
            !name.is_empty()
                && name.len() <= 80
                && name
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b':'))
        });
        warn!(
            id = entry.row.id.0,
            kind,
            stage = self.stage,
            category = self.category,
            source_host,
            responding_host = self.host.as_deref(),
            status = self.status,
            extractor,
            retry = self.retry.map(|retry| retry.attempt),
            retry_budget = self.retry.map(|retry| retry.of),
            segment = self.segment.map(|(number, _)| number),
            segments = self.segment.map(|(_, total)| total),
            "download failed"
        );
    }
}

fn host(url: &str) -> Option<String> {
    Url::parse(url).ok()?.host_str().map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;
    use crate::{KnownFolders, Manager, ManagerConfig};
    use wirecatch_protocol::app::AddRequest;

    #[derive(Clone, Default)]
    struct Captured(Arc<Mutex<Vec<u8>>>);

    impl std::io::Write for Captured {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().write(bytes)
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn a_terminal_warning_uses_safe_fields_and_leaves_private_failures_out() {
        let dir = tempfile::tempdir().unwrap();
        let folders = KnownFolders::under_home(dir.path().join("home"));
        let manager = Manager::open(ManagerConfig::for_tests(dir.path().join("data"), folders))
            .await
            .unwrap();
        let id = manager
            .add_download(AddRequest {
                url: "https://page.example/path-secret?sign=query-secret"
                    .to_owned()
                    .into(),
                queue: None,
                folder: None,
                file_name: None,
                start: false,
            })
            .await
            .unwrap();
        let captured = Captured::default();
        let saved = captured.clone();
        let subscriber = tracing_subscriber::fmt()
            .with_ansi(false)
            .with_writer(move || captured.clone())
            .finish();
        let error = wirecatch_streams::Error::Segment {
            number: 51,
            total: 120,
            cause: Error::HttpStatus {
                status: 472,
                url: "https://cdn.example/path-secret?sign=query-secret".into(),
            },
        };
        tracing::subscriber::with_default(subscriber, || {
            let mut state = manager.lock();
            let entry = state.downloads.get_mut(&id).unwrap();
            Details::stream(&error).write(entry);
            Details::new("convert", "ffmpeg").write(entry);
            entry.private_session = Some("private".into());
            Details::stream(&error).write(entry);
        });
        let output = saved.0.lock().unwrap();
        let log = std::str::from_utf8(&output).unwrap();
        assert_eq!(log.matches("download failed").count(), 2, "{log}");
        for field in [
            "page.example",
            "cdn.example",
            "status=472",
            "segment=51",
            "segments=120",
            "ffmpeg",
        ] {
            assert!(log.contains(field), "{log}");
        }
        assert!(!log.contains("path-secret"), "{log}");
        assert!(!log.contains("query-secret"), "{log}");
    }
}

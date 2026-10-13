//! How the fields of an event or a span are written: as `tracing-subscriber`'s default writes them, with every value redacted as it is recorded. A `&str` is redacted before `Debug` escapes its quotes, and any other value as its `Debug` writes it, `Some(..)` and struct fields included. The finished line is redacted again on its way out.

use std::error::Error;
use std::fmt::{self, Debug};

use tracing::field::{Field, Visit};
use tracing_subscriber::field::RecordFields;
use tracing_subscriber::fmt::FormatFields;
use tracing_subscriber::fmt::format::Writer;

use crate::redact;

/// The field formatter of every binary's subscriber.
#[derive(Debug, Default)]
pub(crate) struct RedactedFields;

impl<'writer> FormatFields<'writer> for RedactedFields {
    fn format_fields<R: RecordFields>(&self, writer: Writer<'writer>, fields: R) -> fmt::Result {
        let mut visitor = Visitor {
            writer,
            result: Ok(()),
            first: true,
        };
        fields.record(&mut visitor);
        visitor.result
    }
}

/// Writes each field it is shown, `message` first and without its name, the rest as `name=value`, separated by spaces.
struct Visitor<'writer> {
    writer: Writer<'writer>,
    result: fmt::Result,
    first: bool,
}

impl Visitor<'_> {
    /// Writes the field `name` with `value`, redacted already, or with [`redact::HIDDEN`] when the name says the value is a secret (`password = %typed`).
    fn write(&mut self, name: &str, value: &str) {
        if self.result.is_err() {
            return;
        }
        let space = if self.first { "" } else { " " };
        self.first = false;
        let name = name.strip_prefix("r#").unwrap_or(name);
        self.result = if name == "message" {
            write!(self.writer, "{space}{value}")
        } else if redact::names_a_secret(name) {
            write!(self.writer, "{space}{name}={}", redact::HIDDEN)
        } else {
            write!(self.writer, "{space}{name}={value}")
        };
    }

    /// Whether `field` is one of the `log` crate's own, which the line shows already as its target and location.
    fn is_from_log(field: &Field) -> bool {
        field.name().starts_with("log.")
    }
}

impl Visit for Visitor<'_> {
    fn record_str(&mut self, field: &Field, value: &str) {
        if Self::is_from_log(field) {
            return;
        }
        let clean = redact::text_for_log(value);
        if field.name() == "message" {
            self.write(field.name(), &clean);
        } else {
            self.write(field.name(), &format!("{clean:?}"));
        }
    }

    fn record_error(&mut self, field: &Field, value: &(dyn Error + 'static)) {
        self.write(field.name(), &redact::text_for_log(&value.to_string()));
        let mut sources = Vec::new();
        let mut next = value.source();
        while let Some(source) = next {
            sources.push(redact::text_for_log(&source.to_string()));
            next = source.source();
        }
        if !sources.is_empty() {
            self.write(
                &format!("{}.sources", field.name()),
                &format!("[{}]", sources.join(", ")),
            );
        }
    }

    fn record_debug(&mut self, field: &Field, value: &dyn Debug) {
        if Self::is_from_log(field) {
            return;
        }
        self.write(field.name(), &redact::text_for_log(&format!("{value:?}")));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex, PoisonError};

    /// Collects what a subscriber writes, unredacted by any sink.
    #[derive(Clone, Default)]
    struct Collected(Arc<Mutex<Vec<u8>>>);

    impl std::io::Write for Collected {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    fn written(log: impl FnOnce()) -> String {
        let collected = Collected::default();
        let writer = collected.clone();
        let subscriber = tracing_subscriber::fmt()
            .with_ansi(false)
            .without_time()
            .with_target(false)
            .fmt_fields(RedactedFields)
            .with_writer(move || writer.clone())
            .finish();
        tracing::subscriber::with_default(subscriber, log);
        let text = collected
            .0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        String::from_utf8(text).unwrap()
    }

    #[test]
    fn each_value_is_redacted_as_it_is_recorded() {
        let text = written(|| {
            let body = r#"{"password":"a\"FIELDPW","user":"ada"}"#;
            let error = std::io::Error::other("https://ada:ERRPW@h.example/f");
            let job = tracing::info_span!("job", r#type = "media", key = "SPANKEY");
            job.in_scope(|| {
                tracing::warn!(
                    body,
                    error = &error as &(dyn Error + 'static),
                    attempts = 3,
                    "posting {}",
                    "https://h.example/?sig=MSGSIG"
                );
            });
        });
        for secret in ["FIELDPW", "ERRPW", "SPANKEY", "MSGSIG"] {
            assert!(!text.contains(secret), "{secret} was written:\n{text}");
        }
        for kept in [
            "job{type=\"media\" key=***}",
            "posting https://h.example/?sig=***",
            r#"\"user\":\"ada\""#,
            "error=https://ada:***@h.example/f",
            "attempts=3",
        ] {
            assert!(text.contains(kept), "{kept} is missing:\n{text}");
        }
    }
}

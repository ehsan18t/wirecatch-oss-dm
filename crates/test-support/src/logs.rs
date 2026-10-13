//! What the code under test logs, captured as structured events, so a test
//! can check what was logged by level, target and field rather than only by
//! the wording of a message.
//!
//! Each event carries the spans it was logged in, with their fields, as a log
//! line shows them: a value that reaches the logs only as a span field is
//! still in [`Logs::text`].
//!
//! ```no_run
//! use wirecatch_test_support::logs::Logs;
//!
//! let (logs, _capturing) = Logs::capture();
//! tracing::warn!(retry_in = ?std::time::Duration::from_secs(1), "request failed");
//! assert!(logs.events().iter().any(|e| e.field("retry_in").is_some()));
//! ```

use std::sync::{Arc, Mutex, PoisonError};

use tracing::field::{Field, Visit};
use tracing::span::{Attributes, Id, Record};
use tracing::subscriber::DefaultGuard;
use tracing::{Event, Level, Subscriber};
use tracing_subscriber::Registry;
use tracing_subscriber::layer::{Context, Layer, SubscriberExt as _};
use tracing_subscriber::registry::{LookupSpan, Scope};

/// Fields by name, each as `Debug` formats it, in the order they were recorded.
pub type Fields = Vec<(String, String)>;

/// One span an event was logged in.
#[derive(Clone, Debug)]
pub struct InSpan {
    /// Its name.
    pub name: String,
    /// Its fields, including those recorded after it was created.
    pub fields: Fields,
}

impl std::fmt::Display for InSpan {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}{{", self.name)?;
        for (i, (name, value)) in self.fields.iter().enumerate() {
            let separator = if i == 0 { "" } else { " " };
            write!(f, "{separator}{name}={value}")?;
        }
        write!(f, "}}")
    }
}

/// One event, as it was logged.
#[derive(Clone, Debug)]
pub struct Logged {
    /// Its level.
    pub level: Level,
    /// Its target: the module path of the code that logged it, unless it named another.
    pub target: String,
    /// The spans it was logged in, outermost first.
    pub spans: Vec<InSpan>,
    /// Its message, empty if it had none.
    pub message: String,
    /// Its other fields.
    pub fields: Fields,
}

impl Logged {
    /// The value of the event's own field `name`, if it has it.
    #[must_use]
    pub fn field(&self, name: &str) -> Option<&str> {
        self.fields
            .iter()
            .find(|(field, _)| field == name)
            .map(|(_, value)| value.as_str())
    }
}

impl std::fmt::Display for Logged {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} {}: ", self.level, self.target)?;
        for span in &self.spans {
            write!(f, "{span}: ")?;
        }
        write!(f, "{}", self.message)?;
        for (name, value) in &self.fields {
            write!(f, " {name}={value}")?;
        }
        Ok(())
    }
}

/// The events logged at every level while capturing, in order. Clones share
/// them.
#[derive(Clone, Default)]
pub struct Logs(Arc<Mutex<Vec<Logged>>>);

impl Logs {
    /// Captures what this thread logs until the guard is dropped. A test on a
    /// multi-threaded runtime logs from other threads too, which this does not
    /// see; it installs [`subscriber`](Self::subscriber) for the process
    /// instead, in a test binary of its own.
    #[must_use]
    pub fn capture() -> (Self, DefaultGuard) {
        let logs = Self::default();
        let guard = tracing::subscriber::set_default(logs.subscriber());
        (logs, guard)
    }

    /// A subscriber that records every event, at every level, into these logs.
    #[must_use]
    pub fn subscriber(&self) -> impl Subscriber + Send + Sync + 'static {
        Registry::default().with(Recorder(self.clone()))
    }

    /// Every event so far.
    #[must_use]
    pub fn events(&self) -> Vec<Logged> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Every event so far, one per line: level, target, the spans it was
    /// logged in with their fields, message and fields.
    #[must_use]
    pub fn text(&self) -> String {
        let mut text = String::new();
        for event in self.events() {
            text.push_str(&event.to_string());
            text.push('\n');
        }
        text
    }
}

/// The layer that records events into [`Logs`]. Each span keeps its fields in
/// its extensions, so an event can carry those of every span it is in.
struct Recorder(Logs);

impl<S> Layer<S> for Recorder
where
    S: Subscriber + for<'lookup> LookupSpan<'lookup>,
{
    fn on_new_span(&self, attributes: &Attributes<'_>, id: &Id, ctx: Context<'_, S>) {
        let mut fields = Fields::new();
        attributes.record(&mut Recording(&mut fields));
        if let Some(span) = ctx.span(id) {
            span.extensions_mut().insert(fields);
        }
    }

    fn on_record(&self, id: &Id, values: &Record<'_>, ctx: Context<'_, S>) {
        if let Some(span) = ctx.span(id) {
            let mut extensions = span.extensions_mut();
            if let Some(fields) = extensions.get_mut::<Fields>() {
                values.record(&mut Recording(fields));
            }
        }
    }

    fn on_event(&self, event: &Event<'_>, ctx: Context<'_, S>) {
        let metadata = event.metadata();
        let spans = ctx
            .event_scope(event)
            .into_iter()
            .flat_map(Scope::from_root)
            .map(|span| InSpan {
                name: span.name().to_owned(),
                fields: span
                    .extensions()
                    .get::<Fields>()
                    .cloned()
                    .unwrap_or_default(),
            })
            .collect();
        let mut fields = Fields::new();
        event.record(&mut Recording(&mut fields));
        let message = fields
            .iter()
            .position(|(name, _)| name == "message")
            .map(|at| fields.remove(at).1)
            .unwrap_or_default();
        let logged = Logged {
            level: *metadata.level(),
            target: metadata.target().to_owned(),
            spans,
            message,
            fields,
        };
        self.0
            .0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(logged);
    }
}

/// Reads the fields of an event or a span.
struct Recording<'a>(&'a mut Fields);

impl Visit for Recording<'_> {
    fn record_str(&mut self, field: &Field, value: &str) {
        self.0.push((field.name().to_owned(), value.to_owned()));
    }

    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        self.0.push((field.name().to_owned(), format!("{value:?}")));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_event_carries_its_message_and_its_own_fields() {
        let (logs, _capturing) = Logs::capture();
        tracing::warn!(attempt = 2, reason = "busy", "request failed");

        let events = logs.events();
        let [event] = events.as_slice() else {
            panic!("expected one event: {events:?}");
        };
        assert_eq!(event.level, Level::WARN);
        assert_eq!(event.message, "request failed");
        assert_eq!(event.field("attempt"), Some("2"));
        assert_eq!(event.field("reason"), Some("busy"));
        assert!(event.spans.is_empty(), "{:?}", event.spans);
    }

    /// A value that is only a span field, given when the span is created or
    /// recorded later, is in the event and in the text, as a log line shows it.
    #[test]
    fn an_event_carries_the_fields_of_every_span_it_is_in() {
        let (logs, _capturing) = Logs::capture();
        let outer = tracing::info_span!("session", host = "example.test");
        let inner = tracing::debug_span!(
            parent: &outer,
            "login",
            user = "alice",
            password = tracing::field::Empty
        );
        inner.record("password", "span-secret");
        inner.in_scope(|| tracing::debug!("logged in"));

        let events = logs.events();
        let [event] = events.as_slice() else {
            panic!("expected one event: {events:?}");
        };
        let spans: Vec<_> = event.spans.iter().map(ToString::to_string).collect();
        assert_eq!(
            spans,
            [
                "session{host=example.test}",
                "login{user=alice password=span-secret}"
            ]
        );
        assert_eq!(event.field("password"), None, "only the event's own fields");
        let text = logs.text();
        assert!(
            text.contains(
                "session{host=example.test}: login{user=alice password=span-secret}: logged in"
            ),
            "{text}"
        );
    }
}

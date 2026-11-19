//! Tasks and blocking work that stay in the caller's `tracing` span. A task tokio starts
//! otherwise has no span, so a subscriber that filters lines by span (an app keeping some
//! downloads out of its log) would see that task's lines as nobody's.

use std::future::Future;

use tokio::task::JoinHandle;
use tracing::Instrument as _;

/// [`tokio::spawn`], with the task in the caller's span.
pub fn spawn<F>(future: F) -> JoinHandle<F::Output>
where
    F: Future + Send + 'static,
    F::Output: Send + 'static,
{
    tokio::task::spawn(future.in_current_span())
}

/// [`tokio::task::spawn_blocking`], with the work in the caller's span.
pub fn spawn_blocking<T, F>(work: F) -> JoinHandle<T>
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    let span = tracing::Span::current();
    tokio::task::spawn_blocking(move || span.in_scope(work))
}

/// [`spawn`] on `runtime`, for a caller that may be on no runtime's thread.
pub fn spawn_on<F>(runtime: &tokio::runtime::Handle, future: F) -> JoinHandle<F::Output>
where
    F: Future + Send + 'static,
    F::Output: Send + 'static,
{
    runtime.spawn(future.in_current_span())
}

/// [`spawn_blocking`] on `runtime`, for a caller that may be on no runtime's thread.
pub fn spawn_blocking_on<T, F>(runtime: &tokio::runtime::Handle, work: F) -> JoinHandle<T>
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    let span = tracing::Span::current();
    runtime.spawn_blocking(move || span.in_scope(work))
}

/// `run`, on a thread of its own in the caller's span.
pub fn in_thread<T>(run: impl FnOnce() -> T + Send + 'static) -> impl FnOnce() -> T + Send + 'static
where
    T: Send + 'static,
{
    let span = tracing::Span::current();
    move || span.in_scope(run)
}

#[cfg(test)]
mod tests {
    use tracing::Instrument as _;
    use wirecatch_test_support::logs::Logs;

    #[tokio::test]
    async fn tasks_and_blocking_work_stay_in_the_callers_span() {
        let (logs, _capturing) = Logs::capture();
        // Other threads log to this capture too; the span is what they must carry over.
        let dispatch = tracing::dispatcher::get_default(Clone::clone);
        let logged = move |what: &'static str| {
            let dispatch = dispatch.clone();
            move || tracing::dispatcher::with_default(&dispatch, || tracing::warn!(what))
        };
        async {
            super::spawn(async { tracing::warn!("in a task") })
                .await
                .unwrap();
            super::spawn_blocking(logged("blocking")).await.unwrap();
            std::thread::spawn(super::in_thread(logged("in a thread")))
                .join()
                .unwrap();
        }
        .instrument(tracing::error_span!("caller"))
        .await;
        let spans: Vec<Vec<String>> = logs
            .events()
            .iter()
            .map(|event| event.spans.iter().map(|span| span.name.clone()).collect())
            .collect();
        assert_eq!(spans, vec![vec!["caller".to_owned()]; 3]);
    }
}

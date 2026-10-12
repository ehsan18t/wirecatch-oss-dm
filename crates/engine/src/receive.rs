//! Waiting for the next piece of a response body, the same way for a
//! download's connections and for the [`Fetcher`](crate::Fetcher).

use std::time::Duration;

use tokio_util::sync::CancellationToken;

use crate::error::{Error, Result};
use crate::numeric::clamp_to_len;

/// What waiting for the next piece of a body gave.
pub(crate) enum Received<T> {
    /// The next piece.
    Data(T),
    /// The body ended normally.
    End,
    /// The token was cancelled while waiting.
    Cancelled,
}

/// Waits for `next`, the next piece of a body, at most `stall`
/// ([`Error::Stalled`] after that), and not at all once `cancel` is cancelled.
pub(crate) async fn receive<T>(
    cancel: &CancellationToken,
    stall: Duration,
    next: impl Future<Output = Result<Option<T>>>,
) -> Result<Received<T>> {
    tokio::select! {
        biased;
        () = cancel.cancelled() => Ok(Received::Cancelled),
        next = tokio::time::timeout(stall, next) => match next {
            Err(_) => Err(Error::Stalled { after: stall }),
            Ok(Ok(Some(data))) => Ok(Received::Data(data)),
            Ok(Ok(None)) => Ok(Received::End),
            Ok(Err(e)) => Err(e),
        },
    }
}

/// `data` without what is left of the first `skip` bytes of the body, which a
/// server that ignores ranges sends before the ones wanted; `skip` goes down by
/// what was dropped.
pub(crate) fn skip_start<'a>(data: &'a [u8], skip: &mut u64) -> &'a [u8] {
    let n = clamp_to_len(*skip, data.len());
    *skip -= n as u64;
    &data[n..]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skipping_spans_pieces() {
        let mut skip = 5;
        assert_eq!(skip_start(b"abc", &mut skip), b"");
        assert_eq!(skip_start(b"defgh", &mut skip), b"fgh");
        assert_eq!(skip_start(b"ij", &mut skip), b"ij");
        assert_eq!(skip, 0);
    }
}

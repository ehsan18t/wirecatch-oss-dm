//! Scans over text that look at each byte once, and in tests count how many they looked at.

/// The first position from `from` whose byte is one `stop` picks, or the end of `bytes`.
pub(super) fn scan_until(bytes: &[u8], from: usize, stop: impl Fn(u8) -> bool) -> usize {
    let found = bytes
        .get(from..)
        .and_then(|rest| rest.iter().position(|b| stop(*b)))
        .map_or(bytes.len().max(from), |at| from + at);
    steps::add(found.saturating_sub(from) + 1);
    found
}

/// Where `needle` next starts in `bytes` from `from`.
pub(super) fn find(bytes: &[u8], mut from: usize, needle: &[u8]) -> Option<usize> {
    let first = *needle.first()?;
    loop {
        let at = scan_until(bytes, from, |b| b == first);
        if at >= bytes.len() {
            return None;
        }
        if bytes.get(at..at + needle.len()) == Some(needle) {
            return Some(at);
        }
        from = at + 1;
    }
}

/// The next position a scan stops at, kept while the scan moves forward so no stretch of text is scanned twice.
#[derive(Debug, Default)]
pub(super) struct NextAt(Option<usize>);

impl NextAt {
    /// The position `scan` finds from `from`: the one found before when it is not behind `from`, since the scan found nothing between.
    pub(super) fn from(&mut self, from: usize, scan: impl FnOnce(usize) -> usize) -> usize {
        match self.0 {
            Some(found) if found >= from => found,
            _ => *self.0.insert(scan(from)),
        }
    }
}

/// Outside tests, counting the bytes scanned costs nothing.
#[cfg(not(test))]
pub(super) mod steps {
    pub(in crate::redact) fn add(_: usize) {}
}

/// How many bytes the scans looked at, counted in tests to hold the redaction to linear time.
#[cfg(test)]
pub(super) mod steps {
    use std::cell::Cell;

    thread_local! {
        static TAKEN: Cell<usize> = const { Cell::new(0) };
    }

    pub(in crate::redact) fn add(n: usize) {
        TAKEN.with(|taken| taken.set(taken.get().saturating_add(n)));
    }

    pub(in crate::redact) fn reset() {
        TAKEN.with(|taken| taken.set(0));
    }

    pub(in crate::redact) fn taken() -> usize {
        TAKEN.with(Cell::get)
    }
}

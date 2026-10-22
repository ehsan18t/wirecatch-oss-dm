import assert from 'node:assert/strict';
import { test } from 'node:test';
import { timingRaces } from './test-timing.mjs';

const TEST_FILE = 'crates/engine/tests/download.rs';

/** The lines `timingRaces` reports for `text`. */
function lines(text, file = TEST_FILE) {
  return timingRaces(file, text).map((hit) => hit.line);
}

test('an upper bound on elapsed time in a test is a race', () => {
  const text = [
    '#[tokio::test]',
    'async fn fails_fast() {',
    '    let started = Instant::now();',
    '    run().await;',
    '    assert!(started.elapsed() < Duration::from_secs(2), "fails fast");',
    '}',
  ].join('\n');
  assert.deepEqual(lines(text), [5]);
});

test('a duration kept in a name and bounded later is a race, written either way round', () => {
  const text = [
    'async fn quick() {',
    '    let started = Instant::now();',
    '    run().await;',
    '    let took = started.elapsed();',
    '    assert!(took >= LEAST);',
    '    assert!(took < MOST, "took {took:?}");',
    '    assert!(MOST > took);',
    '    let secs = start.elapsed().as_secs_f64();',
    '    assert!(secs <= 0.6);',
    '}',
  ].join('\n');
  assert.deepEqual(lines(text), [6, 7, 9]);
});

test('a range with an upper end around a duration is a race; one without is not', () => {
  const text = [
    'fn bounded() {',
    '    let took = start.elapsed();',
    '    assert!((LEAST..MOST).contains(&took));',
    '    assert!((LEAST..=MOST).contains(&took));',
    '    assert!((LEAST..).contains(&took));',
    '}',
  ].join('\n');
  assert.deepEqual(lines(text), [3, 4]);
});

test('the gap between two instants a server recorded is measured too', () => {
  const text = [
    'fn retried() {',
    '    let waited = seen[1].at - seen[0].at;',
    '    assert!(waited >= Duration::from_secs(1));',
    '    assert!(waited < Duration::from_secs(3));',
    '}',
  ].join('\n');
  assert.deepEqual(lines(text), [4]);
});

test('a window in which something must not happen is not a race', () => {
  const text = [
    'fn quiet() {',
    '    let start = Instant::now();',
    '    while start.elapsed() < window {',
    '        assert!(nothing_happened());',
    '    }',
    '}',
  ].join('\n');
  assert.deepEqual(lines(text), []);
});

test('a test on the paused clock may bound time from above', () => {
  const text = [
    '#[tokio::test(start_paused = true)]',
    'async fn holds_the_limit() {',
    '    let elapsed = start.elapsed().as_secs_f64();',
    '    assert!((9.5..=10.1).contains(&elapsed));',
    '}',
    '#[tokio::test]',
    'async fn races() {',
    '    assert!(start.elapsed() < LIMIT);',
    '}',
  ].join('\n');
  assert.deepEqual(lines(text), [8]);
});

test('only test code is read: product code and comments may compare elapsed time', () => {
  const text = [
    'fn connect() {',
    '    if started.elapsed() < BUSY_WAIT { retry(); }',
    '}',
    '// assert!(started.elapsed() < LIMIT);',
    '#[cfg(test)]',
    'mod tests {',
    '    fn races() {',
    '        let text = "started.elapsed() < LIMIT";',
    '        assert!(started.elapsed() < LIMIT);',
    '    }',
    '}',
  ].join('\n');
  assert.deepEqual(lines(text, 'crates/ipc/src/windows.rs'), [9]);
});

test('a name a test reuses for something else is not taken for a duration', () => {
  const text = [
    'fn first() {',
    '    let took = start.elapsed();',
    '    assert!(took >= LEAST);',
    '}',
    'fn second() {',
    '    let took = attempts.len();',
    '    assert!(took < 3);',
    '}',
  ].join('\n');
  assert.deepEqual(lines(text), []);
});

test('a duration a helper measures and returns is measured where it is called, also in a tuple', () => {
  const text = [
    'async fn time_download() -> TestResult<Duration> {',
    '    let start = Instant::now();',
    '    run().await;',
    '    Ok(start.elapsed())',
    '}',
    'async fn fetch() -> (Vec<u8>, Duration) {',
    '    (bytes, started.elapsed())',
    '}',
    'async fn obeys_the_limit() {',
    '    let took = time_download().await?;',
    '    assert!((LEAST..MOST).contains(&took));',
    '    let (bytes, took) = fetch().await;',
    '    assert!(took < MOST);',
    '}',
  ].join('\n');
  assert.deepEqual(lines(text), [11, 13]);
});

test('a module behind cfg(all(test, ..)) is test code', () => {
  const text = [
    '#[cfg(all(test, windows))]',
    'mod tests {',
    '    fn races() {',
    '        assert!(started.elapsed() < LIMIT);',
    '    }',
    '}',
  ].join('\n');
  assert.deepEqual(lines(text, 'crates/ipc/src/windows.rs'), [4]);
});

test('a short fixed timeout on a wait that has to succeed is a race; one that must run out is not', () => {
  const text = [
    'async fn waits() {',
    '    let ended = timeout(Duration::from_secs(2), relay)',
    '        .await',
    '        .expect("the relay ended");',
    '    let answer = ext.outcome_within(list, Duration::from_secs(1)).await;',
    '    assert!(',
    '        timeout(Duration::from_millis(300), listener.accept())',
    '            .await',
    '            .is_err()',
    '    );',
    '    timeout(PATIENCE, relay).await.unwrap();',
    '    let h = Harness::with_capture_timeout(Duration::from_millis(300)).await?;',
    '}',
  ].join('\n');
  assert.deepEqual(lines(text), [2, 5]);
});

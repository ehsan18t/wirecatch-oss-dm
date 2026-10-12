//! SQLite persistence (D17). One connection lives on a dedicated thread; async code
//! hands it writes through a channel and never waits on the disk.

use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use rusqlite::config::DbConfig;
use rusqlite::{Connection, OptionalExtension, Row, Transaction, params};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use tokio::sync::oneshot;
use tracing::{error, warn};
use wirecatch_protocol::app::{AppSettings, DownloadStatus, DownloadSummary, MediaSummary, Queue};
use wirecatch_protocol::extension::Header;
use wirecatch_protocol::extension::MediaKind;
use wirecatch_protocol::{DownloadId, QueueId};

use crate::error::{CoreError, Result};
use crate::media::StoredMedia;
use crate::owner_only;

/// File name of the database inside the data folder.
pub const DATABASE_FILE: &str = "wirecatch.db";

/// Schema changes, applied in order. Entry `n` moves the schema to version `n + 1`.
/// Never edit a released entry; append a new one.
const MIGRATIONS: &[&str] = &[
    r#"
    CREATE TABLE queues (
        id             INTEGER PRIMARY KEY,
        position       INTEGER NOT NULL,
        name           TEXT    NOT NULL,
        max_concurrent INTEGER NOT NULL,
        speed_limit    INTEGER,
        schedule       TEXT,
        is_default     INTEGER NOT NULL
    );
    CREATE TABLE downloads (
        id           INTEGER PRIMARY KEY,
        url          TEXT    NOT NULL,
        path         TEXT    NOT NULL,
        queue_id     INTEGER NOT NULL,
        position     INTEGER NOT NULL,
        category     TEXT    NOT NULL,
        status       TEXT    NOT NULL,
        total        INTEGER,
        done         INTEGER NOT NULL,
        error        TEXT,
        added_at     INTEGER NOT NULL,
        completed_at INTEGER,
        origin       TEXT    NOT NULL
    );
    CREATE TABLE settings (
        key   TEXT PRIMARY KEY,
        value TEXT NOT NULL
    );
"#,
    r#"
    ALTER TABLE queues ADD COLUMN paused INTEGER NOT NULL DEFAULT 0;
    ALTER TABLE queues ADD COLUMN running_outside_schedule INTEGER NOT NULL DEFAULT 0;
"#,
    r#"
    ALTER TABLE downloads ADD COLUMN settled INTEGER NOT NULL DEFAULT 1;
    ALTER TABLE downloads ADD COLUMN chosen_name TEXT;
    ALTER TABLE downloads ADD COLUMN chosen_folder TEXT;
"#,
    r#"
    ALTER TABLE downloads ADD COLUMN media TEXT;
"#,
    // D23 (2026-10-11): every browser download now opens its own window before it starts, so
    // `ask_before_download` is on by default. That default changed before the first release,
    // while stored settings still held the old one (off) that nobody chose on purpose, so the
    // setting is switched on once, here. Turning it off afterwards is kept: this runs once.
    r#"
    UPDATE settings
    SET value = json_set(value, '$.extension.ask_before_download', json('true'))
    WHERE key = 'app' AND json_valid(value);
"#,
];

const SETTINGS_KEY: &str = "app";
const LAST_IDS_KEY: &str = "last_ids";

/// The highest download and queue ids ever handed out. Kept so an id is never
/// reused, even after the newest download or queue was deleted.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct LastIds {
    pub download: i64,
    pub queue: i64,
}

/// What a download needs from the browser to keep working after a restart:
/// cookies, referrer and other headers, and the user agent.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Origin {
    pub user_agent: Option<String>,
    pub headers: Vec<Header>,
    /// Its cookies and credentials were deleted because it did not finish
    /// within a week, so a failure that needs a fresh link says so.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub forgot_sign_in: bool,
}

/// One download as stored.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct StoredDownload {
    pub summary: DownloadSummary,
    /// Order within its queue; lower runs first.
    pub position: i64,
    pub origin: Origin,
    pub naming: NameChoice,
    /// For video and audio: what the row shows and, for streams and pages, what
    /// to download (kept so it continues after a restart).
    pub media: Option<StoredMedia>,
}

/// How a download's final name and folder get decided.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NameChoice {
    /// The final path is fixed: the server has answered with the file. Until then
    /// the download's path is a provisional guess made when it was added.
    pub settled: bool,
    /// A name the user or the browser chose; it wins over the server's suggestion.
    pub name: Option<String>,
    /// A folder the user chose; otherwise the category rule decides.
    pub folder: Option<String>,
}

impl NameChoice {
    /// A download whose path is already final.
    #[cfg(test)]
    pub fn settled() -> Self {
        Self {
            settled: true,
            name: None,
            folder: None,
        }
    }
}

/// Everything loaded at startup.
#[derive(Debug, Default)]
pub(crate) struct Snapshot {
    pub settings: Option<AppSettings>,
    pub last_ids: LastIds,
    pub queues: Vec<Queue>,
    pub downloads: Vec<StoredDownload>,
    /// What had to be repaired to open the list, for the person to be told.
    pub recovery: Option<Recovery>,
}

/// A repair made while opening the download list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Recovery {
    /// The database file was damaged. It was moved to `kept_at`, with its journal files next to it under the names SQLite pairs with it, and a new, empty list was started.
    Damaged {
        /// Where the damaged file now is.
        kept_at: PathBuf,
    },
    /// This many downloads could not be read (a newer version may have saved them) and are kept, hidden, for a version that can, without their sign-in. Counted once: on the start that first found them.
    UnreadableKept(usize),
}

/// Suffix of a damaged database moved aside, followed by the Unix time in milliseconds.
const DAMAGED_SUFFIX: &str = ".damaged-";
/// Suffix of the copy taken before a schema upgrade, followed by the schema version it holds.
const BACKUP_SUFFIX: &str = ".bak-v";
/// SQLite's own files next to the database, which belong to it and move with it.
const SIDE_FILES: [&str; 2] = ["-wal", "-shm"];
/// How long a damaged list set aside, or a copy taken before an upgrade, is kept. Both hold every download's cookies, which a live download forgets after a week (D17); a month leaves time to recover a list by hand.
const KEEP_COPIES_FOR: Duration = Duration::from_secs(30 * 24 * 60 * 60);

/// Told why saving failed after every retry.
pub(crate) type FailureHandler = Box<dyn Fn(String) + Send>;

/// How long a write waits for a lock held by someone else (a backup tool, say).
const BUSY_TIMEOUT: Duration = Duration::from_secs(5);
/// Pauses before each retry of a failed batch of writes.
const RETRY_PAUSES: [Duration; 3] = [
    Duration::from_millis(50),
    Duration::from_millis(250),
    Duration::from_secs(1),
];
/// How long kept writes wait for a retry when no new writes arrive.
const RETRY_KEPT_AFTER: Duration = Duration::from_secs(5);
/// Most failed writes kept for a later try.
const MAX_KEPT: usize = 10_000;

enum Op {
    SaveDownloads(Vec<StoredDownload>),
    DeleteDownloads(Vec<DownloadId>),
    SaveQueues(Vec<Queue>),
    SaveSettings(Box<AppSettings>),
    SaveLastIds(LastIds),
    #[cfg_attr(not(test), allow(dead_code))]
    Flush(oneshot::Sender<()>),
    /// Saves everything sent before it, closes the database and stops the thread.
    Close(oneshot::Sender<()>),
}

/// Handle to the database thread. Writes are applied in the order they are sent.
#[derive(Debug, Clone)]
pub(crate) struct Store {
    ops: mpsc::Sender<Op>,
}

impl std::fmt::Debug for Op {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Op::SaveDownloads(_) => "SaveDownloads",
            Op::DeleteDownloads(_) => "DeleteDownloads",
            Op::SaveQueues(_) => "SaveQueues",
            Op::SaveSettings(_) => "SaveSettings",
            Op::SaveLastIds(_) => "SaveLastIds",
            Op::Flush(_) => "Flush",
            Op::Close(_) => "Close",
        })
    }
}

impl Store {
    /// Opens (or creates) the database in `dir`, brings its schema up to date and
    /// loads everything. Blocks on the disk, so call it from a blocking thread.
    ///
    /// `on_failure` is called (on the database thread) with the reason whenever a
    /// batch of writes could not be saved even after retrying.
    ///
    /// A database file SQLite reports as damaged is moved aside and a new list is started, so a power cut or a bad sync tool never stops Wirecatch from opening; [`Snapshot::recovery`] says so.
    pub fn open(dir: &Path, on_failure: FailureHandler) -> Result<(Store, Snapshot)> {
        std::fs::create_dir_all(dir).map_err(|source| CoreError::File {
            path: dir.display().to_string(),
            source,
        })?;
        // The folder first, so the database and its journal files inherit its access.
        restrict_to_owner(dir);
        let path = dir.join(DATABASE_FILE);
        remove_old_copies(&path);
        // Damage can also show while reading the rows, after the file opened.
        let opened = open_database(&path).and_then(|conn| Ok((load(&conn)?, conn)));
        let (mut snapshot, conn, damaged) = match opened {
            Ok((snapshot, conn)) => (snapshot, conn, None),
            Err(CoreError::Database(e)) if is_damage(&e) => {
                error!(error = %e, "the download list is damaged; setting it aside and starting a new one");
                let kept_at = set_aside(&path)?;
                let conn = open_database(&path)?;
                (load(&conn)?, conn, Some(Recovery::Damaged { kept_at }))
            }
            Err(e) => return Err(e),
        };
        snapshot.recovery = damaged.or(snapshot.recovery);

        let (ops, rx) = mpsc::channel();
        thread::Builder::new()
            .name("wirecatch-db".into())
            .spawn(move || run(conn, rx, on_failure))
            .map_err(|source| CoreError::File {
                path: path.display().to_string(),
                source,
            })?;
        Ok((Store { ops }, snapshot))
    }

    /// Inserts or replaces these downloads.
    pub fn save_downloads(&self, downloads: Vec<StoredDownload>) {
        if !downloads.is_empty() {
            self.send(Op::SaveDownloads(downloads));
        }
    }

    /// Deletes these downloads.
    pub fn delete_downloads(&self, ids: Vec<DownloadId>) {
        if !ids.is_empty() {
            self.send(Op::DeleteDownloads(ids));
        }
    }

    /// Replaces every queue with `queues`, keeping their order.
    pub fn save_queues(&self, queues: Vec<Queue>) {
        self.send(Op::SaveQueues(queues));
    }

    /// Replaces the stored settings.
    pub fn save_settings(&self, settings: AppSettings) {
        self.send(Op::SaveSettings(Box::new(settings)));
    }

    /// Records the highest ids handed out.
    pub fn save_last_ids(&self, ids: LastIds) {
        self.send(Op::SaveLastIds(ids));
    }

    /// Waits until every write sent so far is on disk.
    #[cfg(test)]
    pub async fn flush(&self) {
        let (done, wait) = oneshot::channel();
        self.send(Op::Flush(done));
        let _ = wait.await;
    }

    /// Saves every write sent so far, closes the database file and stops the
    /// database thread, waiting at most `timeout` for it. Afterwards the data folder
    /// can be deleted or moved at once (Windows refuses while a file in it is open),
    /// and later writes are dropped.
    pub async fn close(&self, timeout: Duration) {
        let (done, wait) = oneshot::channel();
        self.send(Op::Close(done));
        if tokio::time::timeout(timeout, wait).await.is_err() {
            warn!(?timeout, "the database did not close in time");
        }
    }

    fn send(&self, op: Op) {
        if let Err(mpsc::SendError(op)) = self.ops.send(op) {
            // After `close` this is expected: the app is shutting down.
            warn!(?op, "the database is closed; change not saved");
        }
    }
}

/// Applies writes until every [`Store`] handle is gone. Writes that queued up while
/// the disk was busy are committed together in one transaction. A batch that fails
/// is retried after a short pause; if it still fails, its writes are tried one by
/// one, so one bad write cannot take the others down with it, and `on_failure` is
/// told why. Writes that still fail are kept, in order, and tried again before
/// the next writes (or after a while if none come), never dropped: a lost delete
/// would bring back a removed download.
fn run(conn: Connection, ops: mpsc::Receiver<Op>, on_failure: FailureHandler) {
    let (conn, closed) = write_until_closed(conn, ops, on_failure);
    // Closing the connection checkpoints the journal and releases the files.
    if let Err((_, e)) = conn.close() {
        warn!(error = %e, "the database did not close cleanly");
    }
    if let Some(closed) = closed {
        let _ = closed.send(());
    }
}

/// The body of [`run`]: applies writes until a `Close` arrives or every handle is
/// gone, and returns the connection with the waiting `Close`, if any.
fn write_until_closed(
    mut conn: Connection,
    ops: mpsc::Receiver<Op>,
    on_failure: FailureHandler,
) -> (Connection, Option<oneshot::Sender<()>>) {
    let mut kept: Vec<Op> = Vec::new();
    loop {
        let first = if kept.is_empty() {
            match ops.recv() {
                Ok(op) => Some(op),
                Err(mpsc::RecvError) => return (conn, None),
            }
        } else {
            match ops.recv_timeout(RETRY_KEPT_AFTER) {
                Ok(op) => Some(op),
                Err(mpsc::RecvTimeoutError::Timeout) => None,
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    // Closing: one last try, then whatever still fails is lost.
                    let lost = apply_one_by_one(&mut conn, std::mem::take(&mut kept));
                    if !lost.is_empty() {
                        error!(
                            writes = lost.len(),
                            "changes could not be saved before closing"
                        );
                    }
                    return (conn, None);
                }
            }
        };
        let mut writes = std::mem::take(&mut kept);
        let retried = writes.len();
        let mut flushed = Vec::new();
        let mut closing = None;
        for op in first.into_iter().chain(ops.try_iter()) {
            match op {
                Op::Flush(done) => flushed.push(done),
                Op::Close(done) => {
                    // Writes sent after the close are not taken.
                    closing = Some(done);
                    break;
                }
                write => writes.push(write),
            }
        }
        if !writes.is_empty()
            && let Err(e) = apply_with_retries(&mut conn, &writes)
        {
            error!(error = %e, "could not save changes to the database");
            let fresh = writes.len() > retried;
            kept = apply_one_by_one(&mut conn, writes);
            if kept.len() > MAX_KEPT {
                error!(
                    dropped = kept.len() - MAX_KEPT,
                    "too many changes could not be saved; dropping the oldest"
                );
                kept.drain(..kept.len() - MAX_KEPT);
            }
            // A quiet retry of writes already reported is not news.
            if fresh {
                on_failure(e.to_string());
            }
        }
        for done in flushed {
            let _ = done.send(());
        }
        if let Some(done) = closing {
            if !kept.is_empty() {
                let lost = apply_one_by_one(&mut conn, std::mem::take(&mut kept));
                if !lost.is_empty() {
                    error!(
                        writes = lost.len(),
                        "changes could not be saved before closing"
                    );
                }
            }
            return (conn, Some(done));
        }
    }
}

/// Applies each write on its own and returns the ones that failed, in order. A
/// write is also held back when an earlier write to the same row or setting
/// failed, so that writes to one thing never land out of order.
fn apply_one_by_one(conn: &mut Connection, writes: Vec<Op>) -> Vec<Op> {
    let mut failed = Vec::new();
    let mut blocked: Vec<Target> = Vec::new();
    for write in writes.into_iter().flat_map(split) {
        let target = target_of(&write);
        if blocked.contains(&target) {
            failed.push(write);
            continue;
        }
        if let Err(e) = apply(conn, std::slice::from_ref(&write)) {
            warn!(error = %e, ?write, "a change could not be saved; keeping it to try again");
            blocked.push(target);
            failed.push(write);
        }
    }
    failed
}

/// Splits a write about several downloads into one write per download.
fn split(op: Op) -> Vec<Op> {
    match op {
        Op::SaveDownloads(rows) => rows
            .into_iter()
            .map(|row| Op::SaveDownloads(vec![row]))
            .collect(),
        Op::DeleteDownloads(ids) => ids
            .into_iter()
            .map(|id| Op::DeleteDownloads(vec![id]))
            .collect(),
        other => vec![other],
    }
}

/// What a single write changes.
#[derive(Debug, PartialEq, Eq)]
enum Target {
    Download(Option<DownloadId>),
    Queues,
    Settings,
    LastIds,
    Nothing,
}

fn target_of(op: &Op) -> Target {
    match op {
        Op::SaveDownloads(rows) => Target::Download(rows.first().map(|r| r.summary.id)),
        Op::DeleteDownloads(ids) => Target::Download(ids.first().copied()),
        Op::SaveQueues(_) => Target::Queues,
        Op::SaveSettings(_) => Target::Settings,
        Op::SaveLastIds(_) => Target::LastIds,
        Op::Flush(_) | Op::Close(_) => Target::Nothing,
    }
}

fn apply_with_retries(conn: &mut Connection, writes: &[Op]) -> rusqlite::Result<()> {
    let mut pauses = RETRY_PAUSES.iter();
    loop {
        match apply(conn, writes) {
            Ok(()) => return Ok(()),
            Err(e) => match pauses.next() {
                Some(pause) => {
                    warn!(error = %e, retry_in = ?pause, "saving failed; trying again");
                    thread::sleep(*pause);
                }
                None => return Err(e),
            },
        }
    }
}

fn apply(conn: &mut Connection, writes: &[Op]) -> rusqlite::Result<()> {
    let tx = conn.transaction()?;
    for op in writes {
        match op {
            Op::SaveDownloads(rows) => rows.iter().try_for_each(|row| save_download(&tx, row))?,
            Op::DeleteDownloads(ids) => {
                let mut stmt = tx.prepare_cached("DELETE FROM downloads WHERE id = ?1")?;
                for id in ids {
                    stmt.execute([id.0])?;
                }
            }
            Op::SaveQueues(queues) => save_queues(&tx, queues)?,
            Op::SaveSettings(settings) => write_value(&tx, SETTINGS_KEY, &**settings)?,
            Op::SaveLastIds(ids) => write_value(&tx, LAST_IDS_KEY, ids)?,
            Op::Flush(_) | Op::Close(_) => {}
        }
    }
    tx.commit()
}

/// Connection settings. Deleted rows (which may have held cookies) are overwritten
/// with zeros rather than left in free pages.
fn configure(conn: &Connection) -> rusqlite::Result<()> {
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    conn.pragma_update(None, "secure_delete", "ON")?;
    Ok(())
}

/// Opens the database at `path`, checks it is intact and brings its schema up to date.
fn open_database(path: &Path) -> Result<Connection> {
    let mut conn = Connection::open(path)?;
    restrict_to_owner(path);
    conn.busy_timeout(BUSY_TIMEOUT)?;
    // Until the file is known to be intact, closing the connection must not fold the journal into it: a damaged list is set aside with its journal as it was found. The check also comes before `configure`, which writes to the file to switch its journal mode.
    conn.set_db_config(DbConfig::SQLITE_DBCONFIG_NO_CKPT_ON_CLOSE, true)?;
    check_integrity(&conn)?;
    conn.set_db_config(DbConfig::SQLITE_DBCONFIG_NO_CKPT_ON_CLOSE, false)?;
    configure(&conn)?;
    migrate(&mut conn, path)?;
    Ok(conn)
}

/// SQLite's full check, stopping at the first problem. A damaged file can open and answer the first queries, then fail later, so this runs before anything is read; unlike `quick_check` it also compares the indexes with the rows. The list is small, so it costs little.
fn check_integrity(conn: &Connection) -> rusqlite::Result<()> {
    let verdict: String = conn.query_row("PRAGMA integrity_check(1)", [], |row| row.get(0))?;
    if verdict == "ok" {
        Ok(())
    } else {
        Err(rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_CORRUPT),
            Some(verdict),
        ))
    }
}

/// Whether an error means the file itself is damaged, as opposed to a full disk or a locked file, which a later start can get past.
fn is_damage(error: &rusqlite::Error) -> bool {
    matches!(
        error.sqlite_error_code(),
        Some(rusqlite::ErrorCode::NotADatabase | rusqlite::ErrorCode::DatabaseCorrupt)
    )
}

/// `path` with `suffix` added to its file name.
fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}

/// Moves a damaged database to `<name>.damaged-<unix ms>` and its side files next to it as `<that name>-wal` and `-shm`, the names SQLite pairs with it, so the kept list opens with what was still in its journal. Either everything moves or nothing does: when one move fails (a sync tool holding a file, say), the ones before it are undone, and the next start tries again. Returns where the database went.
fn set_aside(path: &Path) -> Result<PathBuf> {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_millis());
    let kept_at = with_suffix(path, &format!("{DAMAGED_SUFFIX}{stamp}"));
    let mut moves = vec![(path.to_path_buf(), kept_at.clone())];
    for side in SIDE_FILES {
        let from = with_suffix(path, side);
        if from.exists() {
            moves.push((from, with_suffix(&kept_at, side)));
        }
    }
    for (done, (from, to)) in moves.iter().enumerate() {
        if let Err(source) = std::fs::rename(from, to) {
            for (back_from, back_to) in moves[..done].iter().rev() {
                if let Err(e) = std::fs::rename(back_to, back_from) {
                    error!(error = %e, file = %back_to.display(), "could not move a file of the damaged list back");
                }
            }
            return Err(CoreError::File {
                path: from.display().to_string(),
                source,
            });
        }
    }
    Ok(kept_at)
}

/// Deletes damaged lists set aside and copies taken before an upgrade once they are older than [`KEEP_COPIES_FOR`], with their side files. Failures are only logged: an old copy left behind never stops a start.
fn remove_old_copies(path: &Path) {
    let (Some(dir), Some(name)) = (path.parent(), path.file_name().and_then(|n| n.to_str())) else {
        return;
    };
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let copy_prefixes = [
        format!("{name}{DAMAGED_SUFFIX}"),
        format!("{name}{BACKUP_SUFFIX}"),
    ];
    for entry in entries.flatten() {
        let file_name = entry.file_name();
        let Some(file_name) = file_name.to_str() else {
            continue;
        };
        if !copy_prefixes
            .iter()
            .any(|prefix| file_name.starts_with(prefix))
        {
            continue;
        }
        let old = entry
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|modified| modified.elapsed().ok())
            .is_some_and(|age| age > KEEP_COPIES_FOR);
        if old && let Err(e) = std::fs::remove_file(entry.path()) {
            warn!(error = %e, file = file_name, "could not delete an old copy of the download list");
        }
    }
}

/// Copies the database to `<name>.bak-v<version>` before its schema changes, so a faulty upgrade can be undone by hand. `VACUUM INTO` writes a consistent copy, including what is still in the write-ahead log.
///
/// Best effort: when the copy cannot be made (a nearly full disk has no room for a second list, say), the upgrade goes ahead without it, logged, and what was partly written is removed. Each upgrade step runs in a transaction, so a failing one leaves the list as it was; the copy only guards against a step that succeeds but is wrong, and refusing to start would cost the person more than that.
fn back_up_before_upgrade(conn: &Connection, path: &Path, version: u32) {
    let backup = with_suffix(path, &format!("{BACKUP_SUFFIX}{version}"));
    let remove = |what: &str| match std::fs::remove_file(&backup) {
        Ok(()) => true,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => true,
        Err(e) => {
            warn!(error = %e, file = %backup.display(), "could not remove {what}");
            false
        }
    };
    if !remove("an earlier copy of the download list") {
        warn!("upgrading the download list without a copy");
        return;
    }
    match conn.execute("VACUUM INTO ?1", [backup.to_string_lossy()]) {
        Ok(_) => restrict_to_owner(&backup),
        Err(e) => {
            warn!(error = %e, "could not copy the download list before upgrading it; upgrading without a copy");
            remove("a partial copy of the download list");
        }
    }
}

/// The schema version every migration together leads to, as SQLite's
/// `user_version` stores it. A list of 2^32 migrations does not compile, so the
/// conversion below never falls back.
fn schema_version() -> u32 {
    const { assert!(MIGRATIONS.len() <= u32::MAX as usize) };
    u32::try_from(MIGRATIONS.len()).unwrap_or(u32::MAX)
}

fn migrate(conn: &mut Connection, path: &Path) -> Result<()> {
    let supported = schema_version();
    let found: u32 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if found > supported {
        return Err(CoreError::DatabaseTooNew { found, supported });
    }
    // A new, empty database has nothing worth a copy.
    if found > 0 && found < supported {
        back_up_before_upgrade(conn, path, found);
    }
    // Migration n (from 1) leads to version n.
    for (version, sql) in (1u32..).zip(MIGRATIONS).filter(|&(v, _)| v > found) {
        let tx = conn.transaction()?;
        tx.execute_batch(sql)?;
        tx.pragma_update(None, "user_version", version)?;
        tx.commit()?;
    }
    Ok(())
}

/// A JSON value from the key-value table, or `None` if it is missing or unreadable.
fn read_value<T: DeserializeOwned>(
    conn: &Connection,
    key: &str,
    what: &str,
) -> rusqlite::Result<Option<T>> {
    Ok(conn
        .query_row("SELECT value FROM settings WHERE key = ?1", [key], |row| {
            row.get::<_, String>(0)
        })
        .optional()?
        .and_then(|json| from_json(&json, what)))
}

fn write_value<T: Serialize>(tx: &Transaction<'_>, key: &str, value: &T) -> rusqlite::Result<()> {
    tx.prepare_cached("INSERT OR REPLACE INTO settings (key, value) VALUES (?1, ?2)")?
        .execute(params![key, to_json(value)])?;
    Ok(())
}

fn load(conn: &Connection) -> Result<Snapshot> {
    let settings = read_value(conn, SETTINGS_KEY, "settings")?;
    let last_ids = read_value(conn, LAST_IDS_KEY, "id counters")?.unwrap_or_default();

    let mut stmt = conn.prepare(
        "SELECT id, name, max_concurrent, speed_limit, schedule, is_default, paused,
                running_outside_schedule
         FROM queues ORDER BY position",
    )?;
    let queues = stmt
        .query_map([], read_queue)?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    let mut stmt = conn.prepare(
        "SELECT id, url, path, queue_id, position, category, status, total, done, error,
                added_at, completed_at, origin, settled, chosen_name, chosen_folder, media
         FROM downloads ORDER BY position",
    )?;
    let mut downloads = Vec::new();
    let mut newly_unreadable = 0;
    for row in stmt.query_map([], read_download)? {
        match row? {
            StoredRow::Readable {
                download,
                origin_damaged,
            } => {
                if origin_damaged {
                    // Rewritten at once, so the damaged text, which may hold cookies, is gone.
                    conn.execute(
                        "UPDATE downloads SET origin = ?2 WHERE id = ?1",
                        params![download.summary.id.0, to_json(&download.origin)],
                    )?;
                }
                downloads.push(*download);
            }
            StoredRow::Unreadable { id, origin } => {
                let (kept, first_seen) = without_sign_in(&origin);
                conn.execute(
                    "UPDATE downloads SET origin = ?2 WHERE id = ?1",
                    params![id.0, kept],
                )?;
                if first_seen {
                    newly_unreadable += 1;
                }
            }
        }
    }

    Ok(Snapshot {
        settings,
        last_ids,
        queues,
        downloads,
        recovery: (newly_unreadable > 0).then_some(Recovery::UnreadableKept(newly_unreadable)),
    })
}

/// The stored origin of a row this build cannot read, without its sign-in, and whether this is the first start that found the row. Such a row is kept for a version that can read it (after going back to an older version, say), but nothing here can resume it or forget its sign-in after a week (D17), so it must not keep one. It is marked as having forgotten its sign-in, which also tells a later start it was already counted.
fn without_sign_in(origin: &str) -> (String, bool) {
    let mut value = match serde_json::from_str::<serde_json::Value>(origin) {
        Ok(value @ serde_json::Value::Object(_)) => value,
        _ => return (to_json(&forgotten_origin()), true),
    };
    let first_seen = value.get("forgot_sign_in") != Some(&serde_json::Value::Bool(true));
    let headers = value
        .get("headers")
        .and_then(|h| serde_json::from_value::<Vec<Header>>(h.clone()).ok())
        .map(|h| crate::headers::without_sign_in(&h).unwrap_or(h))
        .unwrap_or_default();
    value["headers"] = serde_json::to_value(headers).unwrap_or_default();
    value["forgot_sign_in"] = serde_json::Value::Bool(true);
    (value.to_string(), first_seen)
}

/// An origin that holds nothing, for a row whose stored origin was damaged.
fn forgotten_origin() -> Origin {
    Origin {
        forgot_sign_in: true,
        ..Origin::default()
    }
}

fn save_queues(tx: &Transaction<'_>, queues: &[Queue]) -> rusqlite::Result<()> {
    tx.execute("DELETE FROM queues", [])?;
    let mut stmt = tx.prepare_cached(
        "INSERT INTO queues (id, position, name, max_concurrent, speed_limit, schedule, is_default,
                             paused, running_outside_schedule)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
    )?;
    for (position, q) in (0i64..).zip(queues) {
        stmt.execute(params![
            q.id.0,
            position,
            q.name,
            q.max_concurrent,
            q.speed_limit.map(to_i64),
            q.schedule.as_ref().map(to_json),
            q.is_default,
            q.paused,
            q.running_outside_schedule,
        ])?;
    }
    Ok(())
}

fn read_queue(row: &Row<'_>) -> rusqlite::Result<Queue> {
    Ok(Queue {
        id: QueueId(row.get(0)?),
        name: row.get(1)?,
        max_concurrent: row.get(2)?,
        speed_limit: row.get::<_, Option<i64>>(3)?.map(to_u64),
        schedule: row
            .get::<_, Option<String>>(4)?
            .and_then(|json| from_json(&json, "queue schedule")),
        is_default: row.get(5)?,
        paused: row.get(6)?,
        running_outside_schedule: row.get(7)?,
    })
}

fn save_download(tx: &Transaction<'_>, row: &StoredDownload) -> rusqlite::Result<()> {
    let d = &row.summary;
    tx.prepare_cached(
        "INSERT OR REPLACE INTO downloads
            (id, url, path, queue_id, position, category, status, total, done, error,
             added_at, completed_at, origin, settled, chosen_name, chosen_folder, media)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)",
    )?
    .execute(params![
        d.id.0,
        d.url,
        d.path,
        d.queue.0,
        row.position,
        enum_text(&d.category),
        enum_text(&d.status),
        d.total.map(to_i64),
        to_i64(d.done),
        d.error.as_ref().map(to_json),
        d.added_at,
        d.completed_at,
        to_json(&row.origin),
        row.naming.settled,
        row.naming.name,
        row.naming.folder,
        row.media.as_ref().map(to_json),
    ])?;
    Ok(())
}

/// One stored download row, or a row that cannot be understood.
enum StoredRow {
    Readable {
        download: Box<StoredDownload>,
        /// Its stored origin could not be read and was replaced by [`forgotten_origin`].
        origin_damaged: bool,
    },
    Unreadable {
        id: DownloadId,
        /// The stored origin, as it is.
        origin: String,
    },
}

/// A row that cannot be understood is reported rather than failing the whole start. A damaged origin costs only the sign-in it held.
fn read_download(row: &Row<'_>) -> rusqlite::Result<StoredRow> {
    let id = DownloadId(row.get(0)?);
    let path: String = row.get(2)?;
    let category = from_text(&row.get::<_, String>(5)?);
    let status = from_text(&row.get::<_, String>(6)?);
    let stored_origin: String = row.get(12)?;
    let (Some(category), Some(status)) = (category, status) else {
        warn!(id = id.0, "a stored download cannot be read");
        return Ok(StoredRow::Unreadable {
            id,
            origin: stored_origin,
        });
    };
    let parsed: Option<Origin> = from_json(&stored_origin, "download origin");
    let origin_damaged = parsed.is_none();
    let origin = parsed.unwrap_or_else(forgotten_origin);
    let mut status = status;
    let mut error = row
        .get::<_, Option<String>>(9)?
        .and_then(|json| from_json(&json, "download error"));
    let media: Option<StoredMedia> = match row.get::<_, Option<String>>(16)? {
        None => None,
        Some(json) => match from_json(&json, "download media") {
            Some(media) => Some(media),
            // Media whose details are damaged must never run as a plain
            // download of its link (a page or a playlist).
            None => {
                if !matches!(
                    status,
                    DownloadStatus::Completed | DownloadStatus::Cancelled
                ) {
                    status = DownloadStatus::Failed;
                    error = Some(crate::media::unreadable());
                }
                Some(StoredMedia {
                    summary: MediaSummary {
                        quality: "Unknown".into(),
                        source: MediaKind::Page,
                        video_key: None,
                    },
                    job: Some(crate::media::MediaJob::Unreadable),
                })
            }
        },
    };
    let file_name = PathBuf::from(&path)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    Ok(StoredRow::Readable {
        origin_damaged,
        download: Box::new(StoredDownload {
            summary: DownloadSummary {
                id,
                url: row.get(1)?,
                file_name,
                path,
                queue: QueueId(row.get(3)?),
                category,
                status,
                total: row.get::<_, Option<i64>>(7)?.map(to_u64),
                done: to_u64(row.get(8)?),
                speed: 0,
                connections: 0,
                eta_secs: None,
                error,
                added_at: row.get(10)?,
                completed_at: row.get(11)?,
                waiting: None,
                media: media.as_ref().map(|m| m.summary.clone()),
                processing: None,
            },
            position: row.get(4)?,
            origin,
            naming: NameChoice {
                settled: row.get(13)?,
                name: row.get(14)?,
                folder: row.get(15)?,
            },
            media,
        }),
    })
}

/// Unit-like enums are stored as their snake_case serde name.
fn enum_text<T: Serialize>(value: &T) -> String {
    match serde_json::to_value(value) {
        Ok(serde_json::Value::String(s)) => s,
        other => {
            error!(?other, "enum did not serialize to a string");
            String::new()
        }
    }
}

fn from_text<T: DeserializeOwned>(text: &str) -> Option<T> {
    serde_json::from_value(serde_json::Value::String(text.to_string())).ok()
}

fn to_json<T: Serialize>(value: &T) -> String {
    // Serializing these plain data types cannot fail.
    serde_json::to_string(value).unwrap_or_default()
}

fn from_json<T: DeserializeOwned>(json: &str, what: &str) -> Option<T> {
    serde_json::from_str(json)
        .inspect_err(|e| warn!(error = %e, "stored {what} is unreadable"))
        .ok()
}

/// SQLite integers are signed; byte counts never get near the limit.
fn to_i64(value: u64) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

fn to_u64(value: i64) -> u64 {
    u64::try_from(value).unwrap_or(0)
}

/// The database holds cookies, so only its owner may read it (D17). A failure is
/// logged rather than fatal: the app still works, with the folder's default access.
fn restrict_to_owner(path: &Path) {
    if let Err(e) = owner_only::restrict(path) {
        warn!(path = %path.display(), error = %e, "could not restrict access to the current user");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wirecatch_protocol::app::{
        Category, DownloadStatus, FixAction, Schedule, TimeOfDay, UserError,
    };

    fn download(id: i64) -> StoredDownload {
        StoredDownload {
            summary: DownloadSummary {
                id: DownloadId(id),
                url: "https://example.com/a.zip".into(),
                file_name: "a.zip".into(),
                path: std::env::temp_dir()
                    .join("a.zip")
                    .to_string_lossy()
                    .into_owned(),
                queue: QueueId(1),
                category: Category::Archive,
                status: DownloadStatus::Failed,
                total: Some(100),
                done: 40,
                speed: 0,
                connections: 0,
                eta_secs: None,
                error: Some(UserError {
                    message: "expired".into(),
                    fix: Some(FixAction::RefreshLink),
                }),
                added_at: 1,
                completed_at: None,
                waiting: None,
                media: None,
                processing: None,
            },
            position: 3,
            origin: Origin {
                user_agent: Some("UA".into()),
                headers: vec![Header {
                    name: "Cookie".into(),
                    value: "a=b".into(),
                }],
                forgot_sign_in: false,
            },
            naming: NameChoice {
                settled: false,
                name: Some("chosen.zip".into()),
                folder: None,
            },
            media: None,
        }
    }

    fn ignore_failures() -> FailureHandler {
        Box::new(|_| {})
    }

    #[tokio::test]
    async fn a_write_that_keeps_failing_is_reported() {
        let dir = tempfile::tempdir().unwrap();
        let reports = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink = reports.clone();
        let (store, _) = Store::open(
            dir.path(),
            Box::new(move |reason| sink.lock().unwrap().push(reason)),
        )
        .unwrap();
        store.save_downloads(vec![download(1)]);
        store.flush().await;
        assert!(reports.lock().unwrap().is_empty());

        // Something outside the app breaks the database.
        let other = Connection::open(dir.path().join(DATABASE_FILE)).unwrap();
        other.execute("DROP TABLE downloads", []).unwrap();
        store.save_downloads(vec![download(2)]);
        store.flush().await;
        let reports = reports.lock().unwrap();
        assert_eq!(reports.len(), 1, "{reports:?}");
        assert!(reports[0].contains("downloads"), "{reports:?}");
    }

    fn stored_ids(conn: &Connection) -> Vec<i64> {
        let mut stmt = conn
            .prepare("SELECT id FROM downloads ORDER BY id")
            .unwrap();
        stmt.query_map([], |row| row.get(0))
            .unwrap()
            .collect::<rusqlite::Result<Vec<i64>>>()
            .unwrap()
    }

    /// One write that keeps failing must not take the others in its batch down
    /// with it, and is itself kept and tried again later rather than dropped: a
    /// lost delete would bring back a removed download, cookies and all.
    #[tokio::test]
    async fn a_failing_write_is_kept_and_the_others_are_saved() {
        let dir = tempfile::tempdir().unwrap();
        let reports = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink = reports.clone();
        let (store, _) = Store::open(
            dir.path(),
            Box::new(move |reason| sink.lock().unwrap().push(reason)),
        )
        .unwrap();
        store.save_downloads(vec![download(3)]);
        store.flush().await;

        // Something outside the app makes saving download 2 fail.
        let other = Connection::open(dir.path().join(DATABASE_FILE)).unwrap();
        other
            .execute_batch(
                "CREATE TRIGGER refuse_two BEFORE INSERT ON downloads WHEN NEW.id = 2
                 BEGIN SELECT RAISE(ABORT, 'injected failure'); END;",
            )
            .unwrap();
        store.save_downloads(vec![download(1), download(2)]);
        store.delete_downloads(vec![DownloadId(3)]);
        store.delete_downloads(vec![DownloadId(2)]);
        store.save_downloads(vec![download(2)]);
        store.flush().await;
        assert_eq!(stored_ids(&other), vec![1], "the other writes were lost");
        assert!(
            reports.lock().unwrap()[0].contains("injected failure"),
            "{reports:?}"
        );

        // Once the problem is gone, the kept writes land, in their order.
        other.execute_batch("DROP TRIGGER refuse_two").unwrap();
        store.save_last_ids(LastIds::default());
        store.flush().await;
        assert_eq!(stored_ids(&other), vec![1, 2]);
    }

    #[tokio::test]
    async fn a_media_download_keeps_what_it_downloads() {
        let dir = tempfile::tempdir().unwrap();
        let (store, _) = Store::open(dir.path(), ignore_failures()).unwrap();
        let mut row = download(1);
        let media = StoredMedia {
            summary: wirecatch_protocol::app::MediaSummary {
                quality: "1080p".into(),
                source: wirecatch_protocol::extension::MediaKind::Page,
                video_key: None,
            },
            job: Some(crate::media::MediaJob::Page {
                format: "137+140".into(),
                video: Some("137".into()),
                audio: Some("140".into()),
                output: None,
                keep_ext: None,
            }),
        };
        row.summary.media = Some(media.summary.clone());
        row.media = Some(media);
        store.save_downloads(vec![row.clone()]);
        store.flush().await;
        drop(store);
        let (_store, snapshot) = Store::open(dir.path(), ignore_failures()).unwrap();
        assert_eq!(snapshot.downloads, vec![row]);
    }

    /// A media download whose stored details are damaged must not run as a
    /// plain download of its page.
    #[tokio::test]
    async fn damaged_media_details_become_a_failed_download() {
        let dir = tempfile::tempdir().unwrap();
        let (store, _) = Store::open(dir.path(), ignore_failures()).unwrap();
        let mut row = download(1);
        row.summary.status = DownloadStatus::Queued;
        row.summary.error = None;
        row.media = Some(StoredMedia {
            summary: MediaSummary {
                quality: "1080p".into(),
                source: MediaKind::Page,
                video_key: None,
            },
            job: None,
        });
        store.save_downloads(vec![row]);
        store.flush().await;
        drop(store);
        let conn = Connection::open(dir.path().join(DATABASE_FILE)).unwrap();
        conn.execute("UPDATE downloads SET media = '{\"broken'", [])
            .unwrap();
        drop(conn);
        let (_store, snapshot) = Store::open(dir.path(), ignore_failures()).unwrap();
        let loaded = &snapshot.downloads[0];
        assert_eq!(loaded.summary.status, DownloadStatus::Failed);
        assert_eq!(loaded.summary.error, Some(crate::media::unreadable()));
        assert_eq!(
            loaded.media.as_ref().and_then(|m| m.job.clone()),
            Some(crate::media::MediaJob::Unreadable)
        );
    }

    #[tokio::test]
    async fn everything_written_is_read_back() {
        let dir = tempfile::tempdir().unwrap();
        let (store, snapshot) = Store::open(dir.path(), ignore_failures()).unwrap();
        assert!(snapshot.settings.is_none() && snapshot.queues.is_empty());

        let queue = Queue {
            id: QueueId(4),
            name: "Night".into(),
            max_concurrent: 2,
            speed_limit: Some(1000),
            schedule: Some(Schedule {
                start: TimeOfDay {
                    hour: 1,
                    minute: 30,
                },
                stop: None,
                days: vec![],
            }),
            is_default: false,
            paused: false,
            running_outside_schedule: false,
        };
        let settings = crate::settings::default_settings();
        store.save_queues(vec![queue.clone()]);
        store.save_settings(settings.clone());
        store.save_downloads(vec![download(1), download(2)]);
        store.delete_downloads(vec![DownloadId(2)]);
        store.flush().await;
        drop(store);

        let (_store, snapshot) = Store::open(dir.path(), ignore_failures()).unwrap();
        assert_eq!(snapshot.settings, Some(settings));
        assert_eq!(snapshot.queues, vec![queue]);
        assert_eq!(snapshot.downloads, vec![download(1)]);
    }

    /// Files in `dir` whose names start with `prefix`.
    fn files_starting(dir: &Path, prefix: &str) -> Vec<PathBuf> {
        std::fs::read_dir(dir)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| {
                path.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with(prefix)
            })
            .collect()
    }

    fn assert_set_aside(dir: &Path, snapshot: &Snapshot, original: &[u8]) {
        let Some(Recovery::Damaged { kept_at }) = &snapshot.recovery else {
            panic!(
                "expected a damaged list to be set aside, got {:?}",
                snapshot.recovery
            );
        };
        assert_eq!(
            std::fs::read(kept_at).unwrap(),
            original,
            "the damaged file is kept as it was"
        );
        assert!(snapshot.downloads.is_empty() && snapshot.queues.is_empty());
        let damaged = format!("{DATABASE_FILE}{DAMAGED_SUFFIX}");
        assert_eq!(files_starting(dir, &damaged).len(), 1);
    }

    // A damaged file used to stop the app from opening on every start, with advice to restart that could never help.
    #[test]
    fn a_file_that_is_not_a_database_is_set_aside() {
        let dir = tempfile::tempdir().unwrap();
        let garbage =
            b"this is not a database, just some bytes a sync tool left behind".repeat(100);
        std::fs::write(dir.path().join(DATABASE_FILE), &garbage).unwrap();

        let (store, snapshot) = Store::open(dir.path(), ignore_failures()).unwrap();

        assert_set_aside(dir.path(), &snapshot, &garbage);
        store.save_downloads(vec![download(1)]);
        drop(store);
        let (_, reopened) = Store::open(dir.path(), ignore_failures()).unwrap();
        assert_eq!(reopened.recovery, None, "the new list opens normally");
    }

    #[test]
    fn a_truncated_database_is_set_aside() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(DATABASE_FILE);
        {
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch("CREATE TABLE filler (data BLOB);")
                .unwrap();
            for _ in 0..64 {
                conn.execute("INSERT INTO filler VALUES (zeroblob(4096))", [])
                    .unwrap();
            }
        }
        let whole = std::fs::read(&path).unwrap();
        let cut = &whole[..whole.len() / 2];
        std::fs::write(&path, cut).unwrap();

        let (_, snapshot) = Store::open(dir.path(), ignore_failures()).unwrap();

        assert_set_aside(dir.path(), &snapshot, cut);
    }

    // The kept journal was renamed to `wirecatch.db-wal.damaged-<t>`, a name SQLite never pairs with
    // `wirecatch.db.damaged-<t>`, and closing the connection that found the damage folded the journal
    // into the damaged file first: whatever was still in it was lost to anyone recovering the list.
    #[test]
    fn a_damaged_list_keeps_its_journal_under_the_name_sqlite_pairs() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(DATABASE_FILE);
        // A list in WAL mode: older pages in the file, the newest still in its journal.
        let source = tempfile::tempdir().unwrap();
        let source_path = source.path().join(DATABASE_FILE);
        let conn = Connection::open(&source_path).unwrap();
        conn.pragma_update(None, "journal_mode", "WAL").unwrap();
        conn.pragma_update(None, "wal_autocheckpoint", 0).unwrap();
        conn.execute_batch("CREATE TABLE filler (data BLOB); CREATE TABLE recent (n INTEGER);")
            .unwrap();
        for _ in 0..16 {
            conn.execute("INSERT INTO filler VALUES (zeroblob(4096))", [])
                .unwrap();
        }
        conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()))
            .unwrap();
        conn.execute("INSERT INTO recent VALUES (1)", []).unwrap();
        let journal = std::fs::read(with_suffix(&source_path, "-wal")).unwrap();
        assert!(!journal.is_empty());
        // Damage to pages in the file itself, past its header.
        let mut damaged = std::fs::read(&source_path).unwrap();
        let page = 4096;
        for start in (2 * page..damaged.len()).step_by(page) {
            damaged[start..start + 16].fill(0xFF);
        }
        std::fs::write(&path, &damaged).unwrap();
        std::fs::write(with_suffix(&path, "-wal"), &journal).unwrap();
        drop(conn);

        let (_, snapshot) = Store::open(dir.path(), ignore_failures()).unwrap();

        let Some(Recovery::Damaged { kept_at }) = snapshot.recovery else {
            panic!(
                "expected the list to be set aside, got {:?}",
                snapshot.recovery
            );
        };
        let kept = std::fs::read(with_suffix(&kept_at, "-wal")).unwrap();
        assert!(
            kept == journal,
            "the journal is kept as it was, next to the list"
        );
    }

    // When renaming the journal failed after the list itself had moved, the start failed, and the
    // list and its journal were left apart.
    #[cfg(windows)]
    #[test]
    fn a_damaged_list_that_cannot_move_whole_stays_where_it_is() {
        use std::os::windows::fs::OpenOptionsExt;

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(DATABASE_FILE);
        let garbage = b"not a database".repeat(100);
        std::fs::write(&path, &garbage).unwrap();
        let journal = with_suffix(&path, "-wal");
        std::fs::write(&journal, b"journal").unwrap();
        // Held open without sharing, like a sync tool can, so it cannot be renamed.
        let held = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(&journal)
            .unwrap();

        assert!(Store::open(dir.path(), ignore_failures()).is_err());

        drop(held);
        assert_eq!(
            std::fs::read(&path).unwrap(),
            garbage,
            "the list moved back"
        );
        assert!(journal.exists());
    }

    // An upgrade that fails part way leaves the list as it was, in place: it is not damage.
    #[test]
    fn a_failed_upgrade_leaves_the_list_as_it_was() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(DATABASE_FILE);
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch(MIGRATIONS[0]).unwrap();
        // The second step adds this column, so it fails half way.
        conn.execute_batch("ALTER TABLE queues ADD COLUMN paused INTEGER")
            .unwrap();
        conn.pragma_update(None, "user_version", 1).unwrap();
        drop(conn);

        assert!(Store::open(dir.path(), ignore_failures()).is_err());

        let conn = Connection::open(&path).unwrap();
        let version: u32 = conn
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(version, 1);
        let columns: i64 = conn
            .query_row(
                "SELECT count(*) FROM pragma_table_info('queues') WHERE name = 'running_outside_schedule'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(columns, 0, "the failed step changed nothing");
    }

    // The copy before an upgrade was required: when it could not be made (no room for a second
    // list on a nearly full disk), the app did not start.
    #[test]
    fn an_upgrade_goes_ahead_when_no_copy_can_be_made() {
        let dir = tempfile::tempdir().unwrap();
        let conn = Connection::open(dir.path().join(DATABASE_FILE)).unwrap();
        conn.execute_batch(MIGRATIONS[0]).unwrap();
        conn.pragma_update(None, "user_version", 1).unwrap();
        drop(conn);
        // A folder where the copy goes: neither removing nor writing it can work.
        let backup = dir.path().join(format!("{DATABASE_FILE}{BACKUP_SUFFIX}1"));
        std::fs::create_dir(&backup).unwrap();
        std::fs::write(backup.join("keep"), b"").unwrap();

        Store::open(dir.path(), ignore_failures()).unwrap();

        let conn = Connection::open(dir.path().join(DATABASE_FILE)).unwrap();
        let version: u32 = conn
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(version as usize, MIGRATIONS.len());
    }

    // Copies set aside or taken before an upgrade hold every download's cookies and were kept for
    // good.
    #[test]
    fn old_copies_of_the_list_are_removed() {
        let dir = tempfile::tempdir().unwrap();
        let old = std::time::SystemTime::now() - KEEP_COPIES_FOR - Duration::from_secs(60);
        let copy = |suffix: &str, modified: Option<std::time::SystemTime>| {
            let path = dir.path().join(format!("{DATABASE_FILE}{suffix}"));
            let file = std::fs::File::create(&path).unwrap();
            if let Some(modified) = modified {
                file.set_modified(modified).unwrap();
            }
            path
        };
        let old_damaged = copy(&format!("{DAMAGED_SUFFIX}1"), Some(old));
        let old_journal = copy(&format!("{DAMAGED_SUFFIX}1-wal"), Some(old));
        let old_backup = copy(&format!("{BACKUP_SUFFIX}1"), Some(old));
        let recent = copy(&format!("{DAMAGED_SUFFIX}2"), None);
        let unrelated = dir.path().join("notes.txt");
        std::fs::File::create(&unrelated)
            .unwrap()
            .set_modified(old)
            .unwrap();

        Store::open(dir.path(), ignore_failures()).unwrap();

        for gone in [old_damaged, old_journal, old_backup] {
            assert!(!gone.exists(), "{}", gone.display());
        }
        assert!(recent.exists());
        assert!(unrelated.exists());
    }

    #[test]
    fn a_database_is_copied_before_its_schema_is_upgraded() {
        let dir = tempfile::tempdir().unwrap();
        let conn = Connection::open(dir.path().join(DATABASE_FILE)).unwrap();
        conn.execute_batch(MIGRATIONS[0]).unwrap();
        conn.pragma_update(None, "user_version", 1).unwrap();
        insert_old_download(&conn);
        drop(conn);

        Store::open(dir.path(), ignore_failures()).unwrap();

        let backup = dir.path().join(format!("{DATABASE_FILE}{BACKUP_SUFFIX}1"));
        let copy = Connection::open(&backup).unwrap();
        let version: u32 = copy
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        let rows: i64 = copy
            .query_row("SELECT count(*) FROM downloads", [], |row| row.get(0))
            .unwrap();
        assert_eq!(
            (version, rows),
            (1, 1),
            "the copy is the list as it was before the upgrade"
        );
    }

    // Rows this build could not read were deleted for good, including rows a newer version wrote
    // with a status this one does not know, which going back to an older version then lost.
    #[tokio::test]
    async fn unreadable_downloads_are_kept_without_their_sign_in() {
        let dir = tempfile::tempdir().unwrap();
        let (store, _) = Store::open(dir.path(), ignore_failures()).unwrap();
        let mut signed_in = download(2);
        signed_in.origin.headers = vec![
            Header {
                name: "Cookie".into(),
                value: "session=secret".into(),
            },
            Header {
                name: "Referer".into(),
                value: "https://example.com/".into(),
            },
        ];
        store.save_downloads(vec![download(1), signed_in]);
        store.flush().await;
        drop(store);
        let conn = Connection::open(dir.path().join(DATABASE_FILE)).unwrap();
        conn.execute(
            "UPDATE downloads SET status = 'from_a_newer_version' WHERE id = 2",
            [],
        )
        .unwrap();
        drop(conn);

        let (_, snapshot) = Store::open(dir.path(), ignore_failures()).unwrap();
        assert_eq!(snapshot.recovery, Some(Recovery::UnreadableKept(1)));
        assert_eq!(snapshot.downloads, vec![download(1)]);
        let conn = Connection::open(dir.path().join(DATABASE_FILE)).unwrap();
        let origin: String = conn
            .query_row("SELECT origin FROM downloads WHERE id = 2", [], |row| {
                row.get(0)
            })
            .unwrap();
        drop(conn);
        assert!(!origin.contains("secret"), "{origin}");
        assert!(origin.contains("Referer"), "{origin}");

        let (_, again) = Store::open(dir.path(), ignore_failures()).unwrap();
        assert_eq!(again.recovery, None, "told once");
        assert_eq!(again.downloads, vec![download(1)]);
    }

    #[tokio::test]
    async fn a_damaged_origin_costs_only_the_sign_in() {
        let dir = tempfile::tempdir().unwrap();
        let (store, _) = Store::open(dir.path(), ignore_failures()).unwrap();
        store.save_downloads(vec![download(1), download(2)]);
        store.flush().await;
        drop(store);
        let conn = Connection::open(dir.path().join(DATABASE_FILE)).unwrap();
        conn.execute(
            "UPDATE downloads SET origin = 'Cookie: session=secret' WHERE id = 2",
            [],
        )
        .unwrap();
        drop(conn);

        let (_, snapshot) = Store::open(dir.path(), ignore_failures()).unwrap();
        assert_eq!(snapshot.recovery, None);
        let mut recovered = download(2);
        recovered.origin = forgotten_origin();
        assert_eq!(snapshot.downloads, vec![download(1), recovered]);
        let conn = Connection::open(dir.path().join(DATABASE_FILE)).unwrap();
        let origin: String = conn
            .query_row("SELECT origin FROM downloads WHERE id = 2", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert!(!origin.contains("secret"), "{origin}");
    }

    #[test]
    fn a_newer_database_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let conn = Connection::open(dir.path().join(DATABASE_FILE)).unwrap();
        conn.pragma_update(None, "user_version", 99).unwrap();
        drop(conn);
        let err = Store::open(dir.path(), ignore_failures()).unwrap_err();
        assert!(matches!(err, CoreError::DatabaseTooNew { found: 99, .. }));
        assert!(err.to_string().contains("Update Wirecatch"));
    }

    #[test]
    fn a_database_from_the_first_version_is_upgraded_in_place() {
        let dir = tempfile::tempdir().unwrap();
        let conn = Connection::open(dir.path().join(DATABASE_FILE)).unwrap();
        conn.execute_batch(MIGRATIONS[0]).unwrap();
        conn.pragma_update(None, "user_version", 1).unwrap();
        conn.execute(
            "INSERT INTO queues (id, position, name, max_concurrent, is_default)
             VALUES (1, 0, 'Main', 3, 1)",
            [],
        )
        .unwrap();
        insert_old_download(&conn);
        drop(conn);

        let (_, snapshot) = Store::open(dir.path(), ignore_failures()).unwrap();
        let main = &snapshot.queues[0];
        assert_eq!((main.name.as_str(), main.max_concurrent), ("Main", 3));
        assert!(!main.paused && !main.running_outside_schedule);
        assert_old_download_upgraded(&snapshot);
    }

    #[test]
    fn a_database_from_the_second_version_is_upgraded_in_place() {
        let dir = tempfile::tempdir().unwrap();
        let conn = Connection::open(dir.path().join(DATABASE_FILE)).unwrap();
        conn.execute_batch(MIGRATIONS[0]).unwrap();
        conn.execute_batch(MIGRATIONS[1]).unwrap();
        conn.pragma_update(None, "user_version", 2).unwrap();
        conn.execute(
            "INSERT INTO queues (id, position, name, max_concurrent, is_default, paused,
                                 running_outside_schedule)
             VALUES (1, 0, 'Main', 3, 1, 1, 0)",
            [],
        )
        .unwrap();
        insert_old_download(&conn);
        drop(conn);

        let (_, snapshot) = Store::open(dir.path(), ignore_failures()).unwrap();
        assert!(snapshot.queues[0].paused, "the queue's pause was lost");
        assert_old_download_upgraded(&snapshot);
        let conn = Connection::open(dir.path().join(DATABASE_FILE)).unwrap();
        let version: u32 = conn
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .unwrap();
        assert_eq!(version, schema_version());
    }

    /// A download as versions before 3 stored it, whose path was always final.
    fn insert_old_download(conn: &Connection) {
        conn.execute(
            "INSERT INTO downloads (id, url, path, queue_id, position, category, status,
                                    total, done, error, added_at, completed_at, origin)
             VALUES (7, 'https://example.com/a.zip', ?1, 1, 4, 'archive', 'paused',
                     100, 40, NULL, 1, NULL, '{\"user_agent\":null,\"headers\":[]}')",
            [download(7).summary.path],
        )
        .unwrap();
    }

    fn assert_old_download_upgraded(snapshot: &Snapshot) {
        let [old] = snapshot.downloads.as_slice() else {
            panic!("expected one download, got {:?}", snapshot.downloads);
        };
        assert_eq!(old.summary.id, DownloadId(7));
        assert_eq!(old.summary.status, DownloadStatus::Paused);
        assert_eq!((old.summary.done, old.summary.total), (40, Some(100)));
        assert_eq!(old.summary.file_name, "a.zip");
        // Its path was chosen before paths waited for the server: it is final.
        assert_eq!(old.naming, NameChoice::settled());
    }

    /// Stored settings as a database at `version` holds them, with `ask` for the window setting.
    fn database_with_settings(dir: &Path, version: usize, ask: bool) {
        let conn = Connection::open(dir.join(DATABASE_FILE)).unwrap();
        for sql in &MIGRATIONS[..version] {
            conn.execute_batch(sql).unwrap();
        }
        conn.pragma_update(None, "user_version", u32::try_from(version).unwrap())
            .unwrap();
        let mut settings = crate::settings::default_settings();
        settings.extension.ask_before_download = ask;
        settings.notify_on_complete = false;
        conn.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2)",
            params![SETTINGS_KEY, to_json(&settings)],
        )
        .unwrap();
    }

    #[tokio::test]
    async fn the_window_before_each_download_is_switched_on_once() {
        let dir = tempfile::tempdir().unwrap();
        // Saved by a version from before D23, with the old default.
        database_with_settings(dir.path(), 4, false);
        let (store, snapshot) = Store::open(dir.path(), ignore_failures()).unwrap();
        let settings = snapshot.settings.expect("the settings survive");
        assert!(settings.extension.ask_before_download, "switched on");
        assert!(!settings.notify_on_complete, "nothing else changes");
        assert!(
            settings.show_progress_windows,
            "a new setting takes its default"
        );

        // Turned off afterwards: that choice is kept from then on.
        let mut chosen = settings;
        chosen.extension.ask_before_download = false;
        store.save_settings(chosen);
        store.flush().await;
        drop(store);
        let (_, snapshot) = Store::open(dir.path(), ignore_failures()).unwrap();
        assert!(!snapshot.settings.unwrap().extension.ask_before_download);
    }

    #[test]
    fn a_fresh_database_has_no_settings_for_the_migration_to_change() {
        let dir = tempfile::tempdir().unwrap();
        let (_, snapshot) = Store::open(dir.path(), ignore_failures()).unwrap();
        assert!(snapshot.settings.is_none());
        assert!(
            crate::settings::default_settings()
                .extension
                .ask_before_download
        );
    }

    #[test]
    fn deleted_rows_are_overwritten() {
        let conn = Connection::open_in_memory().unwrap();
        configure(&conn).unwrap();
        let on: i64 = conn
            .pragma_query_value(None, "secure_delete", |r| r.get(0))
            .unwrap();
        assert_eq!(on, 1);
    }

    #[test]
    fn migrations_run_once() {
        let dir = tempfile::tempdir().unwrap();
        drop(Store::open(dir.path(), ignore_failures()).unwrap());
        drop(Store::open(dir.path(), ignore_failures()).unwrap());
        let conn = Connection::open(dir.path().join(DATABASE_FILE)).unwrap();
        let version: u32 = conn
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .unwrap();
        assert_eq!(version, schema_version());
    }

    #[tokio::test]
    async fn unreadable_rows_are_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let (store, _) = Store::open(dir.path(), ignore_failures()).unwrap();
        store.save_downloads(vec![download(1), download(2)]);
        store.flush().await;
        let conn = Connection::open(dir.path().join(DATABASE_FILE)).unwrap();
        conn.execute("UPDATE downloads SET status = 'exploded' WHERE id = 2", [])
            .unwrap();
        let (_, snapshot) = Store::open(dir.path(), ignore_failures()).unwrap();
        assert_eq!(snapshot.downloads, vec![download(1)]);
    }
}

//! SQLite persistence (D17). One connection lives on a dedicated thread; async code
//! hands it writes through a channel and never waits on the disk.

use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

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
}

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
    pub fn open(dir: &Path, on_failure: FailureHandler) -> Result<(Store, Snapshot)> {
        std::fs::create_dir_all(dir).map_err(|source| CoreError::File {
            path: dir.display().to_string(),
            source,
        })?;
        // The folder first, so the database and its journal files inherit its access.
        restrict_to_owner(dir);
        let path = dir.join(DATABASE_FILE);
        let mut conn = Connection::open(&path)?;
        restrict_to_owner(&path);
        conn.busy_timeout(BUSY_TIMEOUT)?;
        configure(&conn)?;
        migrate(&mut conn)?;
        let snapshot = load(&conn)?;

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

fn migrate(conn: &mut Connection) -> Result<()> {
    let supported = MIGRATIONS.len() as u32;
    let found: u32 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if found > supported {
        return Err(CoreError::DatabaseTooNew { found, supported });
    }
    for (index, sql) in MIGRATIONS.iter().enumerate().skip(found as usize) {
        let tx = conn.transaction()?;
        tx.execute_batch(sql)?;
        tx.pragma_update(None, "user_version", index as u32 + 1)?;
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
    let downloads = stmt
        .query_map([], read_download)?
        .filter_map(|row| match row {
            Ok(Some(download)) => Some(Ok(download)),
            Ok(None) => None,
            Err(e) => Some(Err(e)),
        })
        .collect::<rusqlite::Result<Vec<_>>>()?;

    Ok(Snapshot {
        settings,
        last_ids,
        queues,
        downloads,
    })
}

fn save_queues(tx: &Transaction<'_>, queues: &[Queue]) -> rusqlite::Result<()> {
    tx.execute("DELETE FROM queues", [])?;
    let mut stmt = tx.prepare_cached(
        "INSERT INTO queues (id, position, name, max_concurrent, speed_limit, schedule, is_default,
                             paused, running_outside_schedule)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
    )?;
    for (position, q) in queues.iter().enumerate() {
        stmt.execute(params![
            q.id.0,
            position as i64,
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

/// A row that cannot be understood is skipped with a warning rather than failing
/// the whole start.
fn read_download(row: &Row<'_>) -> rusqlite::Result<Option<StoredDownload>> {
    let id = DownloadId(row.get(0)?);
    let path: String = row.get(2)?;
    let category = from_text(&row.get::<_, String>(5)?);
    let status = from_text(&row.get::<_, String>(6)?);
    let origin = from_json(&row.get::<_, String>(12)?, "download origin");
    let (Some(category), Some(status), Some(origin)) = (category, status, origin) else {
        warn!(id = id.0, "skipping a stored download that cannot be read");
        return Ok(None);
    };
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
    Ok(Some(StoredDownload {
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
        },
        position: row.get(4)?,
        origin,
        naming: NameChoice {
            settled: row.get(13)?,
            name: row.get(14)?,
            folder: row.get(15)?,
        },
        media,
    }))
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
            },
            job: Some(crate::media::MediaJob::Page {
                format: "137+140".into(),
                video: Some("137".into()),
                audio: Some("140".into()),
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
        assert_eq!(version, MIGRATIONS.len() as u32);
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
        assert_eq!(version, MIGRATIONS.len() as u32);
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

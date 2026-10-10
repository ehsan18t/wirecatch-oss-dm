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
use wirecatch_protocol::app::{AppSettings, DownloadSummary, Queue};
use wirecatch_protocol::extension::Header;
use wirecatch_protocol::{DownloadId, QueueId};

use crate::error::{CoreError, Result};
use crate::owner_only;

/// File name of the database inside the data folder.
pub const DATABASE_FILE: &str = "wirecatch.db";

/// Schema changes, applied in order. Entry `n` moves the schema to version `n + 1`.
/// Never edit a released entry; append a new one.
const MIGRATIONS: &[&str] = &[r#"
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
"#];

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
}

/// One download as stored.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct StoredDownload {
    pub summary: DownloadSummary,
    /// Order within its queue; lower runs first.
    pub position: i64,
    pub origin: Origin,
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

enum Op {
    SaveDownloads(Vec<StoredDownload>),
    DeleteDownloads(Vec<DownloadId>),
    SaveQueues(Vec<Queue>),
    SaveSettings(Box<AppSettings>),
    SaveLastIds(LastIds),
    Flush(oneshot::Sender<()>),
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
    pub async fn flush(&self) {
        let (done, wait) = oneshot::channel();
        self.send(Op::Flush(done));
        let _ = wait.await;
    }

    fn send(&self, op: Op) {
        if let Err(mpsc::SendError(op)) = self.ops.send(op) {
            error!(?op, "the database thread has stopped; change not saved");
        }
    }
}

/// Applies writes until every [`Store`] handle is gone. Writes that queued up while
/// the disk was busy are committed together in one transaction. A batch that fails
/// is retried after a short pause; if it still fails, `on_failure` is told why.
fn run(mut conn: Connection, ops: mpsc::Receiver<Op>, on_failure: FailureHandler) {
    while let Ok(first) = ops.recv() {
        let mut writes = Vec::new();
        let mut flushed = Vec::new();
        for op in std::iter::once(first).chain(ops.try_iter()) {
            match op {
                Op::Flush(done) => flushed.push(done),
                write => writes.push(write),
            }
        }
        if let Err(e) = apply_with_retries(&mut conn, &writes) {
            error!(error = %e, "could not save changes to the database");
            on_failure(e.to_string());
        }
        for done in flushed {
            let _ = done.send(());
        }
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
            Op::Flush(_) => {}
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
        "SELECT id, name, max_concurrent, speed_limit, schedule, is_default
         FROM queues ORDER BY position",
    )?;
    let queues = stmt
        .query_map([], read_queue)?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    let mut stmt = conn.prepare(
        "SELECT id, url, path, queue_id, position, category, status, total, done, error,
                added_at, completed_at, origin
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
        "INSERT INTO queues (id, position, name, max_concurrent, speed_limit, schedule, is_default)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
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
        paused: false,
        running_outside_schedule: false,
    })
}

fn save_download(tx: &Transaction<'_>, row: &StoredDownload) -> rusqlite::Result<()> {
    let d = &row.summary;
    tx.prepare_cached(
        "INSERT OR REPLACE INTO downloads
            (id, url, path, queue_id, position, category, status, total, done, error,
             added_at, completed_at, origin)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
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
            error: row
                .get::<_, Option<String>>(9)?
                .and_then(|json| from_json(&json, "download error")),
            added_at: row.get(10)?,
            completed_at: row.get(11)?,
            waiting: None,
        },
        position: row.get(4)?,
        origin,
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
            },
            position: 3,
            origin: Origin {
                user_agent: Some("UA".into()),
                headers: vec![Header {
                    name: "Cookie".into(),
                    value: "a=b".into(),
                }],
            },
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

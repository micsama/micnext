//! 开发者日志落盘：单线程写 SQLite，失败依次降级为文本文件、仅内存；只降不升。
//! 契约：docs/blueprints/developer-log-history.md。

use std::fmt::Write as _;
use std::fs::{self, File, OpenOptions};
use std::io::{ErrorKind, Write as _};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use chrono::{DateTime, Local, SecondsFormat};
use rusqlite::types::Type;
use rusqlite::{params, Connection, OpenFlags};
use serde::{Deserialize, Serialize};

use crate::limits::{
    LOG_DB_BYTES, LOG_FILE_BYTES, LOG_FLUSH_TIMEOUT, LOG_HISTORY_RECORDS, LOG_PERSIST_BATCH,
    LOG_PERSIST_QUEUE, LOG_TRIM_EVERY, LOG_WAL_BYTES,
};
use crate::logs::{LogLevel, LogRecord};

const DB_FILE: &str = "logs.db";
const TEXT_FILE: &str = "micnext.log";
const SCHEMA_VERSION: i64 = 1;

type Failure = Box<dyn std::error::Error + Send + Sync>;

/// 当前落盘去向，供页面提示。
#[derive(Clone, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub(crate) enum Storage {
    Sqlite {
        #[serde(skip)]
        path: PathBuf,
    },
    File {
        path: PathBuf,
        reason: String,
    },
    Memory {
        reason: String,
    },
}

/// 采集侧投递端；投递从不阻塞。
#[derive(Clone)]
pub(crate) struct Sink {
    tx: SyncSender<Command>,
    dropped: Arc<AtomicU64>,
}

enum Command {
    Record(Arc<LogRecord>),
    Flush(mpsc::Sender<()>),
}

impl Sink {
    /// 只收 INFO 及以上；队列满则计数丢弃。
    pub(crate) fn offer(&self, record: &Arc<LogRecord>) {
        if record.level < LogLevel::Info {
            return;
        }
        if self.tx.try_send(Command::Record(record.clone())).is_err() {
            self.dropped.fetch_add(1, Ordering::Relaxed);
        }
    }

    pub(crate) fn flush(&self) {
        let deadline = Instant::now() + LOG_FLUSH_TIMEOUT;
        let (ack, done) = mpsc::channel();
        let mut command = Command::Flush(ack);
        loop {
            match self.tx.try_send(command) {
                Ok(()) => break,
                Err(TrySendError::Full(back)) if Instant::now() < deadline => {
                    command = back;
                    thread::sleep(Duration::from_millis(10));
                }
                Err(_) => return,
            }
        }
        let _ = done.recv_timeout(deadline.saturating_duration_since(Instant::now()));
    }
}

/// 起落盘线程；打开存储在线程内进行。
pub(crate) fn spawn(dir: PathBuf, boot_id: u64, storage: Arc<Mutex<Storage>>) -> Sink {
    let (tx, rx) = mpsc::sync_channel(LOG_PERSIST_QUEUE);
    let dropped = Arc::new(AtomicU64::new(0));
    let writer = Writer {
        dir,
        boot_id,
        storage,
        backend: Backend::Memory,
        dropped: dropped.clone(),
    };
    thread::Builder::new()
        .name("developer-logs".to_owned())
        .spawn(move || writer.run(rx))
        .expect("无法启动日志落盘线程");
    Sink { tx, dropped }
}

struct Writer {
    dir: PathBuf,
    boot_id: u64,
    storage: Arc<Mutex<Storage>>,
    backend: Backend,
    dropped: Arc<AtomicU64>,
}

enum Backend {
    Sqlite(SqliteLog),
    File(TextLog),
    Memory,
}

impl Writer {
    fn run(mut self, rx: Receiver<Command>) {
        self.open();
        let mut batch = Vec::new();
        let mut acks = Vec::new();
        while let Ok(first) = rx.recv() {
            let mut next = Some(first);
            while let Some(command) = next {
                match command {
                    Command::Record(record) => batch.push(record),
                    Command::Flush(ack) => acks.push(ack),
                }
                next = if batch.len() < LOG_PERSIST_BATCH {
                    rx.try_recv().ok()
                } else {
                    None
                };
            }
            let dropped = self.dropped.swap(0, Ordering::Relaxed);
            if dropped > 0 {
                tracing::warn!(dropped, "developer log queue full, records dropped");
            }
            if !batch.is_empty() {
                self.write(&batch);
                batch.clear();
            }
            for ack in acks.drain(..) {
                let _ = ack.send(());
            }
        }
    }

    fn open(&mut self) {
        let path = self.dir.join(DB_FILE);
        let opened = fs::create_dir_all(&self.dir)
            .map_err(Failure::from)
            .and_then(|()| SqliteLog::open(&path));
        match opened {
            Ok(db) => {
                self.set(Storage::Sqlite { path });
                self.backend = Backend::Sqlite(db);
            }
            Err(e) => self.degrade_to_text(format!("日志库 {} 打不开：{e}", path.display())),
        }
    }

    /// 失败的批次交给降级后的去向重写。
    fn write(&mut self, batch: &[Arc<LogRecord>]) {
        loop {
            match &mut self.backend {
                Backend::Sqlite(db) => match db.write(self.boot_id, batch) {
                    Ok(()) => return,
                    Err(e) => self.degrade_to_text(format!("日志库写入失败：{e}")),
                },
                Backend::File(text) => match text.write(batch) {
                    Ok(()) => return,
                    Err(e) => {
                        let reason = format!("文本日志 {} 写入失败：{e}", text.path.display());
                        self.degrade_to_memory(reason);
                    }
                },
                Backend::Memory => return,
            }
        }
    }

    fn degrade_to_text(&mut self, reason: String) {
        tracing::error!(%reason, "developer log database unavailable, writing text file");
        let path = self.dir.join(TEXT_FILE);
        match TextLog::open(&path) {
            Ok(text) => {
                self.set(Storage::File {
                    path: path.clone(),
                    reason,
                });
                self.backend = Backend::File(text);
            }
            Err(e) => self.degrade_to_memory(format!(
                "{reason}；文本日志 {} 也写不了：{e}",
                path.display()
            )),
        }
    }

    fn degrade_to_memory(&mut self, reason: String) {
        tracing::error!(%reason, "developer log file unavailable, keeping memory only");
        self.set(Storage::Memory { reason });
        self.backend = Backend::Memory;
    }

    fn set(&self, storage: Storage) {
        *self.storage.lock().expect("日志存储状态锁中毒") = storage;
    }
}

struct SqliteLog {
    conn: Connection,
    batches: u32,
}

impl SqliteLog {
    /// 版本不符时删库重建：日志可丢。
    fn open(path: &Path) -> Result<Self, Failure> {
        let mut conn = Connection::open(path)?;
        let version: i64 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;
        if version != 0 && version != SCHEMA_VERSION {
            drop(conn);
            for suffix in ["", "-wal", "-shm"] {
                let mut file = path.as_os_str().to_owned();
                file.push(suffix);
                match fs::remove_file(&file) {
                    Err(e) if e.kind() != ErrorKind::NotFound => return Err(e.into()),
                    _ => {}
                }
            }
            tracing::warn!(
                found = version,
                expected = SCHEMA_VERSION,
                "developer log database version mismatch, recreated"
            );
            conn = Connection::open(path)?;
        }
        // NOTE: auto_vacuum 只在建表前设置才生效。
        conn.pragma_update(None, "auto_vacuum", "INCREMENTAL")?;
        conn.pragma_update_and_check(None, "journal_mode", "WAL", |_| Ok(()))?;
        conn.pragma_update_and_check(None, "journal_size_limit", LOG_WAL_BYTES, |_| Ok(()))?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS log (
                id           INTEGER PRIMARY KEY,
                boot_id      INTEGER NOT NULL,
                timestamp_ms INTEGER NOT NULL,
                level        TEXT    NOT NULL,
                target       TEXT    NOT NULL,
                message      TEXT    NOT NULL,
                fields       TEXT    NOT NULL,
                file         TEXT,
                line         INTEGER,
                truncated    INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS log_boot ON log (boot_id, id);",
        )?;
        conn.pragma_update(None, "user_version", SCHEMA_VERSION)?;
        Ok(Self { conn, batches: 0 })
    }

    fn write(&mut self, boot_id: u64, batch: &[Arc<LogRecord>]) -> rusqlite::Result<()> {
        let tx = self.conn.transaction()?;
        {
            let mut insert = tx.prepare_cached(
                "INSERT INTO log (boot_id, timestamp_ms, level, target, message, fields, file, line, truncated)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            )?;
            for record in batch {
                insert.execute(params![
                    boot_id as i64,
                    record.timestamp_ms as i64,
                    record.level.as_str(),
                    record.target,
                    record.message,
                    serde_json::to_string(&record.fields).expect("日志字段可序列化"),
                    record.file,
                    record.line,
                    record.truncated,
                ])?;
            }
        }
        tx.commit()?;
        self.batches += 1;
        if self.batches.is_multiple_of(LOG_TRIM_EVERY) {
            self.trim()?;
        }
        Ok(())
    }

    /// 超过 `LOG_DB_BYTES` 时删最旧的 1/5 并回收空间。
    fn trim(&self) -> rusqlite::Result<()> {
        let bytes: i64 = self.conn.query_row(
            "SELECT (p.page_count - f.freelist_count) * s.page_size
             FROM pragma_page_count() p, pragma_freelist_count() f, pragma_page_size() s",
            [],
            |row| row.get(0),
        )?;
        if bytes <= LOG_DB_BYTES {
            return Ok(());
        }
        self.conn.execute(
            "DELETE FROM log WHERE id <= (
                SELECT id FROM log ORDER BY id LIMIT 1 OFFSET (SELECT count(*) / 5 FROM log)
            )",
            [],
        )?;
        drain(&self.conn, "PRAGMA incremental_vacuum")?;
        drain(&self.conn, "PRAGMA wal_checkpoint(TRUNCATE)")
    }
}

/// 执行可能返回行的 pragma。
fn drain(conn: &Connection, sql: &str) -> rusqlite::Result<()> {
    let mut stmt = conn.prepare(sql)?;
    let mut rows = stmt.query([])?;
    while rows.next()?.is_some() {}
    Ok(())
}

struct TextLog {
    path: PathBuf,
    file: File,
    len: u64,
}

impl TextLog {
    fn open(path: &Path) -> std::io::Result<Self> {
        let file = OpenOptions::new().create(true).append(true).open(path)?;
        let len = file.metadata()?.len();
        Ok(Self {
            path: path.to_owned(),
            file,
            len,
        })
    }

    /// 每条一行；超过 `LOG_FILE_BYTES` 先清空。
    fn write(&mut self, batch: &[Arc<LogRecord>]) -> std::io::Result<()> {
        let mut text = String::new();
        for record in batch {
            let time = DateTime::from_timestamp_millis(record.timestamp_ms as i64)
                .expect("毫秒时间戳在 chrono 范围内")
                .with_timezone(&Local)
                .to_rfc3339_opts(SecondsFormat::Millis, false);
            let mut line = format!(
                "{time} {} {} {}",
                record.level.as_str().to_uppercase(),
                record.target,
                record.message
            );
            for field in &record.fields {
                let _ = write!(line, " {}={}", field.name, field.value);
            }
            text.push_str(&line.replace('\n', "\\n"));
            text.push('\n');
        }
        if self.len + text.len() as u64 > LOG_FILE_BYTES {
            self.file.set_len(0)?;
            self.len = 0;
        }
        self.file.write_all(text.as_bytes())?;
        self.len += text.len() as u64;
        Ok(())
    }
}

/// `GET /developer/logs/history` 的响应。
#[derive(Serialize)]
pub(crate) struct History {
    storage: Storage,
    boot_id: u64,
    records: Vec<StoredRecord>,
}

#[derive(Serialize)]
struct StoredRecord {
    boot_id: u64,
    timestamp_ms: u64,
    level: LogLevel,
    target: String,
    message: String,
    fields: Vec<StoredField>,
    file: Option<String>,
    line: Option<u32>,
    truncated: bool,
}

#[derive(Serialize, Deserialize)]
struct StoredField {
    name: String,
    value: String,
}

/// 之前启动的最近 `LOG_HISTORY_RECORDS` 条，按时间升序；未落 SQLite 时为空。
pub(crate) fn history(storage: Storage, boot_id: u64) -> rusqlite::Result<History> {
    let records = match &storage {
        Storage::Sqlite { path } => read(path, boot_id)?,
        Storage::File { .. } | Storage::Memory { .. } => Vec::new(),
    };
    Ok(History {
        storage,
        boot_id,
        records,
    })
}

fn read(path: &Path, boot_id: u64) -> rusqlite::Result<Vec<StoredRecord>> {
    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let mut stmt = conn.prepare(
        "SELECT boot_id, timestamp_ms, level, target, message, fields, file, line, truncated
         FROM log
         WHERE id < coalesce(
             (SELECT min(id) FROM log WHERE boot_id = ?1),
             (SELECT max(id) + 1 FROM log),
             0
         )
         ORDER BY id DESC
         LIMIT ?2",
    )?;
    let mut records = stmt
        .query_map(params![boot_id as i64, LOG_HISTORY_RECORDS as i64], |row| {
            let level: String = row.get(2)?;
            let fields: String = row.get(5)?;
            Ok(StoredRecord {
                boot_id: row.get::<_, i64>(0)? as u64,
                timestamp_ms: row.get::<_, i64>(1)? as u64,
                level: LogLevel::parse(&level).ok_or_else(|| {
                    rusqlite::Error::FromSqlConversionFailure(
                        2,
                        Type::Text,
                        format!("未知日志级别 {level}").into(),
                    )
                })?,
                target: row.get(3)?,
                message: row.get(4)?,
                fields: serde_json::from_str(&fields).map_err(|e| {
                    rusqlite::Error::FromSqlConversionFailure(5, Type::Text, Box::new(e))
                })?,
                file: row.get(6)?,
                line: row.get(7)?,
                truncated: row.get(8)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    records.reverse();
    Ok(records)
}

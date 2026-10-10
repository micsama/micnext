//! 开发者日志：进程内有界缓冲与 tracing 采集层。
//! 契约：docs/blueprints/developer-diagnostics.md §二。

use std::collections::VecDeque;
use std::fmt::{self, Write as _};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;
use tokio::sync::watch;
use tracing::field::{Field, Visit};
use tracing::{Event, Level, Metadata, Subscriber};
use tracing_subscriber::filter::filter_fn;
use tracing_subscriber::layer::{Context, Layer};
use tracing_subscriber::registry::LookupSpan;

use crate::limits::{LOG_FIELDS, LOG_RECORDS, LOG_RECORD_BYTES, LOG_TOTAL_BYTES};

/// 进程内存日志；clone 共享同一缓冲。
#[derive(Clone)]
pub struct DeveloperLogs {
    inner: Arc<Inner>,
}

struct Inner {
    buffer: Mutex<Buffer>,
    /// 最新已追加记录的 seq；只表示「可能有新记录」。
    latest: watch::Sender<u64>,
}

#[derive(Default)]
struct Buffer {
    records: VecDeque<Arc<LogRecord>>,
    /// 最新已追加记录的 seq，0 = 尚无记录。
    latest: u64,
    bytes: usize,
    trimmed: bool,
}

#[derive(Serialize)]
pub(crate) struct LogRecord {
    #[serde(skip)]
    pub(crate) seq: u64,
    timestamp_ms: u64,
    level: LogLevel,
    target: String,
    message: String,
    fields: Vec<LogField>,
    file: Option<String>,
    line: Option<u32>,
    truncated: bool,
    #[serde(skip)]
    bytes: usize,
}

#[derive(Serialize, Clone, Copy)]
#[serde(rename_all = "lowercase")]
enum LogLevel {
    Debug,
    Info,
    Warn,
    Error,
}

#[derive(Serialize)]
struct LogField {
    name: &'static str,
    value: String,
}

/// 建立连接时的保留窗口。
pub(crate) struct Snapshot {
    pub(crate) records: Vec<Arc<LogRecord>>,
    pub(crate) latest: u64,
    pub(crate) trimmed: bool,
}

/// `after` 的结果：后续记录，或其中一部分已被淘汰。
pub(crate) enum After {
    Records(Vec<Arc<LogRecord>>),
    Evicted,
}

impl Default for DeveloperLogs {
    fn default() -> Self {
        Self::new()
    }
}

impl DeveloperLogs {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Inner {
                buffer: Mutex::new(Buffer::default()),
                latest: watch::Sender::new(0),
            }),
        }
    }

    /// 项目模块（`micnext`、`mic_*`）采 DEBUG 及以上，依赖采 INFO 及以上；不记录 span。
    pub fn layer<S>(&self) -> impl Layer<S> + Send + Sync + 'static
    where
        S: Subscriber + for<'a> LookupSpan<'a>,
    {
        Capture { logs: self.clone() }.with_filter(filter_fn(captured))
    }

    pub(crate) fn subscribe(&self) -> watch::Receiver<u64> {
        self.inner.latest.subscribe()
    }

    pub(crate) fn snapshot(&self) -> Snapshot {
        let buffer = self.lock();
        Snapshot {
            records: buffer.records.iter().cloned().collect(),
            latest: buffer.latest,
            trimmed: buffer.trimmed,
        }
    }

    /// `seq` 之后最多 `limit` 条。
    pub(crate) fn after(&self, seq: u64, limit: usize) -> After {
        let buffer = self.lock();
        let Some(front) = buffer.records.front().map(|r| r.seq) else {
            return After::Records(Vec::new());
        };
        if front > seq + 1 {
            return After::Evicted;
        }
        let skip = usize::try_from(seq + 1 - front).expect("缓冲序号差可放入 usize");
        After::Records(
            buffer
                .records
                .iter()
                .skip(skip)
                .take(limit)
                .cloned()
                .collect(),
        )
    }

    fn push(&self, mut record: LogRecord) {
        let seq = {
            let mut buffer = self.lock();
            buffer.latest += 1;
            record.seq = buffer.latest;
            buffer.bytes += record.bytes;
            buffer.records.push_back(Arc::new(record));
            while buffer.records.len() > LOG_RECORDS || buffer.bytes > LOG_TOTAL_BYTES {
                let evicted = buffer.records.pop_front().expect("超限时缓冲非空");
                buffer.bytes -= evicted.bytes;
                buffer.trimmed = true;
            }
            buffer.latest
        };
        self.inner.latest.send_replace(seq);
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Buffer> {
        self.inner.buffer.lock().expect("开发者日志锁中毒")
    }
}

fn captured(meta: &Metadata<'_>) -> bool {
    if !meta.is_event() {
        return false;
    }
    let project = meta
        .module_path()
        .and_then(|path| path.split("::").next())
        .is_some_and(|root| root == "micnext" || root.starts_with("mic_"));
    let floor = if project { Level::DEBUG } else { Level::INFO };
    *meta.level() <= floor
}

struct Capture {
    logs: DeveloperLogs,
}

impl<S: Subscriber> Layer<S> for Capture {
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        let meta = event.metadata();
        let level = match *meta.level() {
            Level::ERROR => LogLevel::Error,
            Level::WARN => LogLevel::Warn,
            Level::INFO => LogLevel::Info,
            _ => LogLevel::Debug,
        };
        let mut budget = Budget {
            left: LOG_RECORD_BYTES,
            truncated: false,
        };
        let target = budget.take(meta.target());
        let file = meta.file().map(|file| budget.take(file));
        let mut visitor = Collect {
            budget,
            message: String::new(),
            fields: Vec::new(),
        };
        event.record(&mut visitor);
        let Collect {
            budget,
            message,
            fields,
        } = visitor;
        let bytes = LOG_RECORD_BYTES - budget.left;
        self.logs.push(LogRecord {
            seq: 0,
            timestamp_ms: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |d| d.as_millis() as u64),
            level,
            target,
            message,
            fields,
            file,
            line: meta.line(),
            truncated: budget.truncated,
            bytes,
        });
    }
}

/// 单条记录的剩余文本字节。
struct Budget {
    left: usize,
    truncated: bool,
}

impl Budget {
    fn take(&mut self, text: &str) -> String {
        let mut out = String::new();
        let _ = Limited {
            out: &mut out,
            budget: self,
        }
        .write_str(text);
        out
    }
}

/// 超出预算时在字符边界截断并中止格式化。
struct Limited<'a> {
    out: &'a mut String,
    budget: &'a mut Budget,
}

impl fmt::Write for Limited<'_> {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        if s.len() <= self.budget.left {
            self.out.push_str(s);
            self.budget.left -= s.len();
            return Ok(());
        }
        let mut end = self.budget.left;
        while !s.is_char_boundary(end) {
            end -= 1;
        }
        self.out.push_str(&s[..end]);
        self.budget.left -= end;
        self.budget.truncated = true;
        Err(fmt::Error)
    }
}

struct Collect {
    budget: Budget,
    message: String,
    fields: Vec<LogField>,
}

impl Collect {
    fn write(&mut self, field: &Field, value: fmt::Arguments<'_>) {
        if field.name() == "message" {
            let _ = Limited {
                out: &mut self.message,
                budget: &mut self.budget,
            }
            .write_fmt(value);
            return;
        }
        if self.fields.len() == LOG_FIELDS {
            self.budget.truncated = true;
            return;
        }
        let name = field.name();
        let mut out = String::new();
        if self.budget.left >= name.len() {
            self.budget.left -= name.len();
            let _ = Limited {
                out: &mut out,
                budget: &mut self.budget,
            }
            .write_fmt(value);
        } else {
            self.budget.truncated = true;
        }
        self.fields.push(LogField { name, value: out });
    }
}

impl Visit for Collect {
    fn record_str(&mut self, field: &Field, value: &str) {
        self.write(field, format_args!("{value}"));
    }

    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        self.write(field, format_args!("{value:?}"));
    }
}

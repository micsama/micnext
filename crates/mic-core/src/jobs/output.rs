//! 后台任务输出：`jobs/<id>/` 下按全程起始偏移命名的段文件，只保留最近部分，
//! 可读范围与总字节数都由目录内容推出，不另存。

use std::collections::VecDeque;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tokio::time::Instant;

use mic_message::SessionId;
use serde::Serialize;

use crate::event::{Events, KernelEventKind};
use crate::limits::{JOB_NOTIFY_INTERVAL, JOB_OUTPUT_KEEP, JOB_OUTPUT_SEGMENT};

/// 一次读取的结果；偏移均为全程字节偏移。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct JobOutput {
    pub text: String,
    pub offset: u64,
    pub next_offset: u64,
    /// 已读到当前末尾；任务仍在运行时之后还会增长。
    pub eof: bool,
    /// 更早的输出已丢弃。
    pub available_from: u64,
    pub total_bytes: u64,
}

/// 按起始偏移升序的段 `(start, len)`；目录不存在即无输出。
fn segments(dir: &Path) -> io::Result<Vec<(u64, u64)>> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };
    let mut segs = Vec::new();
    for entry in entries {
        let entry = entry?;
        let Some(start) = entry
            .file_name()
            .to_str()
            .and_then(|n| n.parse::<u64>().ok())
        else {
            continue;
        };
        segs.push((start, entry.metadata()?.len()));
    }
    segs.sort_unstable();
    Ok(segs)
}

fn segment_path(dir: &Path, start: u64) -> PathBuf {
    dir.join(format!("{start:020}"))
}

fn bounds(segs: &[(u64, u64)]) -> (u64, u64) {
    match (segs.first(), segs.last()) {
        (Some(&(first, _)), Some(&(last, len))) => (first, last + len),
        _ => (0, 0),
    }
}

/// 读 `[offset, offset+limit)`；`offset` 缺省读尾部，早于可读范围的从最早处读。
/// 两端按 UTF-8 修整：开头跳过续字节，结尾不完整的字符留给下次。
pub(crate) fn read(dir: &Path, offset: Option<u64>, limit: u64) -> io::Result<JobOutput> {
    // 写端可能在列目录与打开之间丢掉旧段，此时重列。
    loop {
        let segs = segments(dir)?;
        let (available_from, total) = bounds(&segs);
        let start = match offset {
            None => total.saturating_sub(limit).max(available_from),
            Some(o) => o.clamp(available_from, total),
        };
        let end = (start + limit).min(total);
        let mut bytes = Vec::with_capacity((end - start) as usize);
        let mut vanished = false;
        for &(seg, len) in &segs {
            let (from, to) = (start.max(seg), end.min(seg + len));
            if from >= to {
                continue;
            }
            let mut file = match File::open(segment_path(dir, seg)) {
                Ok(f) => f,
                Err(e) if e.kind() == io::ErrorKind::NotFound => {
                    vanished = true;
                    break;
                }
                Err(e) => return Err(e),
            };
            file.seek(SeekFrom::Start(from - seg))?;
            file.take(to - from).read_to_end(&mut bytes)?;
        }
        if vanished {
            continue;
        }
        let lead = bytes.iter().take_while(|b| (*b & 0xC0) == 0x80).count();
        let lead = if lead <= 3 { lead } else { 0 };
        let body = &bytes[lead..];
        let keep = body.len() - incomplete_tail(body);
        let offset = start + lead as u64;
        let next_offset = offset + keep as u64;
        return Ok(JobOutput {
            text: String::from_utf8_lossy(&body[..keep]).into_owned(),
            offset,
            next_offset,
            eof: next_offset >= total,
            available_from,
            total_bytes: total,
        });
    }
}

/// 末尾未写完的多字节字符的字节数。
fn incomplete_tail(bytes: &[u8]) -> usize {
    for back in 1..=bytes.len().min(3) {
        let b = bytes[bytes.len() - back];
        if b & 0xC0 == 0x80 {
            continue;
        }
        let width = match b {
            0xC0..=0xDF => 2,
            0xE0..=0xEF => 3,
            0xF0..=0xF7 => 4,
            _ => return 0,
        };
        return if width > back { back } else { 0 };
    }
    0
}

/// 任务所在会话树，用于列表刷新通知。
#[derive(Clone)]
pub(crate) struct Tree {
    pub(crate) root: SessionId,
    pub(crate) channel: String,
}

/// 后台任务输出日志的写端。
pub(crate) struct Writer {
    segments: Arc<Mutex<Segments>>,
    notifier: Arc<Notifier>,
}

/// 收尾前封口：等在途写入落完，之后到达的写入丢弃，读到的尾部即最终输出。
pub(crate) struct Seal(Arc<Mutex<Segments>>);

struct Segments {
    dir: PathBuf,
    segs: VecDeque<(u64, u64)>,
    file: Option<File>,
    total: u64,
    sealed: bool,
}

impl Writer {
    pub(crate) fn create(dir: PathBuf, events: Events, tree: Tree) -> io::Result<(Self, Seal)> {
        fs::create_dir_all(&dir)?;
        let segments = Arc::new(Mutex::new(Segments {
            dir,
            segs: VecDeque::new(),
            file: None,
            total: 0,
            sealed: false,
        }));
        let writer = Self {
            segments: segments.clone(),
            notifier: Arc::new(Notifier {
                events,
                tree,
                state: Mutex::default(),
            }),
        };
        Ok((writer, Seal(segments)))
    }
}

impl Seal {
    pub(crate) async fn seal(self) {
        tokio::task::spawn_blocking(move || self.0.lock().expect("输出段未中毒").sealed = true)
            .await
            .expect("封口任务不会 panic");
    }
}

/// 输出增长通知：每 [`JOB_NOTIFY_INTERVAL`] 至多一次，窗口内的增长在窗口结束时补发。
struct Notifier {
    events: Events,
    tree: Tree,
    state: Mutex<Throttle>,
}

#[derive(Default)]
struct Throttle {
    last: Option<Instant>,
    pending: bool,
}

impl Notifier {
    fn grew(self: &Arc<Self>) {
        let mut t = self.state.lock().expect("通知节流未中毒");
        match t.last.map(|at| at.elapsed()) {
            Some(elapsed) if elapsed < JOB_NOTIFY_INTERVAL => {
                if !t.pending {
                    t.pending = true;
                    let this = self.clone();
                    tokio::spawn(async move {
                        tokio::time::sleep(JOB_NOTIFY_INTERVAL - elapsed).await;
                        let mut t = this.state.lock().expect("通知节流未中毒");
                        t.pending = false;
                        t.last = Some(Instant::now());
                        this.emit();
                    });
                }
            }
            _ => {
                t.last = Some(Instant::now());
                self.emit();
            }
        }
    }

    fn emit(&self) {
        self.events.emit(
            self.tree.root,
            &self.tree.channel,
            KernelEventKind::JobsChanged,
        );
    }
}

impl Segments {
    fn append(&mut self, mut bytes: &[u8]) -> io::Result<()> {
        if self.sealed {
            return Ok(());
        }
        while !bytes.is_empty() {
            let full = self
                .segs
                .back()
                .is_none_or(|&(_, len)| len >= JOB_OUTPUT_SEGMENT);
            if full || self.file.is_none() {
                self.file = Some(
                    OpenOptions::new()
                        .create_new(true)
                        .append(true)
                        .open(segment_path(&self.dir, self.total))?,
                );
                self.segs.push_back((self.total, 0));
            }
            let (_, len) = self.segs.back_mut().expect("刚确保有当前段");
            let n = (JOB_OUTPUT_SEGMENT - *len).min(bytes.len() as u64) as usize;
            self.file
                .as_mut()
                .expect("刚确保有当前段文件")
                .write_all(&bytes[..n])?;
            *len += n as u64;
            self.total += n as u64;
            bytes = &bytes[n..];
        }
        while let (Some(&(start, len)), true) = (self.segs.front(), self.segs.len() > 1) {
            if self.total - (start + len) < JOB_OUTPUT_KEEP {
                break;
            }
            fs::remove_file(segment_path(&self.dir, start))?;
            self.segs.pop_front();
        }
        Ok(())
    }
}

impl Writer {
    pub(crate) async fn write(&mut self, bytes: &[u8]) -> io::Result<()> {
        let segments = self.segments.clone();
        let buf = bytes.to_vec();
        tokio::task::spawn_blocking(move || segments.lock().expect("输出段未中毒").append(&buf))
            .await
            .map_err(io::Error::other)??;
        self.notifier.grew();
        Ok(())
    }
}

#[cfg(test)]
mod tests;

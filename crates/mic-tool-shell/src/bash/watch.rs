//! 后台 `notify_pattern`：stdout、stderr 各自逐行匹配；首个命中立即通知，冷却内的命中攒到冷却结束合成一条。

use std::collections::VecDeque;

use regex::Regex;
use tokio::time::Instant;

use crate::limits::{NOTIFY_BATCH_LINES, NOTIFY_COOLDOWN, NOTIFY_LINE_BYTES, NOTIFY_MAX};

#[derive(Debug, Clone, Copy)]
pub(super) enum Stream {
    Stdout,
    Stderr,
}

impl Stream {
    fn as_str(self) -> &'static str {
        match self {
            Self::Stdout => "stdout",
            Self::Stderr => "stderr",
        }
    }
}

pub(super) struct Watch {
    pattern: Regex,
    stdout: Lines,
    stderr: Lines,
    sent: u32,
    last: Option<Instant>,
    /// 冷却内攒下的命中：只留最近 [`NOTIFY_BATCH_LINES`] 行，`held` 计总数。
    pending: VecDeque<Hit>,
    held: usize,
}

struct Hit {
    stream: Stream,
    line: Line,
}

impl Watch {
    pub(super) fn new(pattern: Regex) -> Self {
        Self {
            pattern,
            stdout: Lines::default(),
            stderr: Lines::default(),
            sent: 0,
            last: None,
            pending: VecDeque::new(),
            held: 0,
        }
    }

    /// 喂入一块读到的输出，返回可立即发出的通知；其余命中留待 [`Self::flush`]。
    pub(super) fn feed(&mut self, stream: Stream, bytes: &[u8]) -> Option<String> {
        let mut notice = None;
        let lines = self.lines(stream);
        lines.buf.extend_from_slice(bytes);
        while let Some(line) = self.lines(stream).next() {
            notice = notice.or(self.check(stream, line));
        }
        notice
    }

    /// 该路 EOF：匹配未带换行的最后一行。
    pub(super) fn finish(&mut self, stream: Stream) -> Option<String> {
        let line = self.lines(stream).rest()?;
        self.check(stream, line)
    }

    /// 攒有命中时，冷却结束的时刻。
    pub(super) fn due(&self) -> Option<Instant> {
        let last = self.last?;
        (!self.pending.is_empty()).then(|| last + NOTIFY_COOLDOWN)
    }

    /// 冷却结束：把攒下的命中合成一条通知。
    pub(super) fn flush(&mut self) -> Option<String> {
        if self.pending.is_empty() {
            return None;
        }
        let held = std::mem::take(&mut self.held);
        let hits: Vec<Hit> = self.pending.drain(..).collect();
        Some(self.emit(held, hits))
    }

    fn lines(&mut self, stream: Stream) -> &mut Lines {
        match stream {
            Stream::Stdout => &mut self.stdout,
            Stream::Stderr => &mut self.stderr,
        }
    }

    fn check(&mut self, stream: Stream, line: Line) -> Option<String> {
        if self.sent >= NOTIFY_MAX || !self.pattern.is_match(&line.text) {
            return None;
        }
        let hit = Hit { stream, line };
        let cooling = self
            .last
            .is_some_and(|at| Instant::now() - at < NOTIFY_COOLDOWN);
        if cooling || !self.pending.is_empty() {
            if self.pending.len() == NOTIFY_BATCH_LINES {
                self.pending.pop_front();
            }
            self.pending.push_back(hit);
            self.held += 1;
            return None;
        }
        Some(self.emit(1, vec![hit]))
    }

    fn emit(&mut self, held: usize, hits: Vec<Hit>) -> String {
        self.last = Some(Instant::now());
        self.sent += 1;
        let mut head = if held == 1 {
            "notify_pattern matched".to_owned()
        } else if held == hits.len() {
            format!("notify_pattern matched {held} lines during cooldown")
        } else {
            format!(
                "notify_pattern matched {held} lines during cooldown, latest {} shown",
                hits.len()
            )
        };
        head.push_str(&format!(" (notice {}/{NOTIFY_MAX}", self.sent));
        if self.sent == NOTIFY_MAX {
            head.push_str("; further matches will not be reported");
        }
        head.push_str("):");
        for Hit { stream, line } in hits {
            let piece = if line.fragment {
                format!(" ({NOTIFY_LINE_BYTES}-byte piece of a long line)")
            } else {
                String::new()
            };
            head.push_str(&format!("\n{}{piece}: {}", stream.as_str(), line.text));
        }
        head
    }
}

struct Line {
    text: String,
    fragment: bool,
}

/// 一路输出里尚未成行的部分。
#[derive(Default)]
struct Lines {
    buf: Vec<u8>,
    /// 正处在一条超长行中间：之后的片段（含收尾那段）都算片段。
    continued: bool,
}

impl Lines {
    /// 取出下一个完整行或超长行片段；不足则留待后续输出。
    fn next(&mut self) -> Option<Line> {
        let newline = self.buf.iter().position(|&b| b == b'\n');
        match newline {
            Some(i) if i <= NOTIFY_LINE_BYTES => {
                let mut line: Vec<u8> = self.buf.drain(..=i).collect();
                line.pop();
                if line.last() == Some(&b'\r') {
                    line.pop();
                }
                let fragment = std::mem::take(&mut self.continued);
                Some(Line {
                    text: String::from_utf8_lossy(&line).into_owned(),
                    fragment,
                })
            }
            _ if self.buf.len() > NOTIFY_LINE_BYTES => {
                // 切点退到字符边界，被切开的字符留给下一片。
                let mut cut = NOTIFY_LINE_BYTES;
                while cut > NOTIFY_LINE_BYTES - 3 && self.buf[cut] & 0xC0 == 0x80 {
                    cut -= 1;
                }
                let piece: Vec<u8> = self.buf.drain(..cut).collect();
                self.continued = true;
                Some(Line {
                    text: String::from_utf8_lossy(&piece).into_owned(),
                    fragment: true,
                })
            }
            _ => None,
        }
    }

    fn rest(&mut self) -> Option<Line> {
        if self.buf.is_empty() {
            return None;
        }
        let rest = std::mem::take(&mut self.buf);
        Some(Line {
            text: String::from_utf8_lossy(&rest).into_owned(),
            fragment: std::mem::take(&mut self.continued),
        })
    }
}

#[cfg(test)]
mod tests;

//! 后台 `notify_pattern`：stdout、stderr 各自逐行匹配，命中按冷却与次数上限转成中途通知。

use regex::Regex;
use tokio::time::Instant;

use crate::limits::{NOTIFY_COOLDOWN, NOTIFY_LINE_BYTES, NOTIFY_MAX};

#[derive(Clone, Copy)]
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
}

impl Watch {
    pub(super) fn new(pattern: Regex) -> Self {
        Self {
            pattern,
            stdout: Lines::default(),
            stderr: Lines::default(),
            sent: 0,
            last: None,
        }
    }

    /// 喂入一块读到的输出，返回要发出的通知。同一时刻至多一条能越过冷却。
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
        let now = Instant::now();
        if self.last.is_some_and(|at| now - at < NOTIFY_COOLDOWN) {
            return None;
        }
        self.last = Some(now);
        self.sent += 1;
        let mut notes = vec![format!("notice {}/{NOTIFY_MAX}", self.sent)];
        if line.fragment {
            notes.push(format!("a {NOTIFY_LINE_BYTES}-byte piece of a long line"));
        }
        if self.sent == NOTIFY_MAX {
            notes.push("further matches will not be reported".to_owned());
        }
        Some(format!(
            "{} matched notify_pattern ({}):\n{}",
            stream.as_str(),
            notes.join("; "),
            line.text
        ))
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

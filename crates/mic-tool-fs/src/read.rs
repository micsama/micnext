use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;

use mic_message::ContentPart;
use mic_tool::{Tool, ToolContext, ToolError};
use schemars::JsonSchema;
use serde::Deserialize;

use crate::common::{blocking, display_path, io_error, is_binary, not_text, resolve};
use crate::limits::{READ_LIMIT, READ_MAX_BYTES, READ_MAX_LINE_CHARS};

pub(crate) struct Read;

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct Args {
    /// Path to read; a relative path resolves against the session workspace.
    file_path: String,
    /// 1-based first line to return. Defaults to 1.
    offset: Option<u64>,
    /// Maximum number of lines to return. Defaults to 2000.
    limit: Option<u64>,
}

impl Tool for Read {
    type Args = Args;

    fn name(&self) -> &str {
        "read"
    }

    fn description(&self) -> &str {
        "Read a UTF-8 text file and return line-numbered content."
    }

    fn prompt_hint(&self) -> Option<&str> {
        Some("View text files with read, not cat; output has line numbers; page large files with offset and limit.")
    }

    async fn execute(&self, args: Args, ctx: &ToolContext) -> Result<Vec<ContentPart>, ToolError> {
        let path = resolve(ctx, &args.file_path)?;
        let offset = positive(args.offset, "offset")?.unwrap_or(1);
        let limit = positive(args.limit, "limit")?.unwrap_or(READ_LIMIT);
        if limit > READ_LIMIT {
            return Err(ToolError::input(format!(
                "limit must be less than or equal to {READ_LIMIT}"
            )));
        }
        let display = display_path(ctx.cwd(), &path);
        let text = blocking(move |_| read_window(path, &display, offset, limit)).await?;
        Ok(vec![ContentPart::Text { text }])
    }
}

fn positive(value: Option<u64>, name: &str) -> Result<Option<u64>, ToolError> {
    match value {
        Some(0) => Err(ToolError::input(format!(
            "{name} must be a positive integer"
        ))),
        v => Ok(v),
    }
}

struct Window {
    lines: Vec<(u64, String)>,
    total: u64,
    capped: bool,
}

fn read_window(path: PathBuf, display: &str, offset: u64, limit: u64) -> Result<String, ToolError> {
    let io = |e| io_error("read", display, e);
    let meta = std::fs::metadata(&path).map_err(io)?;
    if !meta.is_file() {
        return Err(ToolError::business(format!(
            "cannot read \"{display}\": not a regular file"
        )));
    }
    let mut reader = BufReader::new(File::open(&path).map_err(io)?);
    if is_binary(reader.fill_buf().map_err(io)?) {
        return Err(not_text("read", display, "binary file"));
    }

    let mut window = Window {
        lines: Vec::new(),
        total: 0,
        capped: false,
    };
    let mut bytes = 0;
    let mut raw = Vec::new();
    loop {
        raw.clear();
        if reader.read_until(b'\n', &mut raw).map_err(io)? == 0 {
            break;
        }
        let line =
            std::str::from_utf8(&raw).map_err(|_| not_text("read", display, "not valid UTF-8"))?;
        window.total += 1;
        if window.capped || window.total < offset || window.lines.len() as u64 >= limit {
            continue;
        }
        let line = line.strip_suffix('\n').unwrap_or(line);
        let line = truncate_line(line.strip_suffix('\r').unwrap_or(line));
        let size = line.len() + usize::from(!window.lines.is_empty());
        if bytes + size > READ_MAX_BYTES {
            window.capped = true;
            continue;
        }
        bytes += size;
        window.lines.push((window.total, line));
    }

    if !window.capped && offset > window.total && !(window.total == 0 && offset == 1) {
        return Err(ToolError::input(format!(
            "offset {offset} is out of range for \"{display}\" ({} lines)",
            window.total
        )));
    }
    Ok(render(display, offset, &window))
}

fn truncate_line(line: &str) -> String {
    match line.char_indices().nth(READ_MAX_LINE_CHARS) {
        Some((cut, _)) => format!(
            "{}... (line truncated to {READ_MAX_LINE_CHARS} chars)",
            &line[..cut]
        ),
        None => line.to_owned(),
    }
}

fn render(display: &str, offset: u64, window: &Window) -> String {
    let end = window.lines.last().map_or(offset - 1, |(n, _)| *n);
    let footer = if window.capped {
        format!(
            "(Output capped. Showing lines {offset}-{end}. Use offset={} to continue.)",
            end + 1
        )
    } else if end < window.total {
        format!(
            "(Showing lines {offset}-{end} of {}. Use offset={} to continue.)",
            window.total,
            end + 1
        )
    } else {
        format!("(End of file - total {} lines)", window.total)
    };
    let body = if window.lines.is_empty() {
        footer
    } else {
        let lines: Vec<String> = window
            .lines
            .iter()
            .map(|(n, text)| format!("{n}: {text}"))
            .collect();
        format!("{}\n\n{footer}", lines.join("\n"))
    };
    format!("<path>{display}</path>\n<type>file</type>\n<content>\n{body}\n</content>")
}

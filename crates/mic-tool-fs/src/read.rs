use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;

use mic_media::MediaError;
use mic_message::limits::MAX_IMAGE_BYTES;
use mic_message::{ContentPart, ImageData};
use mic_tool::{Tool, ToolContext, ToolError, ToolOutput};
use schemars::JsonSchema;
use serde::Deserialize;

use crate::common::{blocking, display_path, io_error, is_binary, not_text, resolve};
use crate::limits::{READ_LIMIT, READ_MAX_BYTES, READ_MAX_LINE_CHARS};

pub(crate) struct Read;

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct Args {
    /// Absolute, or relative to the working directory.
    file_path: String,
    /// First line, 1-based.
    offset: Option<u64>,
    /// Lines; default 2000.
    limit: Option<u64>,
}

impl Tool for Read {
    type Args = Args;

    fn name(&self) -> &str {
        "read"
    }

    fn description(&self) -> &str {
        "Read a UTF-8 file with line numbers, or view a PNG/JPEG/WebP/GIF image. A long file comes in pages, each ending with the offset to continue from."
    }

    async fn execute(&self, args: Args, ctx: &ToolContext) -> Result<ToolOutput, ToolError> {
        let path = resolve(ctx, &args.file_path)?;
        let paged = args.offset.is_some() || args.limit.is_some();
        let offset = positive(args.offset, "offset")?.unwrap_or(1);
        let limit = positive(args.limit, "limit")?.unwrap_or(READ_LIMIT);
        if limit > READ_LIMIT {
            return Err(ToolError::input(format!(
                "limit must be less than or equal to {READ_LIMIT}"
            )));
        }
        let display = display_path(ctx.cwd(), &path);
        let read = blocking(move |_| read_file(path, &display, offset, limit)).await?;
        Ok(match read {
            Loaded::Text(text) => ToolOutput::completed(vec![ContentPart::Text { text }]),
            Loaded::Image { text, image } if !paged => ToolOutput::Image { text, image },
            Loaded::Image { .. } => {
                return Err(ToolError::input(
                    "offset and limit apply to text files only",
                ))
            }
        })
    }
}

enum Loaded {
    Text(String),
    Image { text: String, image: ImageData },
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

fn read_file(path: PathBuf, display: &str, offset: u64, limit: u64) -> Result<Loaded, ToolError> {
    let io = |e| io_error("read", display, e);
    let meta = std::fs::metadata(&path).map_err(io)?;
    if !meta.is_file() {
        return Err(ToolError::business(format!(
            "cannot read \"{display}\": not a regular file"
        )));
    }
    let mut reader = BufReader::new(File::open(&path).map_err(io)?);
    if is_binary(reader.fill_buf().map_err(io)?) {
        return read_image(reader, meta.len(), display);
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
    Ok(Loaded::Text(render(display, offset, &window)))
}

/// 二进制文件按图片规整；不是图片仍报二进制。
fn read_image(
    mut reader: impl std::io::Read,
    size: u64,
    display: &str,
) -> Result<Loaded, ToolError> {
    let io = |e| io_error("read", display, e);
    if size > MAX_IMAGE_BYTES as u64 {
        return Err(not_text(
            "read",
            display,
            "binary file, too large to view as an image",
        ));
    }
    let mut bytes = Vec::with_capacity(size as usize);
    reader.read_to_end(&mut bytes).map_err(io)?;
    match mic_media::normalize(bytes) {
        Ok(image) => Ok(Loaded::Image {
            text: format!("<path>{display}</path>\n<type>image</type>"),
            image,
        }),
        Err(MediaError::Unsupported) => Err(not_text("read", display, "binary file")),
        Err(e) => Err(not_text("read", display, &format!("invalid image: {e}"))),
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn png() -> Vec<u8> {
        let mut out = std::io::Cursor::new(Vec::new());
        image::RgbImage::new(2, 2)
            .write_to(&mut out, image::ImageFormat::Png)
            .unwrap();
        out.into_inner()
    }

    #[test]
    fn binary_file_is_an_image_only_if_it_decodes() {
        let dir = tempfile::tempdir().unwrap();
        let good = dir.path().join("a.png");
        std::fs::write(&good, png()).unwrap();
        assert!(matches!(
            read_file(good, "a.png", 1, 10),
            Ok(Loaded::Image { .. })
        ));
        let bad = dir.path().join("b.bin");
        std::fs::write(&bad, [0u8, 1, 2, 3]).unwrap();
        let err = read_file(bad, "b.bin", 1, 10).err().unwrap();
        assert!(err.message.contains("binary file"));
    }
}

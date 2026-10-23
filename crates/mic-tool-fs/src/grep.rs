use std::path::{Path, PathBuf};
use std::time::Instant;

use grep_regex::{RegexMatcher, RegexMatcherBuilder};
use grep_searcher::{sinks, BinaryDetection, Searcher, SearcherBuilder};
use ignore::overrides::OverrideBuilder;
use ignore::WalkBuilder;
use mic_message::ContentPart;
use mic_tool::{Tool, ToolContext, ToolError, ToolOutput};
use schemars::JsonSchema;
use serde::Deserialize;

use crate::common::{blocking, display_path, io_error, resolve, Cancel, ScanErrors};
use crate::limits::{GREP_MAX_LINE_BYTES, GREP_MAX_MATCHES, SEARCH_TIMEOUT};

pub(crate) struct Grep;

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct Args {
    pattern: String,
    path: Option<String>,
    /// One glob such as "*.{js,jsx}"; no lists or negation.
    include: Option<String>,
}

impl Tool for Grep {
    type Args = Args;

    fn name(&self) -> &str {
        "grep"
    }

    fn description(&self) -> &str {
        "Search file contents with ripgrep. Returns matching lines with line numbers, grouped by file; at most 250 matches, with the total if capped."
    }

    async fn execute(&self, args: Args, ctx: &ToolContext) -> Result<ToolOutput, ToolError> {
        if args.pattern.is_empty() {
            return Err(ToolError::input("pattern must be a non-empty string"));
        }
        let root = match &args.path {
            Some(p) => resolve(ctx, p)?,
            None => ctx.cwd().to_path_buf(),
        };
        let cwd = ctx.cwd().to_path_buf();
        let text = blocking(move |cancel| {
            let found = search(&root, &args, &cwd, &cancel)?;
            Ok::<_, ToolError>(render(&found))
        })
        .await?;
        Ok(ToolOutput::completed(vec![ContentPart::Text { text }]))
    }
}

struct Match {
    path: String,
    line: u64,
    text: String,
}

struct Found {
    kept: Vec<Match>,
    seen: usize,
}

fn search(root: &Path, args: &Args, cwd: &Path, cancel: &Cancel) -> Result<Found, ToolError> {
    if !root
        .try_exists()
        .map_err(|error| io_error("stat", &display_path(cwd, root), error))?
    {
        return Err(ToolError::input(format!(
            "path not found: \"{}\"",
            display_path(cwd, root)
        )));
    }
    let matcher = RegexMatcherBuilder::new()
        .line_terminator(Some(b'\n'))
        .build(&args.pattern)
        .map_err(|e| ToolError::input(format!("invalid regex: {e}")))?;
    let mut walk = WalkBuilder::new(root);
    walk.sort_by_file_name(|a, b| a.cmp(b));
    if let Some(include) = &args.include {
        let overrides = OverrideBuilder::new(root)
            .add(include)
            .and_then(|b| b.build())
            .map_err(|e| ToolError::input(format!("invalid include glob: {e}")))?;
        walk.overrides(overrides);
    }
    let mut searcher = SearcherBuilder::new()
        .binary_detection(BinaryDetection::quit(b'\x00'))
        .line_number(true)
        .build();

    let deadline = Instant::now() + SEARCH_TIMEOUT;
    let mut found = Found {
        kept: Vec::new(),
        seen: 0,
    };
    let mut errors = ScanErrors::new("grep");
    for entry in walk.build() {
        if cancel.is_cancelled() {
            break;
        }
        if Instant::now() > deadline {
            return Err(timed_out());
        }
        // 不可读的条目跳过，与 rg 一致。
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                if error
                    .io_error()
                    .is_none_or(|e| e.kind() != std::io::ErrorKind::NotFound)
                {
                    errors.record(root, &error);
                }
                continue;
            }
        };
        if !entry.file_type().is_some_and(|t| t.is_file()) {
            continue;
        }
        search_file(
            &mut searcher,
            &matcher,
            entry.path(),
            cwd,
            deadline,
            &mut found,
            &mut errors,
        );
    }
    if Instant::now() > deadline {
        return Err(timed_out());
    }
    Ok(found)
}

fn search_file(
    searcher: &mut Searcher,
    matcher: &RegexMatcher,
    path: &Path,
    cwd: &Path,
    deadline: Instant,
    found: &mut Found,
    errors: &mut ScanErrors,
) {
    let display = display_path(cwd, path);
    // 单个文件读失败不影响其余结果，与 rg 一致。
    if let Err(error) = searcher.search_path(
        matcher,
        PathBuf::from(path),
        sinks::Lossy(|line, text| {
            found.seen += 1;
            if found.kept.len() < GREP_MAX_MATCHES {
                found.kept.push(Match {
                    path: display.clone(),
                    line,
                    text: preview(text.trim_end_matches(['\n', '\r'])),
                });
            }
            Ok(Instant::now() <= deadline)
        }),
    ) {
        if error.kind() != std::io::ErrorKind::NotFound {
            errors.record(path, &error);
        }
    }
}

fn timed_out() -> ToolError {
    ToolError::input(format!(
        "grep timed out after {}s; narrow pattern, path, or include",
        SEARCH_TIMEOUT.as_secs()
    ))
}

fn preview(line: &str) -> String {
    if line.len() <= GREP_MAX_LINE_BYTES {
        return line.to_owned();
    }
    let mut cut = GREP_MAX_LINE_BYTES;
    while !line.is_char_boundary(cut) {
        cut -= 1;
    }
    format!("{} (line truncated)", &line[..cut])
}

fn render(found: &Found) -> String {
    if found.seen == 0 {
        return "No matches found".to_owned();
    }
    let mut groups: Vec<String> = Vec::new();
    let mut current: Option<&str> = None;
    for m in &found.kept {
        if current != Some(m.path.as_str()) {
            groups.push(m.path.clone());
            current = Some(&m.path);
        }
        let group = groups.last_mut().expect("刚推入分组");
        group.push_str(&format!("\nLine {}: {}", m.line, m.text));
    }
    let body = groups.join("\n\n");
    if found.kept.len() == found.seen {
        let noun = if found.seen == 1 { "match" } else { "matches" };
        return format!("Found {} {noun}\n\n{body}", found.seen);
    }
    format!(
        "Found {} of {} matches\n\n{body}\n\n(Narrow pattern, path, or include to see more.)",
        found.kept.len(),
        found.seen
    )
}

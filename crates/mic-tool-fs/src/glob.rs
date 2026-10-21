use std::path::{Path, PathBuf};
use std::time::SystemTime;

use ignore::overrides::OverrideBuilder;
use ignore::WalkBuilder;
use mic_message::ContentPart;
use mic_tool::{Tool, ToolContext, ToolError, ToolOutput};
use schemars::JsonSchema;
use serde::Deserialize;

use crate::common::{blocking, display_path, io_error, resolve, Cancel, ScanErrors};
use crate::limits::GLOB_MAX_RESULTS;

pub(crate) struct Glob;

/// 版本控制元数据目录，始终排除。
const VCS_DIRS: &[&str] = &[".git", ".svn", ".hg", ".bzr", ".jj", ".sl"];

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct Args {
    /// Without "/", matches basenames at any depth.
    pattern: String,
    path: Option<String>,
}

impl Tool for Glob {
    type Args = Args;

    fn name(&self) -> &str {
        "glob"
    }

    fn description(&self) -> &str {
        "Find files (not directories) by glob, including hidden and ignored files but not VCS metadata. At most 100, newest first, with the total if capped."
    }

    async fn execute(&self, args: Args, ctx: &ToolContext) -> Result<ToolOutput, ToolError> {
        if args.pattern.trim().is_empty() {
            return Err(ToolError::input("pattern must be a non-empty string"));
        }
        let root = match &args.path {
            Some(p) => resolve(ctx, p)?,
            None => ctx.cwd().to_path_buf(),
        };
        let cwd = ctx.cwd().to_path_buf();
        let text = blocking(move |cancel| {
            let paths = find(&root, &args.pattern, &cwd, &cancel)?;
            Ok::<_, ToolError>(render(&paths, &cwd))
        })
        .await?;
        Ok(ToolOutput::Completed(vec![ContentPart::Text { text }]))
    }
}

fn find(
    root: &Path,
    pattern: &str,
    cwd: &Path,
    cancel: &Cancel,
) -> Result<Vec<PathBuf>, ToolError> {
    let is_dir = match std::fs::metadata(root) {
        Ok(metadata) => metadata.is_dir(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
        Err(error) => return Err(io_error("stat", &display_path(cwd, root), error)),
    };
    if !is_dir {
        return Err(ToolError::input(format!(
            "path is not an existing directory: \"{}\"",
            display_path(cwd, root)
        )));
    }
    let overrides = OverrideBuilder::new(root)
        .add(pattern)
        .and_then(|b| b.build())
        .map_err(|e| ToolError::input(format!("invalid glob pattern: {e}")))?;
    let walk = WalkBuilder::new(root)
        .standard_filters(false)
        .overrides(overrides)
        .filter_entry(|e| !VCS_DIRS.iter().any(|d| e.file_name() == *d))
        .build();

    let mut found: Vec<(SystemTime, PathBuf)> = Vec::new();
    let mut errors = ScanErrors::new("glob");
    for entry in walk {
        if cancel.is_cancelled() {
            break;
        }
        // 无权限等不可读的条目跳过，与 rg 一致。
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
        let modified = match entry.metadata() {
            Ok(metadata) => match metadata.modified() {
                Ok(modified) => modified,
                Err(error) => {
                    errors.record(entry.path(), &error);
                    SystemTime::UNIX_EPOCH
                }
            },
            Err(error) => {
                if error
                    .io_error()
                    .is_none_or(|e| e.kind() != std::io::ErrorKind::NotFound)
                {
                    errors.record(entry.path(), &error);
                }
                SystemTime::UNIX_EPOCH
            }
        };
        found.push((modified, entry.into_path()));
    }
    found.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    Ok(found.into_iter().map(|(_, p)| p).collect())
}

fn render(paths: &[PathBuf], cwd: &Path) -> String {
    if paths.is_empty() {
        return "No files found".to_owned();
    }
    let shown: Vec<String> = paths
        .iter()
        .take(GLOB_MAX_RESULTS)
        .map(|p| display_path(cwd, p))
        .collect();
    let body = shown.join("\n");
    if paths.len() <= GLOB_MAX_RESULTS {
        return body;
    }
    format!(
        "{body}\n\n(Showing {} of {} paths, most recently modified first. Narrow pattern or path to see more.)",
        shown.len(),
        paths.len()
    )
}

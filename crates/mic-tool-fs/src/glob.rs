use std::path::{Path, PathBuf};
use std::time::SystemTime;

use ignore::overrides::OverrideBuilder;
use ignore::WalkBuilder;
use mic_message::ContentPart;
use mic_tool::{Tool, ToolContext, ToolError};
use schemars::JsonSchema;
use serde::Deserialize;

use crate::common::{blocking, display_path, resolve, Cancel};
use crate::limits::GLOB_MAX_RESULTS;

pub(crate) struct Glob;

/// 版本控制元数据目录，始终排除。
const VCS_DIRS: &[&str] = &[".git", ".svn", ".hg", ".bzr", ".jj", ".sl"];

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct Args {
    /// Glob pattern to match file paths against (e.g. "**/*.ts", "src/**/*.test.js"). A pattern with no "/" matches the basename at any depth, so "*" and "*.ts" both search the whole tree; include a separator to anchor the depth.
    pattern: String,
    /// Directory to search in. Defaults to the session workspace; a relative path resolves against it.
    path: Option<String>,
}

impl Tool for Glob {
    type Args = Args;

    fn name(&self) -> &str {
        "glob"
    }

    fn description(&self) -> &str {
        "Find files whose paths match a glob pattern. Returns matching file paths — never directories — including hidden and ignored files (VCS metadata directories are excluded). Up to 100 paths come back, most recently modified first; a larger result returns the 100 most recently modified paths and says how many matched in total. This tool does not enumerate directory entries."
    }

    fn prompt_hint(&self) -> Option<&str> {
        Some("Find files with glob, not shell find. A pattern without \"/\" matches basenames at any depth, so \"*\" matches the whole tree. Results are files only (hidden and ignored included), newest first; oversized results keep the newest.")
    }

    async fn execute(&self, args: Args, ctx: &ToolContext) -> Result<Vec<ContentPart>, ToolError> {
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
        Ok(vec![ContentPart::Text { text }])
    }
}

fn find(
    root: &Path,
    pattern: &str,
    cwd: &Path,
    cancel: &Cancel,
) -> Result<Vec<PathBuf>, ToolError> {
    if !root.is_dir() {
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
    for entry in walk {
        if cancel.is_cancelled() {
            break;
        }
        // 无权限等不可读的条目跳过，与 rg 一致。
        let Ok(entry) = entry else { continue };
        if !entry.file_type().is_some_and(|t| t.is_file()) {
            continue;
        }
        let modified = entry
            .metadata()
            .ok()
            .and_then(|m| m.modified().ok())
            .unwrap_or(SystemTime::UNIX_EPOCH);
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

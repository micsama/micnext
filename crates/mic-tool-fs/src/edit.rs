use mic_message::ContentPart;
use mic_tool::{Tool, ToolContext, ToolError};
use schemars::JsonSchema;
use serde::Deserialize;

use crate::common::{atomic_write, blocking, decode, display_path, io_error, resolve};

pub(crate) struct Edit;

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct Args {
    /// Path to edit; a relative path resolves against the session workspace.
    file_path: String,
    /// Literal text to replace. Must match exactly.
    old_string: String,
    /// Literal replacement text. Use an empty string to delete the match.
    new_string: String,
    /// Replace all matches. Defaults to false; when false, old_string must appear exactly once.
    replace_all: Option<bool>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Replace {
    Once,
    All,
}

impl Tool for Edit {
    type Args = Args;

    fn name(&self) -> &str {
        "edit"
    }

    fn description(&self) -> &str {
        "Edit an existing UTF-8 text file by replacing literal text."
    }

    fn prompt_hint(&self) -> Option<&str> {
        Some("edit replaces literal old_string with new_string in a UTF-8 text file; old_string must be unique unless replace_all is true. Read the file first unless you just wrote or edited it.")
    }

    async fn execute(&self, args: Args, ctx: &ToolContext) -> Result<Vec<ContentPart>, ToolError> {
        let path = resolve(ctx, &args.file_path)?;
        if args.old_string.is_empty() {
            return Err(ToolError::input("old_string must be a non-empty string"));
        }
        if args.old_string == args.new_string {
            return Err(ToolError::input("old_string and new_string must differ"));
        }
        let replace = if args.replace_all == Some(true) {
            Replace::All
        } else {
            Replace::Once
        };
        let display = display_path(ctx.cwd(), &path);
        let text = blocking(move |_| {
            let io = |e| io_error("edit", &display, e);
            if std::fs::metadata(&path).map_err(io)?.is_dir() {
                return Err(ToolError::business(format!(
                    "cannot edit \"{display}\": is a directory"
                )));
            }
            let original = decode(std::fs::read(&path).map_err(io)?, "edit", &display)?;
            let crlf = is_crlf_dominant(&original);
            let content = original.replace("\r\n", "\n");
            let old = args.old_string.replace("\r\n", "\n");
            let new = args.new_string.replace("\r\n", "\n");

            let matches = content.matches(old.as_str()).count();
            if matches == 0 {
                return Err(ToolError::input(format!(
                    "old_string was not found in \"{display}\""
                )));
            }
            if replace == Replace::Once && matches > 1 {
                return Err(ToolError::input(format!(
                    "old_string matched {matches} times in \"{display}\"; provide a more specific old_string or set replace_all to true"
                )));
            }
            let updated = content.replace(old.as_str(), &new);
            let updated = if crlf {
                updated.replace('\n', "\r\n")
            } else {
                updated
            };
            atomic_write(&path, updated.as_bytes()).map_err(io)?;
            Ok(match replace {
                Replace::Once => format!("The file {display} has been updated successfully."),
                Replace::All => format!(
                    "The file {display} has been updated. All occurrences were successfully replaced."
                ),
            })
        })
        .await?;
        Ok(vec![ContentPart::Text { text }])
    }
}

/// CRLF 行尾多于单独 LF 行尾。
fn is_crlf_dominant(text: &str) -> bool {
    let crlf = text.matches("\r\n").count();
    crlf > text.matches('\n').count() - crlf
}

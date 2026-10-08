use mic_message::ContentPart;
use mic_tool::{Tool, ToolContext, ToolError};
use schemars::JsonSchema;
use serde::Deserialize;

use crate::common::{atomic_write, blocking, display_path, io_error, resolve};

pub(crate) struct Write;

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct Args {
    /// Path to write; a relative path resolves against the session workspace.
    file_path: String,
    /// Full UTF-8 text content to write.
    content: String,
}

impl Tool for Write {
    type Args = Args;

    fn name(&self) -> &str {
        "write"
    }

    fn description(&self) -> &str {
        "Create or fully replace a UTF-8 text file."
    }

    fn prompt_hint(&self) -> Option<&str> {
        Some("write creates a file or fully overwrites it: read an existing file first, and prefer edit for partial changes.")
    }

    async fn execute(&self, args: Args, ctx: &ToolContext) -> Result<Vec<ContentPart>, ToolError> {
        let path = resolve(ctx, &args.file_path)?;
        let display = display_path(ctx.cwd(), &path);
        let text = blocking(move |_| {
            let existed = match std::fs::metadata(&path) {
                Ok(meta) if meta.is_dir() => {
                    return Err(ToolError::business(format!(
                        "cannot write \"{display}\": is a directory"
                    )))
                }
                Ok(_) => true,
                Err(_) => false,
            };
            atomic_write(&path, args.content.as_bytes())
                .map_err(|e| io_error("write", &display, e))?;
            let outcome = if existed { "Updated" } else { "Created" };
            Ok(format!(
                "<path>{display}</path>\n<type>file</type>\n<content>\n{outcome} file\n</content>"
            ))
        })
        .await?;
        Ok(vec![ContentPart::Text { text }])
    }
}

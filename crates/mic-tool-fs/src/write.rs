use mic_message::ContentPart;
use mic_tool::{Tool, ToolContext, ToolError, ToolOutput};
use schemars::JsonSchema;
use serde::Deserialize;

use crate::common::{atomic_write, display_path, io_error, resolve};

pub(crate) struct Write;

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct Args {
    /// Absolute, or relative to the working directory.
    file_path: String,
    content: String,
}

#[cfg(test)]
mod tests;

impl Tool for Write {
    type Args = Args;

    fn name(&self) -> &str {
        "write"
    }

    fn description(&self) -> &str {
        "Create or overwrite a UTF-8 file; missing parent directories are created."
    }

    async fn execute(&self, args: Args, ctx: &ToolContext) -> Result<ToolOutput, ToolError> {
        let path = resolve(ctx, &args.file_path)?;
        let display = display_path(ctx.cwd(), &path);
        let text = ctx
            .commit(move || {
                let existed = match std::fs::metadata(&path) {
                    Ok(meta) if meta.is_dir() => {
                        return Err(ToolError::business(format!(
                            "cannot write \"{display}\": is a directory"
                        )))
                    }
                    Ok(_) => true,
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
                    Err(error) => return Err(io_error("stat before write", &display, error)),
                };
                atomic_write(&path, args.content.as_bytes())
                    .map_err(|e| io_error("write", &display, e))?;
                let outcome = if existed { "Updated" } else { "Created" };
                Ok(format!(
                "<path>{display}</path>\n<type>file</type>\n<content>\n{outcome} file\n</content>"
            ))
            })
            .await;
        let Some(text) = text else {
            return Ok(ToolOutput::Terminal(mic_message::ExecOutcome::Cancelled {
                message: "Stopped before file changes began.".into(),
            }));
        };
        let text = text?;
        Ok(ToolOutput::completed(vec![ContentPart::Text { text }]))
    }
}

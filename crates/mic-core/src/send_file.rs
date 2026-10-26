//! 交给用户的文件：调用时复制快照到 `<files>/<person>/<日期>/`。`send_file` 发到当前会话，push 附带同一种快照。
//! 契约：docs/blueprints/send-file.md。

use std::any::Any;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use mic_message::{ContentPart, FileRef, MessageBody, PersonId};
use mic_store::Store;
use mic_tool::{Tool, ToolContext, ToolError, ToolHandle, ToolOutput};
use schemars::JsonSchema;
use serde::Deserialize;

use crate::event::Events;
use crate::input::{file_name, person_dir, save, SavedFiles, Source};
use crate::jobs::JobPort;
use crate::kernel::session_channel;
use crate::limits::{MAX_SEND_FILES, MAX_SEND_FILE_BYTES};
use crate::run::now_ms;

pub(crate) const SEND_FILE: &str = "send_file";

/// 校验全部路径后逐个复制；任一不合格整次不交付。返回的快照未 `keep` 即删除。
pub(crate) async fn snapshot(
    files: &Path,
    person: PersonId,
    cwd: &Path,
    paths: &[String],
) -> Result<(Vec<FileRef>, SavedFiles), ToolError> {
    if paths.is_empty() || paths.len() > MAX_SEND_FILES {
        return Err(ToolError::input(format!(
            "files must list 1–{MAX_SEND_FILES} paths"
        )));
    }
    let mut sources = Vec::with_capacity(paths.len());
    for raw in paths {
        let path = cwd.join(raw);
        let meta = tokio::fs::metadata(&path)
            .await
            .map_err(|e| match e.kind() {
                ErrorKind::NotFound => ToolError::input(format!("{raw}: no such file")),
                _ => ToolError::dependency(format!("{raw}: {e}")),
            })?;
        if !meta.is_file() {
            return Err(ToolError::input(format!("{raw}: not a regular file")));
        }
        if meta.len() > MAX_SEND_FILE_BYTES {
            return Err(ToolError::input(format!(
                "{raw}: {} bytes exceeds the {MAX_SEND_FILE_BYTES}-byte limit",
                meta.len()
            )));
        }
        sources.push(path);
    }
    let dir = person_dir(files, person);
    let mut saved = SavedFiles::new();
    let mut refs = Vec::with_capacity(sources.len());
    for path in &sources {
        let name = file_name(path.file_name().and_then(|n| n.to_str()).unwrap_or(""));
        let copy: PathBuf = save(&dir, name, Source::Path(path))
            .await
            .map_err(|e| ToolError::dependency(format!("{}: {e}", path.display())))?;
        saved.push(copy.clone());
        let size_bytes = tokio::fs::metadata(&copy)
            .await
            .map_err(|e| ToolError::dependency(format!("{}: {e}", copy.display())))?
            .len();
        refs.push(FileRef {
            path: copy.to_string_lossy().into_owned(),
            size_bytes,
        });
    }
    Ok((refs, saved))
}

/// 工具结果：模型凭此知道交付了哪些快照。
pub(crate) fn sent_text(files: &[FileRef]) -> String {
    files
        .iter()
        .map(|f| format!("Sent to the user: {} ({} bytes)", f.path, f.size_bytes))
        .collect::<Vec<_>>()
        .join("\n")
}

pub(crate) fn handle(store: Store, events: Events, files: PathBuf, owner: PersonId) -> ToolHandle {
    ToolHandle::new(SendFileTool {
        store,
        events,
        files,
        owner,
    })
}

struct SendFileTool {
    store: Store,
    events: Events,
    files: PathBuf,
    owner: PersonId,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct SendFileArgs {
    /// Paths of existing files, relative to the working directory or absolute.
    files: Vec<String>,
}

impl Tool for SendFileTool {
    type Args = SendFileArgs;

    fn name(&self) -> &str {
        SEND_FILE
    }

    fn description(&self) -> &str {
        "Give files to the user in this conversation: they appear as attachments the user can preview or download, and IM channels send them as media. A snapshot is taken now, so later edits do not change what was sent. Files you only write to disk are not visible to the user; call this when the file itself is the deliverable. If the user is not watching this conversation (e.g. a scheduled task), attach files to push instead."
    }

    async fn execute(
        &self,
        args: SendFileArgs,
        ctx: &ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        let port = (ctx.background() as &dyn Any)
            .downcast_ref::<JobPort>()
            .ok_or_else(|| ToolError::business("send_file is not available here"))?;
        let origin = port.origin();
        let (files, saved) = snapshot(&self.files, self.owner, ctx.cwd(), &args.files).await?;
        let dependency = |e: mic_store::StoreError| ToolError::dependency(e.to_string());
        let session = self
            .store
            .session(origin.session)
            .await
            .map_err(dependency)?
            .expect("调用来源会话存在");
        let channel = session_channel(&self.store, &session)
            .await
            .map_err(dependency)?;
        let text = sent_text(&files);
        let publish = self.events.publisher().await;
        let message = self
            .store
            .append(
                origin.session,
                Some(origin.run),
                MessageBody::Attachment { files },
                now_ms(),
            )
            .await
            .map_err(dependency)?;
        saved.keep();
        publish.appended(origin.session, &channel, message);
        Ok(ToolOutput::completed(vec![ContentPart::Text { text }]))
    }
}

#[cfg(test)]
mod tests;

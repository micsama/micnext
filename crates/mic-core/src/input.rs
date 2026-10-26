//! 入站用户输入：校验并把原始字节变成已验证的图片原件与已落盘的文件。

use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use mic_media::MediaError;
use mic_message::limits::MAX_IMAGES_PER_INPUT;
use mic_message::{FileRef, PersonId};
use mic_store::NewInputPart;
use tokio::io::AsyncWriteExt;

/// 入站文件根目录，相对 data_dir。
pub(crate) const FILES_DIR: &str = "files";

/// Channel / Gateway 交给内核的原始片段；图片字节尚未验证，文件数量与大小由渠道边界限定。
pub enum IncomingPart {
    Text(String),
    Image(Vec<u8>),
    /// `name` 是上游给的原始文件名，内核只取最后一段。
    File {
        name: String,
        bytes: Vec<u8>,
    },
}

/// 输入被整条拒收，不落任何消息、图片或文件；文案面向用户。
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum InputError {
    #[error("消息内容为空")]
    Empty,
    #[error("一条消息最多带 {MAX_IMAGES_PER_INPUT} 张图片")]
    ImageLimit,
    #[error("{0}")]
    Image(MediaError),
    #[error("文件保存失败，请重发")]
    FileSave,
}

/// 已校验的片段与本次落盘的文件；输入事务提交后 `keep`，否则丢弃时删除文件。
pub(crate) struct Validated {
    pub(crate) parts: Vec<NewInputPart>,
    pub(crate) files: SavedFiles,
}

/// 纯文本去空白后为空且无附件则拒；图片经 `mic_media::normalize` 规整；
/// 文件存入 `<files>/<person>/<本机日期>/`。
pub(crate) async fn validate(
    parts: Vec<IncomingPart>,
    files: &Path,
    person: PersonId,
) -> Result<Validated, InputError> {
    let mut has_content = false;
    let mut images = 0;
    let mut saved = SavedFiles(Vec::new());
    let mut out = Vec::with_capacity(parts.len());
    for part in parts {
        out.push(match part {
            IncomingPart::Text(text) => {
                has_content |= !text.trim().is_empty();
                NewInputPart::Text(text)
            }
            IncomingPart::Image(bytes) => {
                images += 1;
                if images > MAX_IMAGES_PER_INPUT {
                    return Err(InputError::ImageLimit);
                }
                let image = tokio::task::spawn_blocking(move || mic_media::normalize(bytes))
                    .await
                    .expect("图片规整不 panic")
                    .map_err(InputError::Image)?;
                has_content = true;
                NewInputPart::Image(image)
            }
            IncomingPart::File { name, bytes } => {
                let dir = person_dir(files, person);
                let path = save(&dir, file_name(&name), Source::Bytes(&bytes)).await.map_err(|error| {
                    tracing::warn!(error = %crate::error::chain(&error), dir = %dir.display(), "input file save failed");
                    InputError::FileSave
                })?;
                has_content = true;
                let file = FileRef {
                    path: path.to_string_lossy().into_owned(),
                    size_bytes: bytes.len() as u64,
                };
                saved.0.push(path);
                NewInputPart::File(file)
            }
        });
    }
    if !has_content {
        return Err(InputError::Empty);
    }
    Ok(Validated {
        parts: out,
        files: saved,
    })
}

/// 本次落盘的文件；未 `keep` 即删除。
pub(crate) struct SavedFiles(Vec<PathBuf>);

impl SavedFiles {
    pub(crate) fn new() -> Self {
        Self(Vec::new())
    }

    pub(crate) fn push(&mut self, path: PathBuf) {
        self.0.push(path);
    }

    pub(crate) fn keep(mut self) {
        self.0.clear();
    }
}

impl Drop for SavedFiles {
    fn drop(&mut self) {
        for path in &self.0 {
            remove(path);
        }
    }
}

/// 取最后一段，去掉控制字符；空、`.`、`..` 用 `文件`。
pub(crate) fn file_name(raw: &str) -> &str {
    match raw.rsplit(['/', '\\']).next().unwrap_or("").trim() {
        "" | "." | ".." => "文件",
        name if name.chars().any(char::is_control) => "文件",
        name => name,
    }
}

/// 某人的文件按本机日期分目录：`<files>/<person>/<日期>/`。
pub(crate) fn person_dir(files: &Path, person: PersonId) -> PathBuf {
    files
        .join(person.0.to_string())
        .join(chrono::Local::now().format("%Y-%m-%d").to_string())
}

/// 写入内容：入站字节，或交付时复制的本地文件。
pub(crate) enum Source<'a> {
    Bytes(&'a [u8]),
    Path(&'a Path),
}

/// 独占创建最终名（同名改 `名字 (n).扩展名`）后直接写入；失败或取消删掉本次创建的文件。
pub(crate) async fn save(dir: &Path, name: &str, source: Source<'_>) -> std::io::Result<PathBuf> {
    tokio::fs::create_dir_all(dir).await?;
    let dir = tokio::fs::canonicalize(dir).await?;
    let (stem, ext) = split_name(name);
    let mut n = 1u32;
    let (path, mut file) = loop {
        let candidate = match (n, ext) {
            (1, _) => dir.join(name),
            (_, Some(ext)) => dir.join(format!("{stem} ({n}).{ext}")),
            (_, None) => dir.join(format!("{stem} ({n})")),
        };
        match tokio::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
            .await
        {
            Ok(file) => break (candidate, file),
            Err(error) if error.kind() == ErrorKind::AlreadyExists => n += 1,
            Err(error) => return Err(error),
        }
    };
    let guard = SavedFiles(vec![path.clone()]);
    match source {
        Source::Bytes(bytes) => file.write_all(bytes).await?,
        Source::Path(from) => {
            tokio::io::copy(&mut tokio::fs::File::open(from).await?, &mut file).await?;
        }
    }
    file.sync_all().await?;
    guard.keep();
    Ok(path)
}

/// `报告.pdf` → (`报告`, `pdf`)；`.bashrc`、无扩展名不拆。
fn split_name(name: &str) -> (&str, Option<&str>) {
    match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() && !ext.is_empty() => (stem, Some(ext)),
        _ => (name, None),
    }
}

fn remove(path: &Path) {
    if let Err(error) = std::fs::remove_file(path) {
        if error.kind() != ErrorKind::NotFound {
            tracing::warn!(%error, path = %path.display(), "saved file cleanup failed");
        }
    }
}

#[cfg(test)]
mod tests;

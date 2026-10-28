//! Skill：`<data_dir>/skills/<name>/SKILL.md` 的扫描、校验与加载。契约：docs/blueprints/skills.md。

use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use mic_message::ContentPart;
use mic_tool::{Tool, ToolContext, ToolError, ToolHandle, ToolOutput};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::limits::{SKILL_DESCRIPTION_CHARS, SKILL_FILES_LISTED};

pub(crate) const SKILL: &str = "skill";
/// skill 目录名，相对 data_dir。
pub(crate) const SKILLS_DIR: &str = "skills";
const SKILL_FILE: &str = "SKILL.md";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SkillInfo {
    pub name: String,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct InvalidSkill {
    pub dir: String,
    pub reason: String,
}

/// 均按名称排序，文件未变时目录文本不变。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct SkillList {
    pub skills: Vec<SkillInfo>,
    pub invalid: Vec<InvalidSkill>,
}

/// 读 skill 目录的 I/O 失败；`skills/` 不存在不算。
#[derive(Debug, thiserror::Error)]
#[error("读取 skill 目录 {path} 失败：{source}")]
pub struct SkillsError {
    path: String,
    source: std::io::Error,
}

fn io_error(path: &Path, source: std::io::Error) -> SkillsError {
    SkillsError {
        path: path.display().to_string(),
        source,
    }
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum LoadError {
    #[error("unknown skill `{name}`")]
    NotFound { name: String },
    #[error("skill `{name}` is invalid: {reason}")]
    Invalid { name: String, reason: String },
    #[error(transparent)]
    Io(#[from] SkillsError),
}

/// `dir` 是绝对路径（data_dir 启动时已校验为绝对）。
#[derive(Debug, Clone)]
pub(crate) struct Skills {
    dir: PathBuf,
}

impl Skills {
    pub(crate) fn new(dir: PathBuf) -> Self {
        Self { dir }
    }

    pub(crate) async fn list(&self) -> Result<SkillList, SkillsError> {
        let dir = self.dir.clone();
        tokio::task::spawn_blocking(move || scan(&dir))
            .await
            .expect("扫描 skill 不 panic")
    }

    /// 工具加载与用户点名共用的加载结果。
    pub(crate) async fn load(&self, name: &str) -> Result<String, LoadError> {
        let dir = self.dir.clone();
        let name = name.to_owned();
        tokio::task::spawn_blocking(move || load(&dir, &name))
            .await
            .expect("加载 skill 不 panic")
    }

    /// `/name 其余文字` 且 name 是 skill 时返回点名结果；不是 skill 返回 `None`，按普通文字处理。
    pub(crate) async fn expand(&self, text: &str) -> Option<Named> {
        let rest = text.trim_start().strip_prefix('/')?;
        let split = rest.find(char::is_whitespace).unwrap_or(rest.len());
        let (name, rest) = rest.split_at(split);
        if !valid_name(name) {
            return None;
        }
        let name = name.to_owned();
        match self.load(&name).await {
            Ok(text) => Some(Named::Loaded {
                name,
                text,
                rest: rest.trim().to_owned(),
            }),
            Err(LoadError::NotFound { .. }) => None,
            Err(e) => Some(Named::Failed {
                reason: e.to_string(),
                name,
            }),
        }
    }

    /// system prompt 里的 Skill 目录；没有 skill 时 `None`。
    pub(crate) fn catalog(&self, list: &SkillList) -> Option<String> {
        if list.skills.is_empty() {
            return None;
        }
        let lines: Vec<String> = list
            .skills
            .iter()
            .map(|s| {
                format!(
                    "<skill name=\"{}\" dir=\"{}\">{}</skill>",
                    s.name,
                    self.dir.join(&s.name).display(),
                    s.description
                )
            })
            .collect();
        Some(format!(
            "Skills: load one with the `{SKILL}` tool before a task that matches its description. Relative paths in a skill resolve against its dir.\n<skills>\n{}\n</skills>",
            lines.join("\n")
        ))
    }
}

/// 用户点名的结果：加载成功，或 skill 存在但读不出（原因交给 agent）。
pub(crate) enum Named {
    Loaded {
        name: String,
        text: String,
        rest: String,
    },
    Failed {
        name: String,
        reason: String,
    },
}

fn scan(root: &Path) -> Result<SkillList, SkillsError> {
    let entries = match std::fs::read_dir(root) {
        Ok(entries) => entries,
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(SkillList::default()),
        Err(e) => return Err(io_error(root, e)),
    };
    let mut dirs = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|e| io_error(root, e))?;
        let path = entry.path();
        let is_dir = std::fs::metadata(&path)
            .map_err(|e| io_error(&path, e))?
            .is_dir();
        let name = entry.file_name().to_string_lossy().into_owned();
        if is_dir && !name.starts_with('.') {
            dirs.push(name);
        }
    }
    dirs.sort();
    let mut list = SkillList::default();
    for dir in dirs {
        match read(root, &dir)? {
            Ok((meta, _)) => list.skills.push(meta),
            Err(reason) => list.invalid.push(InvalidSkill { dir, reason }),
        }
    }
    Ok(list)
}

/// 外层错误是 I/O；内层 `Err` 是给用户看的不合法原因。
fn read(root: &Path, dir: &str) -> Result<Result<(SkillInfo, String), String>, SkillsError> {
    let path = root.join(dir).join(SKILL_FILE);
    let raw = match std::fs::read_to_string(&path) {
        Ok(raw) => raw,
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(Err(format!("缺少 {SKILL_FILE}"))),
        Err(e) if e.kind() == ErrorKind::InvalidData => {
            return Ok(Err(format!("{SKILL_FILE} 不是 UTF-8 文本")))
        }
        Err(e) => return Err(io_error(&path, e)),
    };
    Ok(parse(dir, &raw))
}

#[derive(Deserialize)]
struct Frontmatter {
    name: Option<String>,
    description: Option<String>,
}

fn parse(dir: &str, raw: &str) -> Result<(SkillInfo, String), String> {
    let raw = raw.strip_prefix('\u{feff}').unwrap_or(raw);
    let rest = raw
        .strip_prefix("---")
        .and_then(|r| r.strip_prefix('\n').or_else(|| r.strip_prefix("\r\n")))
        .ok_or("开头缺少 --- frontmatter")?;
    let (yaml, body) = split_frontmatter(rest).ok_or("frontmatter 没有以 --- 结束")?;
    let fm: Frontmatter =
        serde_yaml_ng::from_str(yaml).map_err(|e| format!("frontmatter 解析失败：{e}"))?;
    let name = fm.name.ok_or("frontmatter 缺少 name")?;
    if name != dir {
        return Err(format!("name「{name}」与目录名不一致"));
    }
    if !valid_name(&name) {
        return Err(
            "name 只能用小写字母、数字和单个连字符，1–64 个字符，不以连字符开头或结尾".into(),
        );
    }
    if crate::session_commands().iter().any(|c| c.name == name) {
        return Err(format!("name「{name}」与内置指令重名"));
    }
    let description = fm
        .description
        .map(|d| d.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|d| !d.is_empty())
        .ok_or("frontmatter 缺少 description")?;
    if description.chars().count() > SKILL_DESCRIPTION_CHARS {
        return Err(format!("description 超过 {SKILL_DESCRIPTION_CHARS} 字符"));
    }
    Ok((SkillInfo { name, description }, body.trim().to_owned()))
}

/// frontmatter 结束于独占一行的 `---`。
fn split_frontmatter(rest: &str) -> Option<(&str, &str)> {
    let mut at = 0;
    for line in rest.split_inclusive('\n') {
        if line.trim_end() == "---" {
            return Some((&rest[..at], &rest[at + line.len()..]));
        }
        at += line.len();
    }
    None
}

fn valid_name(name: &str) -> bool {
    (1..=64).contains(&name.len())
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        && !name.starts_with('-')
        && !name.ends_with('-')
        && !name.contains("--")
}

fn load(root: &Path, name: &str) -> Result<String, LoadError> {
    if !valid_name(name) || !root.join(name).is_dir() {
        return Err(LoadError::NotFound { name: name.into() });
    }
    let (_, body) = read(root, name)?.map_err(|reason| LoadError::Invalid {
        name: name.into(),
        reason,
    })?;
    let dir = root.join(name);
    let dir = std::fs::canonicalize(&dir).map_err(|e| io_error(&dir, e))?;
    let mut files = Vec::new();
    walk(&dir, &dir, &mut files)?;
    files.retain(|f| f != SKILL_FILE);
    files.sort();
    let mut out = format!("{body}\n\nSkill directory: {}", dir.display());
    if !files.is_empty() {
        out.push_str("\nFiles (relative to the skill directory):");
        for f in files.iter().take(SKILL_FILES_LISTED) {
            out.push_str(&format!("\n- {f}"));
        }
        if files.len() > SKILL_FILES_LISTED {
            out.push_str(&format!(
                "\n- … {} more not listed",
                files.len() - SKILL_FILES_LISTED
            ));
        }
    }
    Ok(out)
}

/// 递归列出普通文件的相对路径，跳过以 `.` 开头的项。
fn walk(root: &Path, dir: &Path, out: &mut Vec<String>) -> Result<(), SkillsError> {
    for entry in std::fs::read_dir(dir).map_err(|e| io_error(dir, e))? {
        let entry = entry.map_err(|e| io_error(dir, e))?;
        if entry.file_name().to_string_lossy().starts_with('.') {
            continue;
        }
        let path = entry.path();
        let kind = entry.file_type().map_err(|e| io_error(&path, e))?;
        if kind.is_dir() {
            walk(root, &path, out)?;
        } else if kind.is_file() {
            let rel = path.strip_prefix(root).expect("在根目录之下");
            out.push(rel.to_string_lossy().into_owned());
        }
    }
    Ok(())
}

pub(crate) fn handle(skills: Skills) -> ToolHandle {
    ToolHandle::new(SkillTool { skills })
}

struct SkillTool {
    skills: Skills,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct SkillArgs {
    /// Skill name as listed under Skills in the system prompt.
    name: String,
}

impl Tool for SkillTool {
    type Args = SkillArgs;

    fn name(&self) -> &str {
        SKILL
    }

    fn description(&self) -> &str {
        "Load a skill's instructions by name; available skills are listed under Skills in the system prompt. Load the matching skill before starting a task it describes.\n\
         A loaded skill stays visible in the conversation, so don't load it again; reload only when its content is no longer in your context (replaced by a placeholder or folded into the summary). Relative paths in a skill resolve against its Skill directory."
    }

    async fn execute(&self, args: SkillArgs, _: &ToolContext) -> Result<ToolOutput, ToolError> {
        match self.skills.load(args.name.trim()).await {
            Ok(text) => Ok(ToolOutput::completed(vec![ContentPart::Text { text }])),
            Err(LoadError::Io(e)) => Err(ToolError::dependency(e.to_string())),
            Err(e) => {
                let available = match self.skills.list().await {
                    Ok(list) => list
                        .skills
                        .iter()
                        .map(|s| s.name.as_str())
                        .collect::<Vec<_>>()
                        .join(", "),
                    Err(io) => return Err(ToolError::dependency(io.to_string())),
                };
                Err(ToolError::input(format!(
                    "{e}; available: {}",
                    if available.is_empty() {
                        "none"
                    } else {
                        &available
                    }
                )))
            }
        }
    }
}

#[cfg(test)]
mod tests;

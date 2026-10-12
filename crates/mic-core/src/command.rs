use mic_message::SessionCommand;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct CommandSpec {
    pub name: &'static str,
    pub usage: &'static str,
    pub description: &'static str,
}

#[derive(Debug, thiserror::Error)]
pub enum CommandParseError {
    #[error("指令 {command} 不接受参数")]
    UnexpectedArguments { command: String },
}

pub fn session_commands() -> &'static [CommandSpec] {
    &[
        CommandSpec {
            name: "compact",
            usage: "/compact [文字]",
            description: "压缩历史，再处理附带文字",
        },
        CommandSpec {
            name: "clear",
            usage: "/clear",
            description: "清空模型上下文，保留对话记录",
        },
    ]
}

pub fn parse_session_command(text: &str) -> Result<Option<SessionCommand>, CommandParseError> {
    let text = text.trim();
    let split = text.find(char::is_whitespace).unwrap_or(text.len());
    let (name, rest) = text.split_at(split);
    let rest = rest.trim();
    match name {
        "/compact" => Ok(Some(SessionCommand::Compact {
            text: (!rest.is_empty()).then(|| rest.to_owned()),
        })),
        "/clear" if rest.is_empty() => Ok(Some(SessionCommand::Clear)),
        "/clear" => Err(CommandParseError::UnexpectedArguments {
            command: name.into(),
        }),
        _ => Ok(None),
    }
}

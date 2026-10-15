use mic_message::SessionCommand;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct CommandSpec {
    pub name: &'static str,
    pub usage: &'static str,
    pub description: &'static str,
}

/// `Session` 入队成为 run；`Stop` 立即作用于执行中的 run，不入队。
#[derive(Debug, Clone, PartialEq)]
pub enum ParsedCommand {
    Session(SessionCommand),
    Stop,
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
        CommandSpec {
            name: "stop",
            usage: "/stop",
            description: "停止当前回答",
        },
    ]
}

pub fn parse_command(text: &str) -> Result<Option<ParsedCommand>, CommandParseError> {
    let text = text.trim();
    let split = text.find(char::is_whitespace).unwrap_or(text.len());
    let (name, rest) = text.split_at(split);
    let rest = rest.trim();
    let bare = |command| {
        if rest.is_empty() {
            Ok(Some(command))
        } else {
            Err(CommandParseError::UnexpectedArguments {
                command: name.into(),
            })
        }
    };
    match name {
        "/compact" => Ok(Some(ParsedCommand::Session(SessionCommand::Compact {
            text: (!rest.is_empty()).then(|| rest.to_owned()),
        }))),
        "/clear" => bare(ParsedCommand::Session(SessionCommand::Clear)),
        "/stop" => bare(ParsedCommand::Stop),
        _ => Ok(None),
    }
}

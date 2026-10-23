//! 模型出站脱敏：只改每次请求的副本，本地原文不动。精确匹配应用自己持有的秘密（始终开启），
//! 正则遮有明显特征的凭据（会话可对新消息临时关闭，见 `ModelRequest::plain_after`）。
//! 编码、拆分、图片与工具直接外传不在范围内。

use std::sync::{Arc, LazyLock};

use base64::{engine::general_purpose::STANDARD, Engine};
use mic_message::{
    Compaction, ContentPart, ContextBoundary, ExecOutcome, MessageBody, Reasoning, ReplyBlock,
    SessionCommand, ToolResultOutcome,
};
use mic_store::SecretValue;
use regex::{Captures, Regex};

use crate::provider::{BoxStream, ModelEvent, ModelRequest, Provider, ProviderError};

/// 每条规则的 `s` 组是要遮的部分；`true` = 值须含数字才遮。
static RULES: LazyLock<Vec<(Regex, bool)>> = LazyLock::new(|| {
    [
        // 服务商 token 前缀
        (r"\b(?P<s>(?:sk-|sk_live_|rk_live_|ghp_|gho_|ghu_|ghs_|ghr_|github_pat_|glpat-|xox[abprs]-|hf_|gsk_|AKIA|AIza)[A-Za-z0-9_\-]{16,})", false),
        // JWT
        (r"\b(?P<s>eyJ[A-Za-z0-9_\-]{8,}\.eyJ[A-Za-z0-9_\-]{8,}\.[A-Za-z0-9_\-]{8,})", false),
        (r"(?i)\bbearer\s+(?P<s>[A-Za-z0-9._~+/=\-]{8,})", false),
        // 凭据字段赋值：`api_key=…`、`"password": "…"`；不含数字的多是代码表达式（`config.api_key`）
        (r#"(?i)\b[\w-]*(?:api[_-]?key|apikey|secret|token|password|passwd|pwd)["']?\s*[:=]\s*["']?(?P<s>[^\s"'`,;]{8,})"#, true),
        // 连接串里的密码
        (r"://[^/\s:@]+:(?P<s>[^/\s@]{3,})@", false),
        (r"(?s)-----BEGIN [A-Z ]*PRIVATE KEY-----(?P<s>.+?)-----END [A-Z ]*PRIVATE KEY-----", false),
    ]
    .into_iter()
    .map(|(r, needs_digit)| (Regex::new(r).expect("脱敏规则是合法正则"), needs_digit))
    .collect()
});

/// 主密钥按工具读出时可能出现的文本形式（hex、base64）登记。
#[derive(Default)]
pub(crate) struct Redactor {
    master_key: Vec<SecretValue>,
}

impl Redactor {
    pub(crate) fn new(master_key: &[u8]) -> Self {
        let hex = master_key.iter().map(|b| format!("{b:02x}")).collect();
        Self {
            master_key: vec![
                SecretValue::new(hex),
                SecretValue::new(STANDARD.encode(master_key)),
            ],
        }
    }

    /// 给模型实例包上出站脱敏；`own_key` 是该实例自己的 API key。
    pub(crate) fn wrap(
        self: &Arc<Self>,
        inner: Arc<dyn Provider>,
        own_key: Option<SecretValue>,
    ) -> Arc<dyn Provider> {
        Arc::new(Redacted {
            inner,
            redactor: self.clone(),
            own_key,
        })
    }
}

struct Redacted {
    inner: Arc<dyn Provider>,
    redactor: Arc<Redactor>,
    own_key: Option<SecretValue>,
}

impl Provider for Redacted {
    fn model(&self) -> &str {
        self.inner.model()
    }

    fn stream(&self, mut req: ModelRequest) -> BoxStream<Result<ModelEvent, ProviderError>> {
        let known = self.redactor.master_key.iter().chain(&self.own_key);
        let known: Vec<&str> = known
            .map(SecretValue::expose)
            .filter(|k| !k.is_empty())
            .collect();
        request(&known, &mut req);
        self.inner.stream(req)
    }
}

struct Scrub<'a> {
    known: &'a [&'a str],
    rules: bool,
}

/// 只改承载内容的字符串；协议 ID、角色、签名与上游密文推理原样保留。system 始终过正则：
/// 其中的摘要和原话可能早于明文水位。
fn request(known: &[&str], req: &mut ModelRequest) {
    Scrub { known, rules: true }.text(&mut req.system);
    for message in &mut req.messages {
        let rules = req.plain_after.is_none_or(|after| message.id <= after);
        Scrub { known, rules }.message(&mut message.body);
    }
}

impl Scrub<'_> {
    fn message(&self, body: &mut MessageBody) {
        match body {
            MessageBody::Command {
                command: SessionCommand::Compact { text: Some(text) },
                ..
            }
            | MessageBody::JobNotice { text, .. }
            | MessageBody::Trigger { text, .. }
            | MessageBody::HarnessNote { text }
            | MessageBody::Notification { text, .. }
            | MessageBody::SideOutput { text, .. }
            | MessageBody::Boundary {
                boundary: ContextBoundary::Compaction(Compaction { summary: text, .. }),
            } => self.text(text),
            MessageBody::UserInput { parts, .. } => self.parts(parts),
            MessageBody::Reply { blocks, .. } => {
                for block in blocks {
                    match block {
                        ReplyBlock::Reasoning(Reasoning::Visible { text, .. })
                        | ReplyBlock::Text { text, .. } => self.text(text),
                        ReplyBlock::ToolCall { args, .. } => self.json(args),
                        ReplyBlock::Reasoning(Reasoning::Redacted { .. }) => {}
                    }
                }
            }
            MessageBody::ToolResult {
                outcome: ToolResultOutcome::Terminal(outcome),
                ..
            }
            | MessageBody::Completion { outcome, .. } => match outcome {
                ExecOutcome::TimedOut { output } | ExecOutcome::Completed { output } => {
                    self.parts(output)
                }
                ExecOutcome::Failed { message, .. } | ExecOutcome::Cancelled { message } => {
                    self.text(message)
                }
            },
            MessageBody::Command { .. }
            | MessageBody::ToolResult { .. }
            | MessageBody::Boundary { .. } => {}
        }
    }

    fn parts(&self, parts: &mut [ContentPart]) {
        for part in parts {
            if let ContentPart::Text { text } = part {
                self.text(text);
            }
        }
    }

    fn json(&self, value: &mut serde_json::Value) {
        match value {
            serde_json::Value::String(s) => self.text(s),
            serde_json::Value::Array(items) => items.iter_mut().for_each(|v| self.json(v)),
            serde_json::Value::Object(map) => map.values_mut().for_each(|v| self.json(v)),
            _ => {}
        }
    }

    fn text(&self, s: &mut String) {
        for secret in self.known {
            if s.contains(secret) {
                *s = s.replace(secret, &mask(secret));
            }
        }
        if !self.rules {
            return;
        }
        for (rule, needs_digit) in RULES.iter() {
            let replaced = rule.replace_all(s, |c: &Captures| {
                let (all, value) = (c.get(0).expect("整段匹配"), c.name("s").expect("s 组"));
                if *needs_digit && !value.as_str().bytes().any(|b| b.is_ascii_digit()) {
                    return all.as_str().to_owned();
                }
                let text = all.as_str();
                let (start, end) = (value.start() - all.start(), value.end() - all.start());
                format!("{}{}{}", &text[..start], mask(value.as_str()), &text[end..])
            });
            if let std::borrow::Cow::Owned(replaced) = replaced {
                *s = replaced;
            }
        }
    }
}

/// 20 字符以上保留首尾各 4 位，更短的整段遮住。
fn mask(secret: &str) -> String {
    let chars: Vec<char> = secret.chars().collect();
    if chars.len() < 20 {
        return "***".to_owned();
    }
    let head: String = chars[..4].iter().collect();
    let tail: String = chars[chars.len() - 4..].iter().collect();
    format!("{head}***{tail}")
}

#[cfg(test)]
mod tests;

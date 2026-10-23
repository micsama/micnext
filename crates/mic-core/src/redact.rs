//! 模型出站脱敏：每次请求的副本里遮住应用自己持有的秘密与有明显特征的凭据。本地原文不动。
//! 只减少明文意外披露；编码、拆分、图片与工具直接外传不在范围内。

use std::borrow::Cow;
use std::sync::{Arc, LazyLock};

use base64::{engine::general_purpose::STANDARD, Engine};
use mic_message::{
    Compaction, ContentPart, ContextBoundary, ExecOutcome, MessageBody, Reasoning, ReplyBlock,
    SessionCommand, ToolResultOutcome,
};
use mic_store::SecretValue;
use regex::{Captures, Regex};

use crate::provider::{BoxStream, ModelEvent, ModelRequest, Provider, ProviderError};

/// 保留首尾的最短长度；更短的整段遮住。
const KEEP_EDGES_FROM: usize = 20;
const EDGE: usize = 4;
const MASK: &str = "***";

/// 每条规则的 `s` 组是要遮的部分；`true` = 值须含数字才遮。
static RULES: LazyLock<Vec<(Regex, bool)>> = LazyLock::new(|| {
    [
        // 服务商 token 前缀
        (r"\b(?P<s>(?:sk-|sk_live_|rk_live_|ghp_|gho_|ghu_|ghs_|ghr_|github_pat_|glpat-|xox[abprs]-|hf_|gsk_|AKIA|AIza)[A-Za-z0-9_\-]{16,})", false),
        // JWT
        (r"\b(?P<s>eyJ[A-Za-z0-9_\-]{8,}\.eyJ[A-Za-z0-9_\-]{8,}\.[A-Za-z0-9_\-]{8,})", false),
        (r"(?i)\bbearer\s+(?P<s>[A-Za-z0-9._~+/=\-]{8,})", false),
        // 凭据字段赋值：`api_key=…`、`"password": "…"`、`TOKEN: …`；不含数字的多是代码表达式（`config.api_key`）
        (r#"(?i)\b[\w-]*(?:api[_-]?key|apikey|secret|token|password|passwd|pwd)["']?\s*[:=]\s*["']?(?P<s>[^\s"'`,;]{8,})"#, true),
        // 连接串里的密码
        (r"://[^/\s:@]+:(?P<s>[^/\s@]{3,})@", false),
        (r"(?s)-----BEGIN [A-Z ]*PRIVATE KEY-----(?P<s>.+?)-----END [A-Z ]*PRIVATE KEY-----", false),
    ]
    .into_iter()
    .map(|(r, needs_digit)| (Regex::new(r).expect("脱敏规则是合法正则"), needs_digit))
    .collect()
});

/// 应用自己持有、无固定格式的秘密，精确匹配。
#[derive(Default)]
pub(crate) struct Redactor {
    known: Vec<SecretValue>,
}

impl Redactor {
    /// 主密钥是原始字节，按工具读出时可能出现的文本形式登记。
    pub(crate) fn new(master_key: &[u8]) -> Self {
        let hex: String = master_key.iter().map(|b| format!("{b:02x}")).collect();
        let mut forms = vec![hex.to_uppercase(), hex, STANDARD.encode(master_key)];
        forms.extend(std::str::from_utf8(master_key).ok().map(str::to_owned));
        Self {
            known: forms.into_iter().map(SecretValue::new).collect(),
        }
    }

    /// 给模型实例包上出站脱敏；`key` 是该实例自己的 API key。
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
        let known: Vec<&str> = self
            .redactor
            .known
            .iter()
            .chain(&self.own_key)
            .map(SecretValue::expose)
            .filter(|k| !k.is_empty())
            .collect();
        redact_request(&known, &mut req);
        self.inner.stream(req)
    }
}

/// 只改承载内容的字符串；协议 ID、角色、签名与上游密文推理原样保留。
fn redact_request(known: &[&str], req: &mut ModelRequest) {
    let r = |s: &mut String| redact_in_place(known, s);
    r(&mut req.system);
    for message in &mut req.messages {
        match &mut message.body {
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
            } => r(text),
            MessageBody::UserInput { parts, .. } => redact_parts(known, parts),
            MessageBody::Reply { blocks, .. } => {
                for block in blocks {
                    match block {
                        ReplyBlock::Reasoning(Reasoning::Visible { text, .. })
                        | ReplyBlock::Text { text, .. } => r(text),
                        ReplyBlock::ToolCall { args, .. } => redact_json(known, args),
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
                    redact_parts(known, output)
                }
                ExecOutcome::Failed { message, .. } | ExecOutcome::Cancelled { message } => {
                    r(message)
                }
            },
            MessageBody::Command { .. }
            | MessageBody::ToolResult { .. }
            | MessageBody::Boundary { .. } => {}
        }
    }
}

fn redact_parts(known: &[&str], parts: &mut [ContentPart]) {
    for part in parts {
        if let ContentPart::Text { text } = part {
            redact_in_place(known, text);
        }
    }
}

fn redact_json(known: &[&str], value: &mut serde_json::Value) {
    match value {
        serde_json::Value::String(s) => redact_in_place(known, s),
        serde_json::Value::Array(items) => items.iter_mut().for_each(|v| redact_json(known, v)),
        serde_json::Value::Object(map) => map.values_mut().for_each(|v| redact_json(known, v)),
        _ => {}
    }
}

fn redact_in_place(known: &[&str], s: &mut String) {
    if let Cow::Owned(redacted) = redact(known, s) {
        *s = redacted;
    }
}

fn redact<'a>(known: &[&str], text: &'a str) -> Cow<'a, str> {
    let mut out = Cow::Borrowed(text);
    for secret in known {
        if out.contains(secret) {
            out = Cow::Owned(out.replace(secret, &mask(secret)));
        }
    }
    for (rule, needs_digit) in RULES.iter() {
        let replaced = rule.replace_all(&out, |c: &Captures| mask_group(c, *needs_digit));
        if let Cow::Owned(replaced) = replaced {
            out = Cow::Owned(replaced);
        }
    }
    out
}

fn mask_group(c: &Captures, needs_digit: bool) -> String {
    let all = c.get(0).expect("整段匹配");
    let s = c.name("s").expect("规则都有 s 组");
    let text = all.as_str();
    if needs_digit && !s.as_str().bytes().any(|b| b.is_ascii_digit()) {
        return text.to_owned();
    }
    let (start, end) = (s.start() - all.start(), s.end() - all.start());
    format!("{}{}{}", &text[..start], mask(s.as_str()), &text[end..])
}

fn mask(secret: &str) -> String {
    let chars: Vec<char> = secret.chars().collect();
    if chars.len() < KEEP_EDGES_FROM {
        return MASK.to_owned();
    }
    let head: String = chars[..EDGE].iter().collect();
    let tail: String = chars[chars.len() - EDGE..].iter().collect();
    format!("{head}{MASK}{tail}")
}

#[cfg(test)]
mod tests;

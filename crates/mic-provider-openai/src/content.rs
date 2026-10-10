//! 两协议共用的内容转换规则。

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use mic_core::ProviderError;
use mic_message::{ContentPart, FileRef, ImageData};

pub(crate) fn unsupported_file(file: &FileRef) -> ProviderError {
    ProviderError::Rejected {
        message: format!("该模型配置不支持文件内容（{}，{}）", file.path, file.mime),
    }
}

/// 工具结果与无图 user 消息：文本片段按行拼接，含文件或图片即拒绝。
pub(crate) fn text_of(parts: Vec<ContentPart>) -> Result<String, ProviderError> {
    let mut texts = Vec::with_capacity(parts.len());
    for part in parts {
        match part {
            ContentPart::Text { text } => texts.push(text),
            ContentPart::File(file) => return Err(unsupported_file(&file)),
            ContentPart::Image(_) => {
                return Err(ProviderError::Rejected {
                    message: "工具结果里的图片暂不支持".into(),
                })
            }
        }
    }
    Ok(texts.join("\n"))
}

pub(crate) fn has_image(parts: &[ContentPart]) -> bool {
    parts.iter().any(|p| matches!(p, ContentPart::Image(_)))
}

pub(crate) fn data_url(img: &ImageData) -> String {
    format!(
        "data:{};base64,{}",
        img.format.mime(),
        BASE64.encode(&img.bytes)
    )
}

/// 历史里的工具参数：原文保留的字符串直接回传，结构化值序列化。
pub(crate) fn arguments_text(args: &serde_json::Value) -> String {
    match args {
        serde_json::Value::String(raw) => raw.clone(),
        other => other.to_string(),
    }
}

/// 只有 JSON 对象才存为结构化值；其余原文保留，由 mic-tool 边界报参数错误给模型。
pub(crate) fn parse_arguments(raw: String) -> serde_json::Value {
    match serde_json::from_str::<serde_json::Value>(&raw) {
        Ok(v @ serde_json::Value::Object(_)) => v,
        _ => serde_json::Value::String(raw),
    }
}

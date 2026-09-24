//! OpenAI Chat Completions 协议的模型实现：DeepSeek / Ollama 预设与通用兼容服务。
//! 契约：docs/blueprints/provider-openai.md；port 见 docs/blueprints/provider-port.md。

mod config;
mod limits;
mod provider;
mod request;
mod response;
mod wire;

use std::collections::BTreeMap;

use mic_core::{BoxError, Module, ModuleConfig, Registry};

use crate::config::EntryConfig;
use crate::provider::OpenAiProvider;

/// 模型模块：`[models.<name>]` 中 `kind = "openai"` 的条目各登记一个 Provider。
pub struct OpenAiModule;

impl Module for OpenAiModule {
    fn name(&self) -> &'static str {
        "openai"
    }

    fn install(&self, reg: &mut Registry, cfg: ModuleConfig) -> Result<(), BoxError> {
        let entries: BTreeMap<String, EntryConfig> = cfg.parse()?;
        for (name, entry) in entries {
            let provider = OpenAiProvider::new(entry.resolve(&name)?)?;
            reg.provider(name, provider);
        }
        Ok(())
    }
}

//! OpenAI 协议族的模型实现（手写协议，reqwest + SSE）：OpenAI 与 ChatGPT 订阅走 Responses，DeepSeek / Ollama / 通用兼容服务走 Chat Completions。
//! 契约：docs/blueprints/provider-openai.md、provider-handwritten.md；工厂契约见 model-settings.md §四。

mod chat;
mod client;
mod config;
mod content;
mod error;
mod factory;
mod limits;
mod probe;
mod responses;
mod stream;
mod token;

use mic_core::{BoxError, Module, ModuleConfig, Registry};

/// 模型模块：登记 `openai` 工厂；具体模型条目由用户在 Web 设置页维护。
pub struct OpenAiModule;

impl Module for OpenAiModule {
    fn name(&self) -> &'static str {
        "openai"
    }

    fn install(&self, reg: &mut Registry, _cfg: ModuleConfig) -> Result<(), BoxError> {
        reg.provider_factory(factory::OpenAiFactory);
        Ok(())
    }
}

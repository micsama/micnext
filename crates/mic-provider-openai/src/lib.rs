//! OpenAI Chat Completions 协议的模型实现：DeepSeek / Ollama 预设与通用兼容服务。
//! 契约：docs/blueprints/provider-openai.md；工厂契约见 docs/blueprints/model-settings.md §四。

mod config;
mod limits;
mod probe;
mod provider;
mod request;
mod response;
mod wire;

use std::sync::Arc;

use mic_core::{
    Activation, BoxError, BoxFuture, ConfigError, Module, ModuleConfig, ProbeError, Provider,
    ProviderFactory, Registry, SecretValue,
};

use crate::config::{EndpointConfig, ModelConfig};
use crate::provider::OpenAiProvider;

/// 模型模块：登记 `openai` 工厂；具体模型条目由用户在 Web 设置页维护。
pub struct OpenAiModule;

impl Module for OpenAiModule {
    fn name(&self) -> &'static str {
        "openai"
    }

    fn activation(&self) -> Activation {
        Activation::Always
    }

    fn install(&self, reg: &mut Registry, _cfg: ModuleConfig) -> Result<(), BoxError> {
        reg.provider_factory(OpenAiFactory);
        Ok(())
    }
}

struct OpenAiFactory;

impl ProviderFactory for OpenAiFactory {
    fn kind(&self) -> &'static str {
        "openai"
    }

    fn display_name(&self) -> &'static str {
        "OpenAI 兼容（DeepSeek / Ollama / vLLM 等）"
    }

    fn check_endpoint(&self, json: &str) -> Result<String, ConfigError> {
        let cfg = EndpointConfig::parse(json)?.canonical()?;
        Ok(serde_json::to_string(&cfg).expect("配置可序列化"))
    }

    fn check_model(
        &self,
        endpoint_json: &str,
        name: &str,
        model_json: &str,
    ) -> Result<String, ConfigError> {
        let endpoint = EndpointConfig::parse(endpoint_json)?;
        let cfg = ModelConfig::parse(model_json)?.canonical(&endpoint, name)?;
        Ok(serde_json::to_string(&cfg).expect("配置可序列化"))
    }

    fn key_env(&self, endpoint_json: &str) -> Option<&'static str> {
        EndpointConfig::parse(endpoint_json).ok()?.key_env()
    }

    fn build(
        &self,
        endpoint_json: &str,
        key: Option<SecretValue>,
        name: &str,
        model_json: &str,
    ) -> Result<Arc<dyn Provider>, ConfigError> {
        let endpoint = EndpointConfig::parse(endpoint_json)?;
        let model = ModelConfig::parse(model_json)?;
        let resolved = config::resolve(&endpoint, model, name, key)?;
        let provider = OpenAiProvider::new(resolved).map_err(|e| ConfigError {
            field: "config".into(),
            message: e.to_string(),
        })?;
        Ok(Arc::new(provider))
    }

    fn list_models(
        &self,
        endpoint_json: &str,
        key: Option<SecretValue>,
    ) -> BoxFuture<Result<Vec<String>, ProbeError>> {
        let endpoint = EndpointConfig::parse(endpoint_json);
        Box::pin(async move {
            let endpoint = endpoint.map_err(|e| ProbeError::Unexpected(e.message))?;
            probe::list_models(&endpoint, key).await
        })
    }
}

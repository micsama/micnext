use std::sync::Arc;

use futures_util::stream;
use mic_core::{
    BoxFuture, BoxStream, ConfigError, ModelEvent, ModelRequest, ProbeError, Provider,
    ProviderError, ProviderFactory, SecretValue,
};

use crate::client::{self, Client};
use crate::config::{self, EndpointConfig, ModelConfig, Protocol, Resolved};
use crate::stream::{drive, Open};
use crate::{chat, probe, responses};

pub(crate) struct OpenAiFactory;

impl ProviderFactory for OpenAiFactory {
    fn kind(&self) -> &'static str {
        "openai"
    }

    fn display_name(&self) -> &'static str {
        "OpenAI / 兼容服务（DeepSeek / Ollama / vLLM 等）"
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
        let client = connect(&endpoint, key)?;
        Ok(Arc::new(OpenAiProvider {
            client,
            cfg: config::resolve(&endpoint, model, name),
        }))
    }

    fn list_models(
        &self,
        endpoint_json: &str,
        key: Option<SecretValue>,
    ) -> BoxFuture<Result<Vec<String>, ProbeError>> {
        let client = EndpointConfig::parse(endpoint_json).and_then(|e| connect(&e, key));
        Box::pin(async move {
            let client = client.map_err(|e| ProbeError::Unexpected(e.message))?;
            probe::list_models(client).await
        })
    }
}

fn connect(endpoint: &EndpointConfig, key: Option<SecretValue>) -> Result<Client, ConfigError> {
    client::build(endpoint.base_url(), endpoint.headers(key)?).map_err(|e| ConfigError {
        field: "config".into(),
        message: e.to_string(),
    })
}

struct OpenAiProvider {
    client: Client,
    cfg: Resolved,
}

type Item = Result<ModelEvent, ProviderError>;

impl Provider for OpenAiProvider {
    fn model(&self) -> &str {
        &self.cfg.model
    }

    fn stream(&self, req: ModelRequest) -> BoxStream<Item> {
        let client = self.client.clone();
        match self.cfg.protocol {
            Protocol::Chat(dialect) => match chat::request(&self.cfg, dialect, &req) {
                Ok(body) => {
                    let open: Open<chat::Chunk> =
                        Box::pin(async move { client.chat().create_stream_byot(body).await });
                    drive(open, chat::Accumulator::new(dialect))
                }
                Err(e) => failed(e),
            },
            Protocol::Responses => match responses::request(&self.cfg, &req) {
                Ok(body) => {
                    let open: Open<_> =
                        Box::pin(async move { client.responses().create_stream_byot(body).await });
                    drive(open, responses::Accumulator)
                }
                Err(e) => failed(e),
            },
        }
    }
}

fn failed(e: ProviderError) -> BoxStream<Item> {
    Box::pin(stream::once(async { Err(e) }))
}

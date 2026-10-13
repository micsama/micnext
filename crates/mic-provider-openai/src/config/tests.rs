use super::*;
use mic_core::{ModelRequest, ToolChoice};

#[test]
fn reasoning_is_a_call_parameter_in_both_protocols() {
    assert!(ModelConfig::parse(r#"{"reasoning_effort":"none"}"#).is_err());
    let req = ModelRequest {
        tool_choice: ToolChoice::None,
        system: "test".into(),
        messages: Vec::new(),
        tools: Vec::new(),
        images: Default::default(),
        cache_key: "test".into(),
    };
    for preset in ["deepseek", "openai", "chatgpt"] {
        let endpoint = EndpointConfig::parse(&format!(r#"{{"preset":"{preset}"}}"#))
            .unwrap()
            .canonical()
            .unwrap();
        for (effort, wire) in [
            (ReasoningEffort::None, "none"),
            (ReasoningEffort::Low, "low"),
            (ReasoningEffort::High, "high"),
        ] {
            let model = ModelConfig::parse("{}")
                .unwrap()
                .canonical(&endpoint, "test")
                .unwrap();
            let cfg = resolve(&endpoint, model, "test", Some(effort));
            let (body, field) = match cfg.protocol {
                Protocol::Chat(dialect) => (
                    serde_json::to_value(crate::chat::request(&cfg, dialect, &req).unwrap())
                        .unwrap(),
                    "/reasoning_effort",
                ),
                Protocol::Responses(_) => (
                    serde_json::to_value(crate::responses::request(&cfg, &req).unwrap()).unwrap(),
                    "/reasoning/effort",
                ),
            };
            assert_eq!(body.pointer(field).unwrap(), wire);
        }
    }
    let endpoint = EndpointConfig::parse(r#"{"preset":"ollama"}"#)
        .unwrap()
        .canonical()
        .unwrap();
    assert!(endpoint.reasoning_options("test").is_none());
    let cfg = resolve(&endpoint, ModelConfig::parse("{}").unwrap(), "test", None);
    let body =
        serde_json::to_value(crate::chat::request(&cfg, Dialect::Ollama, &req).unwrap()).unwrap();
    assert!(body.get("reasoning_effort").is_none());
}

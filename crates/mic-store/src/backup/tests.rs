use super::*;
use crate::{CredentialWrite, EndpointWrite, ModelWrite, Settings};
use std::future::Future;

fn run(f: impl Future<Output = ()>) {
    tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap()
        .block_on(f);
}

async fn populated() -> Store {
    let store = Store::open_in_memory(&[]).await.unwrap();
    let endpoint = store
        .create_endpoint(
            EndpointWrite {
                name: "provider / one".into(),
                kind: "openai".into(),
                config_json: "{}".into(),
                credential: CredentialWrite::Set(SecretValue::new("secret-key".into())),
            },
            1,
        )
        .await
        .unwrap();
    let model = store
        .create_model(
            ModelWrite {
                starred: false,
                endpoint_id: endpoint,
                name: "model / one".into(),
                config_json: "{}".into(),
                context_window: NonZeroU32::new(65536),
                images: true,
                price: ModelPrice {
                    input: Some(0.1),
                    output: Some(0.3),
                    ..Default::default()
                },
            },
            1,
        )
        .await
        .unwrap();
    let persona = store
        .create_persona("test persona".into(), "test prompt".into(), 1)
        .await
        .unwrap();
    store
        .update_settings(Settings {
            default_persona: persona,
            general_prompt: "general".into(),
            default_workdir: "/tmp".into(),
            max_turns: 12,
            compaction: CompactionSettings {
                trigger_percent: 60,
                keep_percent: 10,
            },
        })
        .await
        .unwrap();
    store
        .set_title_model(crate::TitleModelSelection {
            model_id: Some(model),
            reasoning_effort: Some(ReasoningEffort::Low),
        })
        .await
        .unwrap();
    store
}

#[test]
fn roundtrip_and_overwrite_preserve_ids_and_remove_missing() {
    run(async {
        let source = populated().await;
        let backup = source.export_settings().await.unwrap();
        let value = serde_json::to_value(&backup).unwrap();
        let target = Store::open_in_memory(&[]).await.unwrap();
        target.import_settings(backup, 2).await.unwrap();
        assert_eq!(
            serde_json::to_value(target.export_settings().await.unwrap()).unwrap(),
            value
        );
        let endpoints = target.endpoints().await.unwrap();
        let models = target.models().await.unwrap();
        let persona = target.settings().await.unwrap().default_persona;
        let removed = target
            .create_persona("remove".into(), "remove".into(), 3)
            .await
            .unwrap();
        let extra = target
            .create_endpoint(
                EndpointWrite {
                    name: "extra".into(),
                    kind: "openai".into(),
                    config_json: "{}".into(),
                    credential: CredentialWrite::Set(SecretValue::new("extra-key".into())),
                },
                3,
            )
            .await
            .unwrap();
        target
            .import_settings(serde_json::from_value(value.clone()).unwrap(), 4)
            .await
            .unwrap();
        assert_eq!(target.endpoints().await.unwrap(), endpoints);
        assert_eq!(target.models().await.unwrap(), models);
        assert_eq!(target.settings().await.unwrap().default_persona, persona);
        assert!(target.persona(removed).await.unwrap().unwrap().deleted);
        assert!(target.endpoint(extra).await.unwrap().is_none());
        assert_eq!(
            serde_json::to_value(target.export_settings().await.unwrap()).unwrap(),
            value
        );
        target
            .call(move |conn| {
                let key: Option<Vec<u8>> = conn.query_row(
                    "SELECT key_cipher FROM core_endpoints WHERE id=?1",
                    [extra.0],
                    |r| r.get(0),
                )?;
                assert!(key.is_none());
                Ok(())
            })
            .await
            .unwrap();
    });
}

#[test]
fn invalid_reference_and_duplicate_names_roll_back_everything() {
    run(async {
        let store = populated().await;
        let before = serde_json::to_value(store.export_settings().await.unwrap()).unwrap();
        let mut invalid = store.export_settings().await.unwrap();
        invalid.endpoints[0].key = Some("changed".into());
        invalid.preferences.default_persona = "missing".into();
        assert!(matches!(
            store.import_settings(invalid, 2).await,
            Err(SettingsError::ImportInvalid(_))
        ));
        assert_eq!(
            serde_json::to_value(store.export_settings().await.unwrap()).unwrap(),
            before
        );
        let mut invalid = store.export_settings().await.unwrap();
        invalid.preferences.default_model.as_mut().unwrap().model = "missing".into();
        assert!(matches!(
            store.import_settings(invalid, 2).await,
            Err(SettingsError::ImportInvalid(_))
        ));
        let mut invalid = store.export_settings().await.unwrap();
        invalid.personas.push(ExportPersona {
            name: "test persona".into(),
            prompt: "duplicate".into(),
        });
        assert!(matches!(
            store.import_settings(invalid, 2).await,
            Err(SettingsError::ImportInvalid(_))
        ));
        assert_eq!(
            serde_json::to_value(store.export_settings().await.unwrap()).unwrap(),
            before
        );
    });
}

#[test]
fn encrypted_export_rejects_tampering_wrong_key_and_endpoint_ciphertext() {
    run(async {
        use base64::{engine::general_purpose::STANDARD, Engine};
        let store = populated().await;
        let plain = SecretValue::new(
            serde_json::to_string(&store.export_settings().await.unwrap()).unwrap(),
        );
        let text = store.seal_export(&plain);
        assert!(!text.contains("secret-key"));
        assert_eq!(store.open_export(&text).unwrap().expose(), plain.expose());
        assert_ne!(store.seal_export(&plain), text);
        let other = Store::open_in_memory(&[]).await.unwrap();
        assert!(matches!(
            other.open_export(&text),
            Err(StoreError::ExportUndecryptable)
        ));
        let mut blob = STANDARD
            .decode(text.strip_prefix("micnext-settings:").unwrap())
            .unwrap();
        blob[12] ^= 1;
        assert!(store
            .open_export(&format!("micnext-settings:{}", STANDARD.encode(blob)))
            .is_err());
        let endpoint = store.cipher().encrypt(1, &plain);
        assert!(store
            .open_export(&format!("micnext-settings:{}", STANDARD.encode(endpoint)))
            .is_err());
        let blob = STANDARD
            .decode(text.strip_prefix("micnext-settings:").unwrap())
            .unwrap();
        assert!(store.cipher().decrypt(1, &blob).is_err());
        for text in ["", "micnext-settings:bad", "micnext-settings:"] {
            assert!(store.open_export(text).is_err());
        }
    });
}

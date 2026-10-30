//! 设置里的命令环境变量：工具执行时注入，出站时精确脱敏，两处读同一份快照。
//! 契约：docs/blueprints/env-vars.md。

use std::sync::{Arc, RwLock};

use mic_store::{SecretValue, SettingsError, Store, StoreError};
use serde::Serialize;

type Vars = Arc<[(String, String)]>;

/// 设置页看到的变量：值按模型侧正则同样的方式遮住两头之外的部分。
#[derive(Debug, Clone, Serialize)]
pub struct EnvVarView {
    pub name: String,
    pub masked: String,
}

/// 快照只随写入更新；写库与刷新在同一把锁内，并发修改不会留下旧快照。
#[derive(Default)]
pub(crate) struct EnvVars {
    current: RwLock<Vars>,
    writes: tokio::sync::Mutex<()>,
}

impl EnvVars {
    pub(crate) async fn load(store: &Store) -> Result<Self, StoreError> {
        Ok(Self {
            current: RwLock::new(read(store).await?),
            writes: Default::default(),
        })
    }

    pub(crate) fn current(&self) -> Vars {
        self.current.read().expect("快照锁不中毒").clone()
    }

    pub(crate) fn views(&self) -> Vec<EnvVarView> {
        self.current()
            .iter()
            .map(|(name, value)| EnvVarView {
                name: name.clone(),
                masked: crate::redact::ends(value),
            })
            .collect()
    }

    pub(crate) async fn set(
        &self,
        store: &Store,
        name: String,
        value: SecretValue,
    ) -> Result<(), SettingsError> {
        let _write = self.writes.lock().await;
        store.set_env_var(name, value).await?;
        self.refresh(store).await?;
        Ok(())
    }

    pub(crate) async fn delete(&self, store: &Store, name: String) -> Result<(), StoreError> {
        let _write = self.writes.lock().await;
        store.delete_env_var(name).await?;
        self.refresh(store).await
    }

    /// 设置导入整体替换后调用。
    pub(crate) async fn reload(&self, store: &Store) -> Result<(), StoreError> {
        let _write = self.writes.lock().await;
        self.refresh(store).await
    }

    async fn refresh(&self, store: &Store) -> Result<(), StoreError> {
        let vars = read(store).await?;
        *self.current.write().expect("快照锁不中毒") = vars;
        Ok(())
    }
}

async fn read(store: &Store) -> Result<Vars, StoreError> {
    Ok(store
        .env_vars()
        .await?
        .into_iter()
        .map(|(name, value)| (name, value.expose().to_owned()))
        .collect())
}

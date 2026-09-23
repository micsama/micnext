use mic_message::SessionId;
use mic_store::rusqlite::{self, Transaction};
use mic_store::{NewSession, Session, Store};

use crate::KernelError;

/// 模块拿到的内核窄接口。Clone 廉价；核心表只经这些方法写。
#[derive(Clone)]
pub struct Kernel {
    store: Store,
}

impl Kernel {
    pub(crate) fn new(store: Store) -> Self {
        Self { store }
    }

    /// Channel 用：按 (channel, chat) 原子取得或新建 Root 会话。
    pub async fn resolve_root_session(
        &self,
        channel: &str,
        chat: &str,
        init: NewSession,
    ) -> Result<Session, KernelError> {
        Ok(self.store.resolve_root_session(channel, chat, init).await?)
    }

    /// Task/Triggered 用。
    pub async fn create_session(&self, s: NewSession) -> Result<SessionId, KernelError> {
        Ok(self.store.create_session(s).await?.id)
    }

    /// 只许访问本模块 `{name}_` 前缀的表；由模块单元测试把关。
    pub async fn with_module_tx<R, F>(&self, f: F) -> Result<R, KernelError>
    where
        F: FnOnce(&Transaction<'_>) -> rusqlite::Result<R> + Send + 'static,
        R: Send + 'static,
    {
        Ok(self.store.with_module_tx(f).await?)
    }
}

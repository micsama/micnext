use std::path::Path;
use std::sync::{Arc, Mutex};

use rusqlite::{Connection, Transaction};

use crate::diagnostic::Diagnostic;
use crate::endpoints;
use crate::secrets::Cipher;
use crate::{Migration, SecretColumn, SecretKeyFile, SettingsError, StoreError};

#[derive(Clone)]
pub struct Store {
    conn: Arc<Mutex<Connection>>,
    cipher: Arc<Cipher>,
    /// 内存库无只读诊断连接。
    pub(crate) diagnostic: Option<Arc<Diagnostic>>,
}

impl Store {
    pub async fn open(
        path: &Path,
        modules: &[Migration],
        key: SecretKeyFile,
    ) -> Result<Self, StoreError> {
        let path = path.to_owned();
        let diagnostic = path.clone();
        Self::init(
            move || Connection::open(path),
            move |secrets| Diagnostic::open(&diagnostic, secrets).map(Some),
            modules,
            move |has_ciphertext| Cipher::from_file(&key, has_ciphertext),
        )
        .await
    }

    /// 主密钥为随机内存密钥，不写磁盘。
    pub async fn open_in_memory(modules: &[Migration]) -> Result<Self, StoreError> {
        Self::init(
            Connection::open_in_memory,
            |_| Ok(None),
            modules,
            |_| Ok(Cipher::random()),
        )
        .await
    }

    async fn init<F, D, K>(
        connect: F,
        diagnostic: D,
        modules: &[Migration],
        key: K,
    ) -> Result<Self, StoreError>
    where
        F: FnOnce() -> rusqlite::Result<Connection> + Send + 'static,
        D: FnOnce(Vec<SecretColumn>) -> rusqlite::Result<Option<Diagnostic>> + Send + 'static,
        K: FnOnce(bool) -> Result<Cipher, StoreError> + Send + 'static,
    {
        let modules = modules.to_vec();
        let (conn, diagnostic, cipher) = tokio::task::spawn_blocking(move || {
            let mut conn = connect()?;
            crate::schema::init(&mut conn, &modules)?;
            let diagnostic = diagnostic(crate::schema::secret_columns(&modules))?;
            let cipher = key(endpoints::has_ciphertext(&conn)?)?;
            endpoints::verify_keys(&conn, &cipher)?;
            Ok::<_, StoreError>((conn, diagnostic, cipher))
        })
        .await
        .expect("store 阻塞任务 panic")?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
            cipher: Arc::new(cipher),
            diagnostic: diagnostic.map(Arc::new),
        })
    }

    pub(crate) fn cipher(&self) -> Arc<Cipher> {
        Arc::clone(&self.cipher)
    }

    pub(crate) async fn call<R, F>(&self, f: F) -> Result<R, StoreError>
    where
        R: Send + 'static,
        F: FnOnce(&mut Connection) -> Result<R, StoreError> + Send + 'static,
    {
        self.blocking(f).await
    }

    pub(crate) async fn call_settings<R, F>(&self, f: F) -> Result<R, SettingsError>
    where
        R: Send + 'static,
        F: FnOnce(&mut Connection) -> Result<R, SettingsError> + Send + 'static,
    {
        self.blocking(f).await
    }

    pub(crate) async fn blocking<R, E, F>(&self, f: F) -> Result<R, E>
    where
        R: Send + 'static,
        E: Send + 'static,
        F: FnOnce(&mut Connection) -> Result<R, E> + Send + 'static,
    {
        let conn = Arc::clone(&self.conn);
        tokio::task::spawn_blocking(move || {
            let mut conn = conn.lock().expect("store 锁中毒");
            f(&mut conn)
        })
        .await
        .expect("store 阻塞任务 panic")
    }

    pub async fn with_module_tx<R, F>(&self, f: F) -> Result<R, StoreError>
    where
        R: Send + 'static,
        F: FnOnce(&Transaction<'_>) -> rusqlite::Result<R> + Send + 'static,
    {
        self.call(move |conn| {
            let tx = conn.transaction()?;
            let r = f(&tx)?;
            tx.commit()?;
            Ok(r)
        })
        .await
    }
}

#[cfg(test)]
mod tests;

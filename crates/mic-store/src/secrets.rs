//! 模型 API key 的加解密与主密钥文件。契约：docs/blueprints/model-settings.md §五。

use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::{ErrorKind, Write};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::PathBuf;

use aes_gcm::aead::{Aead, Generate, Key, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Nonce};
use zeroize::Zeroizing;

use crate::StoreError;

const NONCE_LEN: usize = 12;
const KEY_LEN: usize = 32;

/// 明文秘密：无 Debug/Display/Serialize，drop 时清零。
pub struct SecretValue(Zeroizing<String>);

impl SecretValue {
    pub fn new(value: String) -> Self {
        Self(Zeroizing::new(value))
    }

    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for SecretValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SecretValue(..)")
    }
}

/// 主密钥文件路径；文件内容为 32 字节原始密钥。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecretKeyFile(pub PathBuf);

pub(crate) struct Cipher(Aes256Gcm);

impl Cipher {
    pub(crate) fn random() -> Self {
        Self(Aes256Gcm::new(&Key::<Aes256Gcm>::generate()))
    }

    /// 文件缺失时：库内已有密文则报错，否则原子创建（0600）。
    pub(crate) fn from_file(
        file: &SecretKeyFile,
        has_ciphertext: bool,
    ) -> Result<Self, StoreError> {
        let path = &file.0;
        match fs::read(path) {
            Ok(bytes) => {
                let mode = fs::metadata(path)?.mode();
                if mode & 0o077 != 0 {
                    return Err(StoreError::SecretKeyInvalid(format!(
                        "{} 权限为 {:o}，须为 0600：chmod 600 {}",
                        path.display(),
                        mode & 0o777,
                        path.display()
                    )));
                }
                let key: [u8; KEY_LEN] = bytes.as_slice().try_into().map_err(|_| {
                    StoreError::SecretKeyInvalid(format!(
                        "{} 须为 {KEY_LEN} 字节原始密钥，实际 {} 字节",
                        path.display(),
                        bytes.len()
                    ))
                })?;
                Ok(Self(Aes256Gcm::new(&key.into())))
            }
            Err(e) if e.kind() == ErrorKind::NotFound => {
                if has_ciphertext {
                    return Err(StoreError::SecretKeyInvalid(format!(
                        "库内已有加密的 API key，但主密钥文件 {} 不存在；请恢复该文件，或清空各模型的 key 后重设",
                        path.display()
                    )));
                }
                let key = Key::<Aes256Gcm>::generate();
                let mut f = OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .mode(0o600)
                    .open(path)?;
                f.write_all(key.as_slice())?;
                f.sync_all()?;
                Ok(Self(Aes256Gcm::new(&key)))
            }
            Err(e) => Err(e.into()),
        }
    }

    /// `nonce || ciphertext || tag`。
    pub(crate) fn encrypt(&self, endpoint: i64, secret: &SecretValue) -> Vec<u8> {
        let nonce = Nonce::generate();
        let sealed = self
            .0
            .encrypt(
                &nonce,
                Payload {
                    msg: secret.expose().as_bytes(),
                    aad: &aad(endpoint),
                },
            )
            .expect("AES-GCM 加密内存缓冲不会失败");
        let mut out = nonce.to_vec();
        out.extend_from_slice(&sealed);
        out
    }

    pub(crate) fn decrypt(&self, endpoint: i64, blob: &[u8]) -> Result<SecretValue, StoreError> {
        let integrity = || StoreError::SecretIntegrity { endpoint };
        if blob.len() <= NONCE_LEN {
            return Err(integrity());
        }
        let (nonce, sealed) = blob.split_at(NONCE_LEN);
        let nonce = Nonce::try_from(nonce).map_err(|_| integrity())?;
        let plain = self
            .0
            .decrypt(
                &nonce,
                Payload {
                    msg: sealed,
                    aad: &aad(endpoint),
                },
            )
            .map_err(|_| integrity())?;
        String::from_utf8(plain)
            .map(SecretValue::new)
            .map_err(|_| integrity())
    }
}

fn aad(endpoint: i64) -> Vec<u8> {
    format!("endpoint-key:{endpoint}").into_bytes()
}

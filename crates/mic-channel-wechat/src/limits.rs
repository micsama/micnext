use std::time::Duration;

pub(crate) const API_BASE: &str = "https://ilinkai.weixin.qq.com/";
pub(crate) const APP_ID: &str = "bot";
pub(crate) const BOT_TYPE: &str = "3";
pub(crate) const PROTOCOL_VERSION: &str = "2.4.9";
pub(crate) const CLIENT_VERSION: u32 = (2 << 16) | (4 << 8) | 9;
pub(crate) const QR_TIMEOUT: Duration = Duration::from_secs(35);
pub(crate) const QR_LIFETIME: Duration = Duration::from_secs(300);
pub(crate) const POLL_INTERVAL: Duration = Duration::from_secs(1);
pub(crate) const NETWORK_RETRY: Duration = Duration::from_secs(2);
pub(crate) const LONGPOLL_TIMEOUT_MS: u32 = 35_000;
pub(crate) const COMMAND_CAPACITY: usize = 16;
pub(crate) const ATTEMPT_ID_BYTES: usize = 16;
pub(crate) const LOCAL_TOKEN_LIMIT: usize = 10;
pub(crate) const VERIFY_CODE_MAX_CHARS: usize = 128;
pub(crate) const TEXT_CHUNK_CHARS: usize = 4000;
pub(crate) const SEND_ATTEMPTS: u32 = 2;
pub(crate) const SEND_RETRY: Duration = Duration::from_secs(2);
/// 与 SDK 一致：输入中指示每 5 秒续发一次。
pub(crate) const TYPING_KEEPALIVE: Duration = Duration::from_secs(5);
pub(crate) const API_TIMEOUT: Duration = Duration::from_secs(15);
pub(crate) const NETWORK_SLOW_RETRY: Duration = Duration::from_secs(30);
pub(crate) const NETWORK_SLOW_THRESHOLD: u32 = 3;

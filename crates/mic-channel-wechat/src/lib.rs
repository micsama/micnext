//! 微信 iLink Channel；Gateway 仅经 core 登录 port 访问。

mod account;
mod client;
mod delivery;
mod limits;
mod login;
mod media;
mod quote;
mod service;
mod typing;
mod wire;

pub use service::WechatModule;

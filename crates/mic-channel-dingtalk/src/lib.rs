//! 钉钉 Channel：整体复用 micbot 现有逻辑，改接统一 Channel 协议
//! （ReplyContext 模式：发送目标编码成版本化不透明 payload，随 Session 存）。
//! 空壳阶段——协议对接细节动工前先走 B2。
//! 设计依据：docs/brainstorm/next-gen-architecture.md §六/§7.3/§十(8)。

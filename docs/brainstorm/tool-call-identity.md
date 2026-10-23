# 工具调用身份

2026-10-22 审查发现 Web 与恢复共用全会话／run 范围的 wire id，存在跨轮误关联。仅修前端不能消除 job 唯一约束问题，改写协议 id 又扩大 Provider 影响。

2026-10-22 human 授权审查剩余项一并实施；选择复用 Reply 消息身份与块位置，契约见 [B2](../blueprints/archive/tool-call-identity.md)。不新增全局执行 id，不把外部字符串强行规定为内部唯一键。

# B1：Web 开发者日志

**状态：方向收敛；契约已与只读 SQL 合并为 [开发者诊断 B2](../blueprints/developer-diagnostics.md)，待批准（2026-10-10）。**

## 已确认方向

个人开发者工具，做到够用。先做 Web 日志，之后再做更新重启；脱敏仅记录、延期。

日志仅放内存，重启清空；自己的模块采集 DEBUG 及以上，依赖只采 INFO 及以上，与终端 RUST_LOG 独立。页面支持实时查看、文本/正则搜索、级别/模块筛选、暂停自动滚动、复制筛选结果。聊天原文可接受；沿用现有凭据隐藏规则，不声称全部日志已脱敏。

## 保留的设计结论

- 容量按条数、总字节、单条大小限制；截断与缺口可见。
- 暂停只停跟随，不停接收；慢浏览器不能阻塞业务 emit。
- 缓冲在 Gateway，tracing 由二进制装配，不放入 core 的 KernelEvent，也不落数据库。
- 独立日志 SSE 回放/跟随；复用现有鉴权与分帧，重连清空页面并重放当前缓冲。接受最多 16 MiB 日志文本及 JSON/SSE 编码开销的重传，不做客户端游标续传。
- 不新增全量聊天/工具埋点，不捕获所有 stderr/panic，不做日志平台。
- 页面内完成有界筛选；非法正则提示。首期接受开发者复杂正则可能卡页，不加 worker、超时协调与另一份缓存。

## 现状依据

main.rs 当前使用带 EnvFilter 的 fmt subscriber；需改为独立 layer 过滤。唯一 GatewayModule 构造一次迁移，不新增临时 API/无关 module.rs；-p 不创建日志缓冲或随机标识。微信 client.rs 已有 `wechat inbound quote` DEBUG 事件，可直接搜索其现有字段。

实体/状态、公开签名、容量、HTTP/SSE、调用方兼容和验收统一见 B2，本文不重复。原后续更新候选已移到 [更新 B1](self-update.md)和 [更新 B2](../blueprints/self-update.md)。

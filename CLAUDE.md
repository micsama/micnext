# micnext

单进程个人 Rust agent：常驻 Gateway（Web）+ Channel，`-p` 一次性调试。按独立变化轴建模，一个概念只承载一股语义。

## 流程

Human 定方向和公开契约，AI 探索、起草、实现。

- **B1**：方向未定 → `docs/brainstorm/<topic>.md` 写候选与权衡，先不实现。
- **B2**：改公开契约（crate、模块、依赖、公开类型/签名、HTTP 接口）→ `docs/blueprints/<topic>.md` 写清契约和调用方影响，批准后实现；实现中发现缺项就停下报告。
- **收尾**：`cargo fmt`、`cargo clippy -- -D warnings`，改前端跑 `web/` 下 `bun run check`。
- **验收与关闭**：能本地自动验收的自己跑完；页面观感和真实环境交给 human。通过后 B2 标 `CLOSED`，`todo.md` 条目移到 `todo-archive.md`。

`todo.md` 每个主题一行，指向文档。审查只找缺陷；架构方向交回 human。

## 设计

- 独立变化的拆，共同变化的合；不为假想未来上重机制。拒绝过度设计、过度防御。
- 外部输入（HTTP、Channel、模型输出、配置、磁盘库）在边界 parse 成强类型，内部互信，不做防御性校验。
- Fail fast，不静默 fallback；降级须 human 同意且对用户可见。
- 一处真相；同一流程不另起分支；新抽象要有明确消费者。
- 同一模块第二个不相关补丁 → 停下考虑重构。
- 大重构不保留兼容性代码，减少历史债务。
- 配置：`config.toml` 只放启动项；用户可调项存 SQLite 经 Web 改；构建期旋钮放 `limits.rs`。

## 代码

- 优先 `tokio` / `serde` / `thiserror` / `anyhow`；注释只写结论。
- 测试以 review 和验收为主，值得写再写，放 `foo/tests.rs`。
- crate 分层：`mic-message` → `mic-store` / `mic-tool` / `mic-media` → `mic-core` → 实现层（Gateway、Channel、Provider、工具），实现层彼此不依赖，由 `bin/micnext` 装配。

## 提交

`type(scope): 中文动宾短句`，正文写动机和影响。用户要求才提交；工作区常有 human 的改动，只暂存自己的。

# micnext 开发约束

轻量、可 hook、正交的 Rust agent，单进程个人工具，不做多实例/分布式。**按独立变化轴建模，一个概念只承载一股语义。**

## 验收

- 唯一常驻模式：挂载 Gateway（Web）+ 已启用的 Channel 长期运行。一次性调试调用（暂定 `-p`/`--chat`）不进 Channel、用完即退，接口形状动工前走 B2。
- 收尾：`cargo fmt` + `cargo clippy -- -D warnings`；改动覆盖到已有测试才跑测试。

## 设计约束

- **拆合看变化轴**：独立变化的拆，只共同变化的合；无证据时视为会独立变化。已知变化轴上的薄接口直接做，不为假想未来上重机制。
- **simple != small**：禁止用 bool 切行为或 action 字符串分发核心逻辑。删机制前说明复杂度去向；领域规则由懂领域的一侧解析，不推给调用方约定。
- **删除查四项**：正确性、隐私边界、失败行为、可审计性全过才许删；不懂来源先查 `git log`。
- **Fail Fast**：配置缺失、实现不匹配、不变量破坏 → 启动报错或 `Err`；禁止静默 fallback。
- **边界收严、收全、不猜**：外部数据在边界一次 parse 成强类型，禁止裸 `serde_json::Value` 漂流；上游字段暂无消费者也完整接收。边界后默认数据合法，内部调用互信，不做防御性校验和重复检查。外部 = HTTP/Channel 入站、模型输出（含 tool args）、配置、磁盘上的库文件版本。
- **配置合并同类项**：用户可调项统一进 `config.toml`，按用户概念分组（如 `[models.*]`），不按 crate 散落，求精简好用；build 后不改的旋钮每个 crate 集中在 `limits.rs`。
- **一处真相**：事实和计算只留一个来源；元数据归最早知道它的 emit 侧，第二个消费者出现时下沉共享。换消费者仍需要的元数据属于协议字段。
- **一条主路径**：仅边界参数不同的流程复用全部事件、落盘和取消，不新增终态、event 或 runner；主路径若需读场景专属状态或分支则拆开。
- **抽象有进有出**：新增封装、状态或协议事件必须指出最终消费者和直接收益；消费者消失时连生产者一起删。
- **重构而非绕行**：同一模块出现第 2 个不相关补丁 → 停止实现做架构 review，默认产出重构提案。重构只针对已观察到的摩擦（补丁熔断、变化轴被证实、一处真相被破坏），不为假想扩展性。小步可编译可回滚推进；改公开契约走 parallel change 并走 B2，迁移完成即删旧路径不留双轨；不改契约沿用 B2 豁免，但需给出行为等价证据（日志或验收命令）。重写受 B1 约束，期间旧路径冻结。
- **禁止自等待死锁**：「内部命令通道 + 带 ack 的 oneshot」模式下，命令处理逻辑本身向同一通道发命令只能 detached fire-and-forget，禁止等待 ack；命令队列只在处理间隙 drain，否则必死锁。

## Rust 约定

- 优先 `tokio` / `serde` / `thiserror`（库内）/ `anyhow`（二进制收口），不造同类轮子；Web 前端资源嵌入用 `rust-embed`。
- 注释只写结论，不写防御性过程说明；特殊注释用 `TODO` / `FIXME` / `NOTE` / `HACK` / `WARN` / `SAFETY`，unsafe 必须有 `SAFETY`。
- **测试从简**：小项目，默认不写测试，靠 review 和手工验收守。确有必要才加：单元测试放同名子模块文件，`foo.rs` 只声明 `#[cfg(test)] mod tests;`，测试写在 `foo/tests.rs`（`lib.rs` 对应 `src/tests.rs`）；只经公开 API 验证 crate 契约时才用 `tests/`，每个 crate 至多一个文件。默认不读 `tests.rs` 与 `tests/`。

## Semantic Blueprint Protocol

Blueprint → Implementation 不可跳；Brainstorm 按需前置。Human 决定方向、公开契约和副作用边界；AI 探索、起草并在批准范围内实现。

- **B1**（方向未收敛）：存在多个真实候选，或架构、所有权、依赖方向、关键未知项未定 → 候选、权衡和未知项记入 `todo.md`，不写实现。多实体或并发协调不得跳过：产出实体与写者清单、各实体状态图、1-2 个关键场景 sequence、必须成立的不变量。
- **B2**（改公开契约）：新增 crate、模块、跨模块依赖、公开 trait/类型/签名/错误枚举 → 写 `docs/blueprints/<topic>.md`，说清签名、错误、强类型契约、副作用和依赖方向；枚举全部调用方逐项判兼容，一个功能改多处则合成一份。**批准前禁止实现**；方向未收敛退回 B1。
- **B2 豁免**：不改公开签名的 bug fix、配置、日志、注释和私有重构。
- **B3**（审查）：按本文件审 Blueprint 或实现。本次改动的缺陷在既定契约内修；既有架构缺陷或新方向交回 human，必要时退回 B1/B2。
- **B4**（实现）：严格按已批准契约；Blueprint 缺项或矛盾 → 立即停手报告。完成后说明端到端事件/状态变化和人工验收方法。
- **关闭**：fmt 和 clippy 通过后标 Blueprint `CLOSED`，更新 `todo.md`。

## Codex Review

- 只找缺陷和挑战假设，不替 human 拍方向。多文件 review 后台跑；review gate 默认关闭。
- 每条 finding 标 `accept` / `reject` / `needs-investigation`（reject 必须说理由），并分类「本次改动缺陷」或「既有架构缺陷」。禁止无脑采纳。
- 方案选择、架构方向和既有架构缺陷交回 human，不在 B3/B4 顺手修。review-fix 超 3 轮未收敛，或连续两轮只有新长尾 → 退回 B1。

## 提交

- 格式 `type(scope): 中文动宾短句`；正文说明动机和影响。
- B1/B2/B3 讨论、Blueprint 修订和 review-fix 中间态不提交；结果按可独立理解、验证、回滚的状态组织，文档/实现共同表达一项变更时可合并。禁止创建明知构建失败的提交。
- 已批准但明确延期的 Blueprint 可独立提交；搁置方案只提交有长期价值的结论。除非用户明确要求，agent 不自行 commit。

## 依赖不变量

真实依赖以 `cargo tree` 为准，不建架构测试，靠 review 守：

- `mic-message` 是 L0：零内部依赖，只被依赖。
- `mic-store` / `mic-tool` 只依赖 `mic-message`，互不依赖，也不依赖 `mic-core`。
- `mic-core` 依赖 `mic-message` + `mic-store` + `mic-tool`，不依赖任何 Channel 或功能模块。
- `mic-gateway`、各 Channel、各功能模块（如 `mic-cron`）依赖 `mic-core`（port）+ 需要的下层 crate；模块之间不互相依赖，只由二进制装配。
- 不得成环。新增跨 crate 依赖先回 B2；`lib.rs` 只 re-export 公开 API，内部默认 `pub(crate)`。

本文件条目长期无引用或已被其他条目覆盖则删除。

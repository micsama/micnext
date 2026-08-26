# micnext 开发约束

轻量、可 hook、正交的 Rust agent，单进程个人工具，不做多实例/分布式。**按独立
变化轴建模，一个概念只承载一股语义。** 本文件从 micbot 的 `CLAUDE.md` 裁剪而来，
保留跨项目通用的约束；micbot 专属的历史 BP 编号、TUI 依赖、旧 crate 名不带入。

## 验收

- 启动方式待定（当前是空壳，尚无 binary 入口约定；见
  `docs/brainstorm/next-gen-architecture.md` §九 前端形态）。
- 收尾：`cargo fmt` + `cargo clippy -- -D warnings` + 与改动相关的测试。

## 设计约束

- **拆合看变化轴**：独立变化的拆，只共同变化的合；没有证据时视为会独立变化。已知变化轴上的薄接口直接做，不为假想未来上重机制。
- **simple != small**：禁止用 bool 切行为或 action 字符串分发核心逻辑。删除机制前说明复杂度去向；领域规则由懂领域的一侧解析，不推给调用方约定。
- **删除查四项**：正确性、隐私边界、失败行为、可审计性全过才许删；不懂来源先查 `git log`。
- **Fail Fast**：配置缺失、实现不匹配或不变量破坏 → 启动报错或 `Err`；禁止静默 fallback。
- **边界收严、收全、不猜**：外部数据在边界一次 parse 成强类型，禁止裸 `serde_json::Value` 漂流；上游字段即使暂无消费者也完整接收。边界后默认数据合法。
- **一处真相**：事实和计算只保留一个来源；元数据归最早知道它的 emit 侧；第二个消费者出现时下沉共享。换个消费者仍需要的元数据属于协议字段。
- **一条主路径**：仅边界参数不同的流程复用全部事件、落盘、取消和测试，不新增终态、event 或 runner；主路径若需读取场景专属状态或分支则拆开。
- **抽象有进有出**：新增封装、状态或协议事件必须指出最终消费者和直接收益；消费者消失时连生产者一起删除。
- **补丁熔断**：同一模块出现第 2 个不相关补丁 → 停止实现并做架构 review，默认产出重构提案（重写前提见「重构姿态」的 B1 三条件）。
- **重构姿态**：发现设计缺陷时默认提议重构，不为绕行而绕行；但重构必须针对已观察到的摩擦（补丁熔断触发、变化轴被证实、一处真相被破坏），不为假想扩展性——与"不上重机制"同权重。无测试先补 characterization test；小步可编译可测可回滚地推进；涉及公开契约走 parallel change，迁移完成后同一提交或紧随提交删旧路径，不留双轨。不改公开契约沿用 B2 豁免，但需给出行为等价证据（测试/日志/验收命令）；改契约走 B2。重写受 B1 三条件约束，重写期间旧路径冻结。
- **禁止自等待死锁**：任何"内部命令通道 + 带 ack 的 oneshot"模式，命令处理逻辑本身向同一通道发命令时只能 detached fire-and-forget，禁止等待 ack；命令队列只能在处理间隙 drain，否则必死锁。micbot 的异步完成通知路径已经踩过这个坑，本项目沿用同一形状时要留意。

## Rust 约定

- 优先使用 `tokio` / `serde` / `thiserror`（库内）/ `anyhow`（二进制收口），不造同类轮子；Web 前端资源嵌入用 `rust-embed`。
- 注释只写非显而易见的原因；特殊注释使用 `TODO` / `FIXME` / `NOTE` / `HACK` / `WARN` / `SAFETY`，unsafe 必须有 `SAFETY`。
- 测试与实现分文件：实现文件只声明 `#[cfg(test)] #[path = "<name>_test.rs"] mod tests;`。默认不读 `_test.rs`，除非修改测试、排查失败或用户要求；不是任务需求不新增测试。

## Semantic Blueprint Protocol

Blueprint → Implementation 不可跳；Brainstorm 按需前置。Human 决定方向、公开契约和副作用边界；AI 探索、起草并在批准范围内实现。

- **B1**：存在多个真实候选，或架构、所有权、依赖方向、关键未知项未收敛 → 记录候选、权衡和未知项到根目录 `todo.md`，不写实现。多实体、并发协调或多实例需求不得跳过；依次产出实体/写者清单、各实体状态图、1-2 个关键场景 sequence 和必须成立的不变量，必要时补进程级 C4。
- **B2**：新增 crate、模块、跨模块/crate 依赖、公开 trait、类型、签名或错误枚举 → 创建 `docs/blueprints/<topic>.md`，写清签名、错误、强类型契约、副作用和依赖方向。枚举全部调用方并逐项判兼容；一个功能改多处公开契约时合成一份跨模块 Blueprint。**用户批准前禁止实现。** 方向未收敛则退回 B1。
- **B2 豁免**：不改公开签名的 bug fix、配置、日志、注释和私有重构。
- **B3**：按本文件审查 Blueprint 或实现。当前改动缺陷可在既定契约内修；既有架构缺陷或新方向选择交回 human，必要时退回 B1/B2。
- **B4**：严格按已批准契约实现；Blueprint 缺项或矛盾 → 立即停手报告。完成后说明端到端事件/状态变化和人工验收方法。
- **关闭**：fmt、clippy 和相关测试通过后，将 Blueprint 标记 `CLOSED` 并更新根目录 `todo.md`。

## Codex Review

- Codex 只找缺陷和挑战假设，不替 human 拍方向。
- 多文件 review 默认后台运行；review gate 默认关闭，仅在高风险且 human 全程监督时临时开启。
- 每条 finding 标记 `accept` / `reject` / `needs-investigation`，reject 必须说明理由；同时分类为"本次改动缺陷"或"既有架构缺陷"。禁止无脑采纳。
- 方案选择、架构方向和既有架构缺陷交回 human，不得在 B3/B4 顺手修。review-fix 超 3 轮未收敛，或连续两轮只有新长尾 → 记录原因和根因假设到根目录 `todo.md`，退回 B1。

## 提交

- 格式：`type(scope): 中文动宾短句`；正文说明动机和影响。
- B1/B2/B3 讨论、Blueprint 修订和 review-fix 中间态不提交；结果完成后按可独立理解、验证、回滚的状态组织，文档/实现/测试共同表达一项变更时可合并。禁止创建明知构建或测试失败的提交。
- 已批准但明确延期的 Blueprint 可独立提交；搁置方案只提交有长期价值的最终结论。除非用户明确要求，agent 不自行 commit。

## 依赖不变量

真实依赖以 `cargo tree` 为准；对应的 `tests/arch_deps.rs` 待 mic-core 落地时一并建立，守以下约束：

- `mic-message` 是 L0：零内部依赖，只被依赖。
- `mic-store` / `mic-tool` 只依赖 `mic-message`，互相不依赖，也不依赖 `mic-core`。
- `mic-core` 依赖 `mic-message` + `mic-store` + `mic-tool`；`mic-core` 不反向依赖任何 Channel crate。
- Channel crate（`mic-channel-web` / `mic-channel-dingtalk`）依赖 `mic-core`；`mic-core` 不知道具体 Channel。
- 依赖不得成环。新增跨 crate 依赖先回 B2；`lib.rs` 只 re-export 公开 API，内部默认 `pub(crate)`。

## 排障与维护

- 日志路径、级别约定待运行时骨架落地后在这里补齐。
- 规则被真实争议引用后再挂 BP 编号；长期无引用、无约束价值或已被其他规则覆盖的条目删除。

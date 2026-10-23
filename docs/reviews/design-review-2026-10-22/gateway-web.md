# Gateway Web 与装配层设计审查

后续实施进度（2026-10-23）见 [README](README.md#实施进度2026-10-23)。本文保留原始审查证据与当时的行号，不代表这些问题仍未处理。

日期：2026-10-22。按本次更新后的项目规则审查当前实现，聚焦用户操作路径、事实关联和额外复杂度。

## 覆盖范围

阅读 Gateway 的路由与鉴权、会话及消息接口、SSE 交接、票据、设置与备份、模型、任务与推送、Trace、SQL、日志、自更新；Web 的 API 解码、会话与全局流、列表状态、聊天与工具呈现、任务与通知、更新与日志状态；装配根的模块注册、日志、停止和 exec。前端通用 UI 组件和第三方代码不作逐行质量审查。

## 已确认缺陷

### G1 P2 自更新构建失败后再次点击会误报已是最新

触发场景：运行中的二进制为 A，上游为 B；第一次更新 reset 到 B 后构建失败。修复构建条件后再次点击更新，上游仍为 B。

`crates/mic-gateway/src/update.rs:211` 从部署目录的 HEAD 读取 from；`:227` 在构建之前就 reset 到 upstream；`:230`—`:234` 仅以此次源码 HEAD 是否变化判断 UpToDate。第一次失败会留下源码 B、进程 A，第二次 from/to 都是 B，于是跳过构建并返回“已是最新”。构建后的重启通知失败也有同类路径。

这条路径由当前代码直接成立，未在实际部署仓库执行更新。`docs/blueprints/archive/self-update.md` 同时规定“失败后可再次点击”和“to == from 则结束”，实现遵循了后者，缺口在契约对失败重试的组合行为。人工终端构建仍可恢复；不能将影响扩大为服务不可用。

整理方向：明确区分当前运行版本、源码版本和本次目标，不把源码没变化等同于运行版本已更新。当前已有构建期 COMMIT 和内存任务阶段，先基于它们确定重试行为，不增加部署数据库、版本快照或自动回滚。需修订现有 B2 的可观察行为；直接 cargo build 时 COMMIT 为空的行为也须在同一契约说明。

### G2 P2 上游跨轮复用工具调用 id 时历史结果会串到另一轮

触发条件：同一会话的两次模型回复携带相同工具调用 id，各自执行产生不同结果。当前 Provider 原样接受并保留这种输入；本项不声称官方服务实际一定复用 id。

- `crates/mic-provider-openai/src/chat.rs:431`、`responses.rs:478` 将上游 id 原样写入 ToolCall，没有建立全会话唯一性。
- `crates/mic-core/src/run.rs:588`、`:735` 原样执行并保存 ToolResult。
- `web/src/components/MessageList.svelte:14` 建立覆盖整个已加载会话的 Map，仅用 tool_call_id 作键；后一轮结果覆盖前一轮。
- `web/src/components/ReplyView.svelte:60` 对两条回复都用相同字符串查结果，因此前一轮也展示后一轮输出。

本地用 Bun 从当前 MessageList 提取实际 results 归约函数，去掉 TypeScript 类型后输入两条 id=call_0 的 ToolResult：Map.size=1，第一次回复的查询结果是 second result。没有向仓库追加测试或修改业务代码。该复现验证归组错误，没有模拟真实服务商网络。

整理方向：先明确工具调用身份的作用域。内部事实应能区分“哪次模型调用中的哪个工具调用”，上游 wire id 保持协议职责；或者在入站边界统一建立可用的内部唯一身份，再一致地用于执行、恢复和呈现。避免只在 Web 拼一个临时键，却让恢复与存储继续各自猜关联。模型与存储关系需要结合 core 审查决定，涉及公开类型、持久化或 Provider wire 映射时走 B2。

## 结构摩擦与减复杂度候选

### R1 会话来源字符串同时承担业务种类和能力判断

Gateway 的 `api.rs:139` 将 Root.channel 与 Triggered.module 都投影到 SessionItem.channel；`writable` 另行判断。Web 的 `ChatView.svelte:25`、`session.svelte.ts:99` 再以 channel == schedule 隐式推导是否显示人设、是否加载后台任务；`get_session` 也用该值决定查任务关联。

当前没有证明这些分支产生错误，但“入口来源”“触发模块”“页面允许的操作”已经被同一个字符串串联。下一个修改者必须掌握散落在前后端的约定，这正是新增功能容易逐层补分支的地方。

局部整理应先列出现有消费者的真实语义，明确会话种类及来源关系，由 Gateway 投影 Web 所需信息，保留合理的页面展示策略。不要为每个条件添加一个 bool，也不要建立通用权限框架；应通过少量明确种类和已有事实自然表达这些区别。调整 SessionItem 与前端解码需 B2。

### R2 装配层存在没有当前消费者的可选路径

全仓库只有 `bin/micnext/src/main.rs:108` 构造 GatewayModule，而且总是立即调用 with_restart_requests。`mic-gateway/src/lib.rs:41`—`:62` 仍为未接线状态保留 Option 和第二阶段构造；`service.rs:60` 再处理“启用了更新但缺少重启通道”。

这不是故障：当前启动检查能准确报错。它的保护对象是仓库内没有出现的装配方式。若后续整理 Gateway 装配，可以在构造时要求 restart sender，删除 setter、可选状态和缺线分支；更新是否启用继续只由 update_repo 决定。需修改公开构造签名，走 B2，收益较小，不值得单独为它开展大重构。

### R3 模型默认值有仓库内无读者的独立 GET 接口

`mic-gateway/src/models.rs:269` 的 GET /api/models/default 返回与 GET /api/models 已有 default_model_id 重复的事实；`web/src/api/client.ts` 只使用后者，单独默认值路径只有 PUT 消费者。未找到仓库内调用独立 GET 的代码。

若 human 没有依赖该接口的外部脚本，下一次模型接口整理可删除独立 GET、DefaultView 和路由的 get 分支。它没有造成当前错误，删除也不是统计单一来源问题，只是减少无消费者公开表面；应随相关 B2 处理。

## 保留的必要边界

- Web 对 HTTP/SSE 数据逐字段解码是外部输入边界；内部状态直接使用已解码类型，没有理由为了减代码删掉边界校验。
- 图片按 session 限定归属、一次性 SSE 票据绑定目标与有效期，均有实际调用场景。
- 会话 SSE 先订阅再回放、用持久化 id 去重，日志流按保留窗口重放，职责不同；不建议为了形式统一抽通用流框架。
- 各页面刷新逻辑存在相似代码，但分页深度、标题局部合并、未读提醒等语义不同。未发现需要引入前端状态管理框架的证据。
- 多份简单时间函数不是当前高收益重构点；为合并几行代码向底层 crate 增加不自然公共工具接口，可能增加维护成本。

## 验证范围

G1 已在 `/private/tmp/micnext-design-review-execution-20261022` 副本向真实 `Updater` 的私有测试模块追加临时用例，使用两个临时本地 Git 仓库作为部署仓库与 upstream，无网络。首次从源码 A 更新到 B，受控 `build.sh` 返回失败，断言阶段为 `Failed(Building)`、源码 HEAD 已是 B、构建调用次数为 1；随后解除构建失败条件，在同一个 Updater 上再次执行，实际阶段为 `UpToDate(B)`，构建次数仍为 1，模拟旧产物 A 未被替换，未发送重启请求。测试 `update::audit_tests::audit_failed_build_retry_is_reported_up_to_date_without_building` 通过。命令：`cargo test --offline --manifest-path /private/tmp/micnext-design-review-execution-20261022/Cargo.toml -p mic-gateway audit_failed_build_retry -- --nocapture`。此项证明当前更新控制流，未进行真实部署或进程 exec。

G2 已对当前前端归约代码运行本地 fixture。页面观感、真实网络、生产数据库和实际自更新没有操作。业务代码未修改，已有并发工作区改动保持原样。

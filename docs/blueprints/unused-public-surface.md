# B2：收缩无消费者入口与冗余关系

状态：2026-10-22 human 批准，按 Store、装配、Gateway 分步实现。范围为仓库内当前调用方，不承诺保留仓库外扩展入口。

来源：[存储 R2、E1、E3](../reviews/design-review-2026-10-22/storage.md)、[Gateway R2、R3](../reviews/design-review-2026-10-22/gateway-web.md)。

## 契约与删除范围

### Store

- 删除公开方法 `bind_identity`、`set_pwd`、`sessions_with_unclaimed_input`。保留当前 `resolve_identity`、创建会话时设置/继承 pwd、wake + claim_next 调度主路径；没有兼容转发，也不新增全库补跑扫描器。
- 删除 `core_jobs.report_message_id` 和 finish_job 中只为此列执行的 UPDATE；Completion 的消息插入、任务终态条件更新仍在同一事务，返回值与提交后事件不变。`core_runs.report_message_id` 有 agent 交付读者，必须保留。
- core schema 新增下一版本的 DROP COLUMN 迁移。该列没有索引或现行读取方，删列不删除任何消息、任务、输出文件或其他关联。迁移失败回滚，不改变“running 才能收尾一次”的保护。
- `core_sessions.parent_tool_call_id` 和 `SessionKind::Task` 本次保留；其去留与[工具身份 B1](../brainstorm/tool-call-identity.md)一并决定，避免先移除身份事实再做关联迁移。

### 模块装配

- 删除 `mic_core::Activation`、`Module::activation` 和所有实现中的 Always 覆盖。
- 传给 Assembly 的模块一律 install；配置段存在时仍要求是表，不存在时传空表。未知模块配置、重复工具与 Provider 检查保留；各模块负责解析自己的配置。
- 微信仍由二进制 feature 决定是否加入模块列表；`update_repo` 仍是自更新启用条件。此清理不增加或移除任何当前已装配能力。

### Gateway

- `GatewayModule::new(logs, restart_requests)` 构造时必须传入重启 sender；删除 `with_restart_requests`、模块与 Gateway service 中对应 Option、启用更新时“缺少通道”的分支。Updater 仍只在配置 update_repo 时创建，装配根负责停止与 exec。
- 删除 `GET /api/models/default`、`get_default` 与 `DefaultView`；**保留** `PUT /api/models/default` 和 `GET /api/models` 的 `default_model_id`。独立 GET 的调用方应改用模型列表；在路由匹配该路径时 GET 返回 405。没有仓库内 GET 消费者，不增加兼容 HTTP 入口。

Gateway R2（restart 通道可选）与存储 E3（Activation 可选装配）是两个独立删除项：虽然在同一份 B2 处理，不作为重复发现合并计数。

## 调用方与实施顺序

| 模块 | 同步修改 |
|---|---|
| mic-store | 三个方法定义、jobs 收尾、schema 迁移；核对外键与消息保留 |
| mic-core | Module trait、导出与 Assembly 安装路径 |
| 六个实现模块 | 删除 Activation 导入与覆盖；install 实现保留 |
| mic-gateway / bin/micnext | Gateway 构造、service、默认模型 GET 路由和 handler |
| 文档 | `archive/mic-store.md`、`archive/mic-core-module.md`、`archive/model-settings.md`、`archive/self-update.md` 标注修订 |

批准后分为 Store、装配、Gateway 三个独立可编译提交；schema 版本按实施时顺序取值，避免与快照蓝图冲突。没有前端形状变化或依赖新增。

## 验收与批准边界

- 真实迁移含工具任务与 Completion 的旧库，确认删除列，消息、任务终态、agent run 交付关系及 foreign_key_check 正常；重复 finish 不重复回注。
- 功能验证：缺配置的已传入模块被安装；非表和未知配置仍拒绝；有/无 update_repo 均能按既有规则装配，构造期必传 sender。
- 模型列表仍带默认值，PUT 默认值继续工作，独立 GET 不再存在；全仓库检索没有旧公开入口调用。
- `cargo fmt`、`cargo clippy -- -D warnings`、Store/Core/Gateway 相关功能测试。
- 批准表示同意移除这些仓库外可能使用的 Rust 扩展点和独立 HTTP GET，以及无读者的 report_message_id 关系副本；不授权删除用户消息、任务或文件。

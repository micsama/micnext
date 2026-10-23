# B2：压缩只保留必读文件路径

状态：CLOSED（2026-10-23 human 页面与生产验收通过；2026-10-22 human 批准）。

来源：[执行审查 R1](../../reviews/design-review-2026-10-22/execution.md#r1文件快照建立在已格式化工具文本上增加了跨层约定和重复数据)、[human 决定 1](../../reviews/design-review-2026-10-22/README.md#human-决定2026-10-22)。

## 行为与契约

压缩保存摘要和继续工作需要读取的路径，不自动读取文件。下一轮模型需要内容时调用普通 `read`，看到当时的文件内容；路径不是“压缩时内容仍然可恢复”的承诺。

- `mic_message::Compaction.files` 从 `Vec<FileSnapshot>` 改为 `Vec<String>`；字符串是模型结尾 `<required_files>` 块解析所得的路径，保留首次出现顺序、去重。字段名保留，语义改为必读路径清单。
- 删除 `FileSnapshot`、`FileSnapshotOutcome`、`SnapshotExtent` 三个公开类型。`mic_store::ContextWindow.files` 同步改为 `Vec<String>`。
- `compaction::parse_summary` 继续只解析正文结尾的完整块；没有该块时清单为空。路径作为模型输出的文本保存，不访问文件系统、不检查文件存在、不将其视为已读取的授权。真正读取仍走 `read` 的参数边界。
- 删除 `read_snapshots`、`NoBackground` 及 `SNAPSHOT_FILES`、`SNAPSHOT_BYTES`；保留既有压缩请求预算与保留段策略，不新增文件快照预算或清单专属版本。
- `request::build` 在 context 的 Summary 部分加入一段必读路径清单，明确内容须按需 read；不再注入 `File snapshot at compaction time` 和冻结正文。
- Web 的 Boundary 解码、上下文接口和消息/SSE 返回使用字符串清单。`ContextBoundaryView` 展示“继续工作时按需读取”的文件路径，删除快照正文展开、截断与不可读原因分支。
- 手动压缩与自动压缩共用本路径；失败或停止仍不提交压缩边界。删除快照读取后，成功摘要到边界提交之间没有文件 I/O。

## 历史数据与提交点

新增一条 core schema 迁移（v21），将全部历史压缩 Boundary 的 `compaction.files` 对象数组转换为原顺序的 `path` 字符串数组，包括已撤回、已归档会话中的边界。其他消息、摘要、保留段关系、调用记录、时间和 id 不变。迁移在既有 schema 事务中执行；解码错误使迁移失败回滚，不按空清单继续。新写入只使用新形状，运行时不维持双格式解码。

**批准本 B2 包含明确同意删除历史快照中的冻结正文、extent 和 unavailable reason。** 保留路径，不删除用户磁盘文件、会话、消息或原始工具结果。历史请求重建以后使用路径清单，不能再逐字还原曾注入的文件快照；这是删除副本的已知影响。若需要保留历史冻结正文，应先改本契约，不能在实现里另建备份表或偷偷保留第二份。

边界落盘仍沿用现有事务与提交后事件发布；重启不执行补读或重做压缩。

## 调用方与文档

| 范围 | 修改 |
|---|---|
| mic-message | `Compaction` 字段类型、删除快照公开类型 |
| mic-store | `ContextWindow`、schema 迁移、历史消息读取验收 |
| mic-core | `compaction.rs`、`request.rs`、limits 及相关功能测试 |
| Gateway / Web | 返回形状随类型变化；`api/types.ts` 解码与 `ContextBoundaryView` 同步 |
| 历史蓝图 | `archive/context-compaction.md`、`archive/compaction-retention.md` 加修订指向本文 |

## 验收

- 内存库构造有 Read/Unavailable、空清单、已撤回边界的旧记录，执行真实迁移，再经 Store 读取，确认路径与其他字段保留，旧快照字段清除；损坏旧记录导致整个迁移回滚。
- 压缩功能测试：必读路径去重；不调用 `read`，包括文件不存在或未安装 `read`；生成请求只有清单，没有冻结正文；手动与自动路径一致。
- `cargo fmt`、`cargo clippy -- -D warnings`、相关 Store/Core 功能测试、`web/` 下 `bun run check`。
- human 页面验收：展开新旧压缩记录均只见路径；撤回仍正常。通过后关闭并归档。

## 实施验证（2026-10-22）

Store/Core 共 63 项功能测试通过，包含旧快照迁移、损坏记录回滚和不读取文件的压缩路径；Web check 零错误零警告。历史快照字段已由 v21 迁移转换，人工页面验收尚未执行。

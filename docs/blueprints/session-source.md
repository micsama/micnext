# B2：会话来源不再借用渠道字符串

状态：2026-10-22 human 授权，2026-10-23 实现完成，待页面验收。

来源：[Gateway R1](../reviews/design-review-2026-10-22/gateway-web.md#r1-会话来源字符串同时承担业务种类和能力判断)。

## 选择与契约

保留底层 SessionKind 与 writable 的现有权威，不增加能力布尔矩阵。Gateway 仅投影 Web 现在确实可浏览的两类来源，避免把触发模块名塞进 Channel 字符串。

SessionItem / SessionDetail 删除顶层 `channel`，新增：

```typescript
source: { kind: "channel"; channel: string } | { kind: "schedule" }
```

Root 投影为 channel；schedule 模块的 Triggered 投影为 schedule；Task 和其他当前不可浏览的 Triggered 继续按原约束不进入该接口。`schedule` 关联信息与 `writable` 字段保留；会话归档和写入权限仍在操作时检查，前端来源不替代授权。

Web 的定时任务标题、人设选择器与 Jobs 加载策略按 source.kind 选择；来源文案统一由一个纯展示函数生成，不再用 `channel === "schedule"`。Root 的列表查询 `?channel=` 仍只表示真实渠道，与 Trace 自己已有的 SessionKind 投影不混用。

HTTP 会话列表、创建会话响应、详情与 Web 解码同步；无数据库迁移、不改定时任务可追问策略、模型设置或人设冻结规则。旧客户端需刷新，与本仓库严格解码约束一致。

## 验收

Gateway 投影功能测试覆盖 Web Root、其他渠道 Root、schedule Triggered（包括任务关联已删除）与不支持的来源。Web check 覆盖所有调用方；人工检查 Web/微信/定时任务标题、只读说明及人设与 Jobs 展示。cargo fmt、cargo clippy、Gateway 测试通过后提交，页面验收前不关闭蓝图。

## 实施验证（2026-10-23）

Gateway 功能测试覆盖真实 channel 名为 schedule 的 Root 与 schedule Triggered，确认二者不会混为一类；也覆盖其他来源拒绝投影。Gateway 2 项测试、工作区 clippy、Web check 通过。

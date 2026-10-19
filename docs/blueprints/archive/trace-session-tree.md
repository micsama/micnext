# B2：Trace 会话树与整树用量

状态：CLOSED（2026-10-19 human 验收通过）。修订 [async-jobs](async-jobs.md) P3.2 的 Web 呈现部分；后端整树汇总口径不变。

## 动机

P3 把子会话当成“列表之外的会话”，于是入口零散补丁化：中栏顶部「子任务」块、聊天任务面板图标、详情返回栈；进入子会话后左栏无高亮、标题退成「会话 N」、看不出父会话，直链无法回到父会话。用量范围开关挂在详情头部，实际只管 Token 卡，且无子任务时也显示。

根因是 trace 会话列表只认 Root。改为一棵按 `parent_session_id` 组织的会话树，子会话与 Root 同等可见，其余入口和开关随之消失。

## 一、会话列表接口

`GET /trace/sessions` 返回所有有已结束 run 的会话（不再限 Root），扁平数组，前端按 `parent` 建树。父会话自身无已结束 run 但有符合条件的后代时也返回，保证树完整。

```ts
type TraceSession = {
  session: number;
  kind: "root" | "task" | "triggered";
  parent: number | null;      // parent_session_id
  label: string | null;       // root：会话标题；task：job description；triggered：留给定时任务 B2
  job: number | null;         // task 对应的 agent job id
  channel: string;            // 沿用：Root 取自身，后代取根会话（与 kernel 的会话所属 Channel 一致）
  last_run_at: number | null;
  failed_runs: number;
  interrupted_runs: number;
  tool_errors: number;
};
```

- `title` 字段改名为 `label`：子会话没有标题，名字来自任务说明，一个字段承载“列表里显示什么”。
- 统计（失败、中断、工具报错）只算会话自身，不汇总后代。
- 扩展：定时任务等新来源只需新增 `kind` 取值，树与接口形态不变。

调用方：Trace 左栏、`traceSessions.title()`、看板若引用了 `title` 一并改名。

## 二、用量接口

`GET /trace/sessions/{id}/usage` 去掉 `include_children` 参数，一次返回两份：

```ts
type SessionUsageView = {
  own: SessionUsage;                                        // 本会话全部已记录调用
  tree: { descendants: number; usage: SessionUsage } | null; // 本会话 + 全部后代；无后代为 null
};
```

`SessionUsage` 字段与口径沿用 P3.2（含重试、压缩、标题；递归含归档、隐藏后代；每条调用只算一次）。Store 的 `session_usage(session, include_children)` 改为一次查询返回两份，避免前端两次请求。

## 三、Web 呈现

**左栏会话树**
- 按 Channel 分组不变；组内 Root 按最近运行倒序，子会话缩进挂在父会话下（左侧竖线），按 job id 升序，前缀 bot 图标，悬停显示「子任务 #id · 描述全文」。
- 有子会话的父会话带折叠箭头。默认收起；当前选中会话所在分支自动展开。搜索命中子会话时保留并展开其父会话。
- 父会话只标自身异常；收起时若后代有异常，只加一个弱化的关联图标，悬停列出是哪些子任务。
- 底部提示「仅列出有已结束运行的会话」保留。

**中栏**
- 删除顶部「子任务」块。
- 子会话的标题区为面包屑「父会话标题 › 子任务描述」，父会话可点击，不依赖返回栈。

**右栏会话详情**
- 删除「用量范围」分段控件。
- Token 卡显示 `own`；`tree` 非 null 时卡底附一行「含 N 个子任务合计 X tok · ≈$Y」，悬停显示整树的输入/输出/缓存明细与未知次数。
- 子会话的会话信息不再只写一句提示：显示「子任务 #id」与任务说明；人设、渠道等 Root 字段不显示。
- 「关联」沿用详情既有样式：父会话列出各子任务（带自身异常标记），子会话列出父会话，点击切换会话。

原型：`tmp/trace-preview/index.html`（2026-10-19 版，旧版对照 `tmp/trace-preview/index-1014.html`）。原型里的「定时任务」分组属于后续定时任务 B2，本文不实现。

**聊天任务面板**
- agent 行的 Trace 入口改用含义明确的「在 Trace 中查看」图标（`arrow-up-right` 类），不再复用侧栏 Trace 的 sparkles。

## 四、不变

- 子会话仍不进聊天侧栏、无聊天入口；Trace 只读。
- run 列表、聊天 run-stats、单次调用费用、上下文占用不受用量汇总影响。
- 不新增配置项与设置项。

## 五、文档同步

实现时同步 async-jobs P3.2 第 228 行、run-trace §六接口、web-ui 中对应描述。

## 验收

- 自动：`cargo fmt`、`cargo clippy -- -D warnings`、`web/` 下 `bun run check`；本地库核对 `/trace/sessions` 含子会话与 `parent`，`usage` 的 `tree` 合计等于各会话 `own` 之和。
- human：会话树观感、折叠与搜索、子会话面包屑往返、Token 卡合计行、窄屏。

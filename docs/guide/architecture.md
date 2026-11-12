# 架构与主流程

本文讲模块之间怎么协作，以及维护时绕不开的约束。各项能力的边界见 `capabilities.md`，具体文件见 `code-map.md`。

## 进程与分层

micnext 是一个进程，一个用户（person，所有者 `dzmfg`），一个业务 SQLite。

```text
bin/micnext（装配根：读配置、组装模块、信号、-p、更新后 exec）
  └─ mic-core Assembly + Kernel
       ├─ 实现层模块（彼此不依赖）：
       │    mic-gateway（HTTP + Web）  mic-channel-wechat
       │    mic-provider-openai（模型）  mic-tool-fs / -shell / -web-fetch（工具）
       └─ mic-core → mic-store / mic-tool / mic-media → mic-message
```

- **mic-message**：消息类型，以及历史在模型请求里的呈现方式（`model_view`）。它是纯数据，没有 I/O。
- **mic-store**：持久化所有业务事实，并提供模块事务入口。
- **mic-tool**：工具接口（port）。
- **mic-media**：图片识别与规整。
- **mic-core**：Kernel 句柄，以及执行主路径，包括：
  - 调度、run、压缩
  - 后台任务、子 agent
  - 推送、定时任务
  - skill、脱敏、杂活模型
- **实现层**：在 `Module::install` 里向 `Registry` 登记自己提供的东西，包括 service、provider、工具、channel 提示、内置 skill、schema。之后它们只通过 `Kernel` 访问内核。

**数据目录**：缺省是 `~/.local/share/micnext`，由 `[core] data_dir` 指定。

| 位置 | 内容 |
|---|---|
| `micnext.db` | 业务库，各模块按表前缀分开：`core_*`、`wechat_*`、`web_*`。schema 有版本号，旧库用 `scripts/db/convert.py` 转换 |
| `logs/` | 独立的日志库，和业务库分开 |
| `files/` | 入站文件，以及交给用户的文件快照 |
| `jobs/` | 后台任务输出 |
| `skills/` | 磁盘 skill |
| `python/` | 共享的 uv venv |
| `micnext.lock` | 数据目录独占锁 |

图片字节随消息一起存在库里。

## 流程一：用户发消息到看到回复

1. **入站**
   - Web 通过 `POST /api/sessions/{id}/messages` 进来；微信由 channel 的 service 收到消息，先解析出身份和会话。
   - 两者都调用 `Kernel::append_user_input`：校验图片和文件、展开 `/skill` 点名，以 pending 状态写入，再唤醒该会话的调度。
   - 微信入站里有不支持的类型或媒体失败时，调用 `append_recorded_input`：输入以 held 状态写入，和处置说明在同一个事务里，不唤醒执行。
2. **调度**（`scheduler`）：同一会话最多一个 worker，不同会话之间并行。worker 先通过 `RunControl` 拿到执行槽，再认领输入（`claim_next`），或者接续可接续的 run。
3. **执行**（`run`）：
   1. 普通对话 run 开始前，先做空闲整理（`tidy_if_idle`）。
   2. 组装请求（`request::build`）：系统提示、上下文窗口、图片、工具。
   3. 调用模型，失败时由 run 负责重试。
   4. 回复落盘后执行工具，每步结束检查是否需要压缩（`compact_if_full`）。
   5. 在每个模型调用边界，并入执行中新到的输入（`absorb`）。
4. **事件**：所有落盘消息都通过 `Events` 的 publisher 发布。Kernel 事件只在内存里广播，订阅方跟不上时收到 `Lagged`，按消息 id 重放。草稿增量（`TextDelta` 等）不落盘。
5. **呈现**
   - Web：会话 SSE（`/api/sessions/{id}/stream`）用 `subscribe_from_now` 取得切点，切点之前的内容走回放，之后的走实时事件，并带上当时的草稿。
   - 微信：投递循环读取 `pending_deliveries("wechat")`，分段发出，再标记送达或失败。

## 流程二：后台任务与子 agent

- **后台 `bash`**：登记为 job，立即返回 job id。输出写到 `jobs/<id>/`，只保留最近一部分。中途匹配到 `notify_pattern`，或者任务结束时，以通知消息回到发起会话，并唤醒它。
- **`agent`**：
  1. 在同一个事务里建好 job、子会话（Task）和第一条输入，然后唤醒子会话，由普通 run 执行。
  2. 子 run 结束后，把最后一条回复作为交付消息，写回父会话。
  3. 停止子 agent 时，会取消它的执行，并停掉它名下的工具 job。
- 管理工具（`jobs`、`job_output`、`job_stop`）的范围是调用会话及其后代。Web 后台任务条按会话树的根拉取。

## 流程三：定时触发

1. 调度循环（`schedule::run`）到点后，向任务会话（`Triggered { module: "schedule" }`）写入一条触发指令。指令带消息头 `[trigger src=… at=…]`，告诉模型这不是用户在问、用户也不在看。
2. 写入触发指令时，如果占用已超过 `SCHEDULE_COMPACT_PERCENT`，会在同一个事务里先排一次压缩，摘要按事项记录。之后唤醒任务会话，走流程一的 run。
3. 结局记在 fire 上：触发跳过、run 失败或错过触发，都通过框架推送告诉用户；连续失败会自动暂停。
4. 启动时，停机期间错过的触发合并为一条，只记录不补跑。

## 流程四：推送

1. `push` 工具或框架通知调用 `Pushes::send` / `notify`。
2. 在同一个事务里写入 `core_pushes`、各 IM 的投递行，以及目标 IM 会话里的通知消息。
3. 提交后发布 `PushCreated`。
4. 各个消费方：
   - **Web 通知中心**：通过全局 SSE（`/api/stream`）重新拉取。
   - **IM**：写进该 person 在该 channel 的私聊会话，走流程一的投递循环；投递结果记在推送的投递状态上（`PushDeliveryChanged`）。
   - **系统通知**：Gateway 的 `web_push::run` 订阅 `PushCreated`，逐个订阅发送一次，不重试。返回 404 或 410 时删除该订阅。

## 流程五：压缩

1. 触发来源有三种：自动（占用达到触发比例）、手动（`/compact`）、空闲时占用过半。
2. 用当前模型生成摘要。摘要提示要求列出 `<required_files>` 和 `<required_skills>`。
3. 写入压缩边界之后，上下文窗口从边界开始计算；边界之前的用户消息原文引用保留。
4. 压缩后，按 `required_skills` 重新加载 skill。
5. 撤回压缩就是撤掉边界，原来的历史仍在。

## 容易踩坑的不变量

- **一个会话一个 run**：执行槽在 `RunControl` 里，停止令牌也在那里。不要绕开调度直接执行。
- **先提交，再发布事件**：所有消息写入都经过同一把发布互斥，所以发布顺序就是提交顺序，消息 id 可以当作完整的前缀游标。新增写入路径也必须经过 publisher。
- **事件可以丢，事实不会丢**：订阅方不能依赖事件做持久状态，跟不上就按 id 重放。草稿只存在内存里。
- **模块只能动自己的表**：`Kernel::with_module_tx` 只允许访问本模块前缀的表，由模块测试把关。跨模块的业务事实通过 Kernel 接口获取。
- **启动收尾**：上次遗留的执行由 `recovery` 统一收尾，它运行在所有 worker 和订阅者出现之前，不发事件。不要在其他地方另做「重启补偿」。
- **结果未知的外部副作用不重放**：微信投递、系统通知、模型调用的重试都只有一处负责。投递中断记为「可能未送达」，不重发。
- **没有内置迁移**：二进制只认当前的 schema 版本。改表时要升版本，并在 `scripts/db/` 里加转换 SQL。
- **出站脱敏只改请求副本**：本地原文不动；`plain_after` 控制会话从哪条消息起关闭正则脱敏。
- **子会话没有自己的归档标记**：归档标记只存在根会话上，执行边界沿祖先链检查。
- **用户可调项在 SQLite，构建期旋钮在各 crate 的 `limits.rs`**：`config.toml` 只放启动项。

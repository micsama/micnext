# 设计审查：Provider 与微信集成（2026-10-22）

后续实施进度（2026-10-23）见 [README](README.md#实施进度2026-10-23)。本文保留原始审查证据与当时的行号，不代表这些问题仍未处理。

状态：审查完成；未修改业务实现，未提交。以下是当前源码审查结果，不是已批准的实现方案。

## 覆盖与判定依据

- 完整阅读 `mic-provider-openai` 的请求映射、Chat/Responses 累积、SSE 驱动、错误分类、配置/凭据、模型探测及现有测试。
- 完整阅读 `mic-channel-wechat` 的协议 DTO/边界解析、账号/备份、登录、连接协调、入站、媒体落盘、引用、投递、typing 和 schema。
- 向上下游核对 Kernel 投递/订阅、Store 消息终态/诊断授权、Assembly 服务退出、Gateway 日志采集。
- 核对现行蓝图：`archive/provider-handwritten.md`、`archive/model-stall.md`、`archive/provider-sdk-responses.md` 相关规则、`archive/wechat-channel.md`、`archive/wechat-quotes.md`、`archive/wechat-inbound-images.md`、`external-error-observability.md`。
- 按当前设计规则判断：优先指出当前行为错误和已有事实无法组合的问题；协议严格拒绝未知字段、服务错误终止进程、微信至多两次发送、不保证恰好一次、重启不重放等均已有人批准，不当作本次缺陷。

严重度：P1 为正常操作导致数据丢失或主服务不可用；P2 为特定故障窗口导致状态不一致，或违反已有敏感信息隔离约定。

## 确认缺陷

### I1 · P1 · 引用成功接收的图片会使整个主服务退出

触发：微信成功接收一张图片（或图文混合消息），随后用户引用该消息并发送新文字。

证据与调用链：

1. `crates/mic-channel-wechat/src/service.rs:739` 的 `flatten` 将成功图片写为 `IncomingPart::Image`，不再是旧版图片文字占位；`service.rs:629` 起把引用前缀与原内容拼接，通过 Kernel 落盘，然后登记微信外部 ID 关联（`service.rs:655`）。
2. 后续引用由 `quote::resolve` 读这条正规消息；`crates/mic-channel-wechat/src/quote.rs:119` 遍历源内容，只处理 `ContentPart::Text`，其余在第 122 行报 `Invalid("引用入站包含未支持的内容类型")`。
3. `crates/mic-channel-wechat/src/service.rs:629` 用 `?` 传播错误；连接 worker/Coordinator 返回 Service Err；`crates/mic-core/src/assembly.rs:273` 经 `settle` 取消主服务。

影响：一个合法的图片引用被当成磁盘不变量破坏；本条新文字没有进入正规消息，Web、微信等常驻能力一起停止。

契约对照：`docs/blueprints/archive/wechat-quotes.md` §二明确要求图片引用为 `[引用：[图片]]`，新文字正常执行。后续图片实现更换了正规存储类型，却没有同步引用消费者。

简化方向：引用直接从正规 `ContentPart` 的内容类型组合出文字表示，`Image` 映射图片占位；不新增图片副本、不重新下载、不另建引用内容库。属于私有局部重构/缺陷修复；若同时扩展文件引用行为，先列清文件的既有契约。

### I2 · P1 · 附件临时路径会覆盖已有真实文件

触发：同一日期目录先保存 `report.txt.part`，再保存 `report.txt`。也可以是工作目录中本来就存在 `report.txt.part`。

证据：`crates/mic-channel-wechat/src/media.rs:210` 对最终文件用 `create_new(true)` 避免同名覆盖，但 `media.rs:221` 固定拼接 `target + ".part"`，第 224 行直接 `File::create`。该调用会截断已有临时路径；第 228 行又把它 rename 为最终名。`Cleanup` 还将这条未独占的 `.part` 路径作为自己的文件清理（第 246 行）。

影响：已存在的真实 `report.txt.part` 内容被截断并被移动；失败/取消也可能把它删掉。最终名去重没有保护临时名，与“同名不覆盖”的现有承诺相冲突。顺序收两份文件即可触发，无须并发。

简化方向：写入者只操作自己通过独占创建取得的路径，清理只拥有成功创建的文件。可以先核对是否存在“完成前必须不可见”的真实消费者；若没有，直接写已独占的目标并在失败/取消时清理即可删除 `.part`、rename 和第二条清理路径。若确有原子发布要求，则独占分配临时路径。无需引入通用存储框架。

### I3 · P1 · 投递成功后的崩溃窗口会让下一次启动失败

触发：微信 `sendmessage` 已返回成功，Kernel 已写 `delivered_at`，进程在写微信模块 `outcome='sent'` 前退出。

证据与调用链：

- `crates/mic-channel-wechat/src/delivery.rs:209` 先调用 `kernel.mark_delivered`，第 210 行再调用 `finish(...Sent)`；两个独立事务。
- 下一次启动 `service.rs:210` 先执行 `delivery::recover`。`delivery.rs:394` 把该行仍为 `sending` 的消息选出；第 406 行先改本地 `interrupted`，第 411 行再对它调用 `mark_delivery_failed`。
- `crates/mic-store/src/messages.rs:215` 无条件写 `delivery_failure`；`crates/mic-store/src/schema.rs:82` 明确约束 `delivery_failure IS NULL OR delivered_at IS NULL`。此时已有 `delivered_at`，因此 SQLite CHECK 失败，微信 Service 启动返回 Err。

影响：本来已送达的消息被 recovery 当成未知失败，导致第一次重启失败。由于 recovery 已先提交微信 `interrupted`，再次启动不再选中它，表现为一次启动故障且留下矛盾的模块状态。

同源窗口还有两处，严重度 P2：

- 失败路径先 `finish(...Skipped)` 再 `mark_delivery_failed`（`delivery.rs:190`、196）；两者之间退出后，模块已终态但 core 永久待投递，后台推送会显示不正确的待投递状态。重启切点和 `stored` 排除让它不会再次发送；不是重复发送问题。
- recovery 自身先把全部 `sending` 转 `interrupted`，再逐条写 core 失败（`delivery.rs:404`–412）；中途退出后，剩余消息不再被后续 recovery 选中，core 失败状态无法补齐。

简化方向：同一投递结果的公开事实需要一个提交点。优先考察让 core 的送达/失败终态成为权威，微信只保留发送计划和逐段尝试事实，去掉重复终态判断；或设计一次完成两个必要写入的受控事务契约。当前 `Kernel::with_module_tx` 只允许模块前缀表（`kernel.rs:1148`），不能通过微信直接写 core 表绕过。涉及 Kernel/Store 契约时需 B2，不能只把两次写入交换顺序。

### I4 · P2 · 微信协议调试日志遗漏媒体凭据遮蔽，完整响应策略的新旧契约冲突

触发：一次二维码、getupdates 或媒体响应带新增字段/字段类型不匹配，进入 `decode` 错误路径。

证据：`crates/mic-channel-wechat/src/client.rs:650`–662 记录 serde 错误，并把 `redacted(&body)` 全文写入 DEBUG。第 667 行的密钥名单只有 `bot_token`、`context_token`；`wire.rs` 还明确接收 `aes_key`、`aeskey`、`encrypt_query_param`、`full_url`、`qrcode`、`qrcode_img_content`、`typing_ticket` 等。错误响应里的这些值及完整消息正文会保留。第 656 行的 serde 错误本身也可能带原始非法值。

影响与范围：`crates/mic-gateway/src/logs.rs:143`、265 表明项目 DEBUG 被常驻开发者日志内存采集，界面可以查看；**DEBUG 不按默认 INFO+ 规则落盘**，不夸大为每次都会写日志数据库。WARN 的 serde 错误则满足持久化等级。这是已有本机/鉴权开发者界面的敏感信息隔离问题，不主张未经鉴权的远程泄漏。

历史许可与当前冲突：`docs/blueprints/archive/wechat-channel.md:205`–206 **明确批准过例外**：“响应解码失败时，debug 级别输出原始响应体供协议对齐（可能含 token 与正文，仅调试时开启）”。随后提交 `b76bbea`（2026-10-10）明确记录“DEBUG 常驻采集后，仅 debug 输出不再构成隐藏”，并刻意改为遮蔽 `bot_token/context_token`、保留消息正文。故不能把完整正文调试本身描述为从未得到批准的行为。当前缺陷是新增媒体密钥/二维码等敏感值未继承这次遮蔽约束；另有较新的 `docs/blueprints/external-error-observability.md:7` 明确要求不记录完整请求/响应和密钥，最新 AGENTS 也禁止完整敏感载荷，而旧蓝图例外未同步修订。

简化候选：收敛为 endpoint、字段路径、JSON 错误类别/位置、长度等诊断信息，可删掉完整响应调试与递归字段黑名单，避免为新凭据名持续打补丁。若仍需保留原始协议对齐能力，由 human 在上述新旧契约间明确取舍；可以选择显式采样操作及其保存边界。**本审查不自行取消旧的明确许可，也不把“必须删除所有原文调试”当成纯代码缺陷修复。**

### I5 · P2 · 入站快照内的 context_token 可绕过诊断凭据遮蔽

触发：图片/附件下载期间查询 `wechat_inbound_batch`，或中断留下 batch 后查询 `snapshot`/用 `json_extract` 读取其中 token。

证据：

- `crates/mic-channel-wechat/src/account.rs:307`–318 把完整 `GetUpdatesResponse` 序列化写到 `snapshot`，其中包含每条消息的 `context_token` 和媒体密钥。
- 成功批次删除；中断批次在 `account.rs:374` 仅改 `interrupted`，快照继续保留。
- `crates/mic-channel-wechat/src/schema.rs:64`–73 的 `secret_columns` 只登记 `wechat_account.bot_token`、`wechat_state.context_token`，未登记快照。
- `crates/mic-store/src/diagnostic.rs:96`–111 只按登记的表/列遮蔽，其余任意只读列允许查询，因此 `SELECT snapshot FROM wechat_inbound_batch` 或对它做 JSON 提取会得到本应隐藏的 token。

影响：同一个凭据在正规状态列读作 NULL，在快照副本中却可见；中断后存在持久化暴露面。不是反对保留已批准的中断现场，而是副本未继承凭据边界。

简化方向：先明确“中断现场审查”真正消费的业务事实；不再执行/重放的 snapshot 是否还需要凭据、下载密钥，需要 human 取舍。最小修复可以登记整列为敏感列；如果仍要诊断内容，应从明确无敏感字段的审查事实读取，不在通用 SQL 层猜 JSON 键。改变已批准的完整快照契约需要 B2。

## 结构摩擦与局部重构候选

### R1 · 从事实推导引用，消除旧占位假设

I1 表明 `source_start` 关联事实本身已足够，但消费者仍假定“入站一律是 Text”。当前扩图片时必须记得去修看似无关的引用读取。整理 `source_text` 对内容类型的映射即可让正文、图片自然组合；不新增媒体标记、副本或新表。现行契约内即可完成，优先级高。

### R2 · 从尝试事实推导投递结果，减少双状态机

当前 `wechat_delivery.outcome` 和 core 的 `delivered_at/delivery_failure` 表达同一投递终态；本地 `Outcome` 的四种值在 `delivery.rs:83`–86 的 match 中最终完全同样处理。启动修复、一次性 v4 迁移、运行期跳过由不同路径维持一致性。I3 是已经付出的代价。

下一次投递变更应一起整理：保留微信确实需要的原始分段计划、attempt、外部消息 ID；终态归属明确到一处。可删掉重复 outcome 判定和全计划校验式的 pending 排除，减少终态同步补丁。不能为了去重删掉 plan：它确实被引用功能用来还原用户实际收到的发送段。跨 core/模块的取舍需 B2。

### R3 · 对“只用于审查”的快照停止复跑业务解析

`account::recover_batches` 把上次导入未完成的全 snapshot 重新完整解码并执行 `client::parse_updates`，然后只把批次标 `interrupted`；不会导入、重放，也不消费解析所得 `Updates`。当前代价是进程启动受无后续执行用途的媒体/引用解析规则影响，磁盘保留大量凭据与外部字段，又需要在每次上游字段变动时维持一份全量 wire schema。

候选：中断事实保留批次身份、游标/时间、已导入项关联及必要的失败描述；确实需要完整样本时另列明确消费者。可删掉 recovery 的业务重新解析和无用途字段副本。**这是现行完整快照/严格边界契约的调整候选，不把已批准方案直接判作缺陷**，须人判断收益后 B2。

### R4 · 单一外部错误消费边界

微信 `network_error` 先带完整根因记录一条 WARN，上层 inbound/delivery/media 又分别记录固定枚举文案（例如 `service.rs:540`、`delivery.rs:166`、`media.rs:176`）。当前一次 CDN 失败/重试可产生多条散开的日志：根因有 endpoint，业务条目有 attempt/session/message，但没有自然关联。

候选：当前调用上下文通过少量明确字段或 tracing span 关联，让保留根因的那条日志同时具有必要业务身份；上层只在状态转换时记录一条摘要。可删掉只重复固定错误文案的日志。无需统一所有 crate 的错误框架，私有实现可局部整理。

### R5 · 投递排除只需要存在事实，却解码并复验整份历史计划

这是可独立先做的小范围删减：`delivery::stored`（`delivery.rs:256`–271）查询 `outcome, plan`，反序列化全部分段并逐段检查非空和字符数，再解码 outcome；唯一调用者（第 81–88 行）对四种 outcome 做完全相同的动作——推进游标并跳过。`schema.rs:41` 已有 outcome CHECK；`plan()` 是该表计划的唯一生产者且写入 `TEXT_CHUNK_CHARS` 长度的分段。

当前保护场景只是“内部写入的计划被手工改坏/磁盘损坏”，但执行排除根本不使用计划内容。这让纯粹确认已处理的流程依赖不相关历史正文的完整解码，增加 SQL 载荷、遍历和无收益失败分支。

简化方向：这里查询行是否存在即可。可删掉此处 Plan 解码、完整长度复验、四分支同体 match，`Outcome` 的 `Sending/Interrupted` 解析用途也可一起消除。真正需要读取实际段文字的 `quoted_text` 仍须正确解码 Plan 和处理段索引；不建议把 JSON 解析错误一律忽略。`recover` 同样只是标中断，不消费计划，当前先完整解码计划的循环也可删。仅私有实现变化，可随投递修复整理。

账号读取也有相似但收益较低的候选：`account::find` 对已由登录/备份边界 `required` 校验并由 `save` 写出的 user/bot/token 再查非空，读取关联 session 再逐项比对不可变的 channel/chat/target（`account.rs:124`–149）；`state` 再验非零 timeout/非空 token（第 266–273 行）。这保卫的是内部数据被改坏，没有发现仓库内可变更这些归属的普通入口。可以明确这些写入不变量后删去纯重复检查；保留必要磁盘解码，以及未来若存在会随操作改变的权限/归属检查。它不是建议关闭所有持久化解码，更不建议新增一层“验证过账号”trait。

### 已核对而不建议继续加机制的项目

- **没有业务 payload 双版本体系。** 当前 Plan 和 snapshot 不带独立 `version` 字段；微信只有 schema migration 的 3/4 版本。`channel_version`、Codex `client_version` 是外部协议必需参数，不是内部历史快照版本，不能混为一谈。
- **没有发现旧配置兼容分支需要清理。** `EndpointConfig`、`ModelConfig`、微信备份明确拒绝未知字段；Generic/Ollama/DeepSeek/Codex 方言有当前消费者。固定地址预设拒绝 `base_url` 是当前公开配置约束，不是无用途的历史兼容提示。
- **不为严格 wire DTO 单独提“容错层”。** 微信拒绝未知字段和缺失协议必需项是既有批准策略；若要从完整 DTO 缩到业务真正使用的字段，应与 R3 一并讨论收益，不自行添加 unknown/fallback 体系。
- **不机械拆 service/client。** 两者虽长，但目前分别承载协调流程和协议边界，不能仅按行数判缺陷。优先删掉 R3/R5 的无消费校验和 I4 的原文调试，再衡量实际剩余职责，避免先抽出更多模块把复杂度搬走。

### Provider 暂无需要立刻拆模块/建通用框架的证据

Chat/Responses 各自持有本协议的类型、请求和累积；共用传输/SSE 驱动；`Protocol` 与方言/后端区分有真实消费者。现状比引入一套通用可配置协议引擎更直接。配置从已校验 JSON 读取时只做解码，没有再普遍重跑 canonical 校验，符合内部互信方向。当前模型名特判有明确服务商差异，不因出现 match 就建议抽象。

## 验证记录与待验证

- `cargo test -p mic-provider-openai -p mic-channel-wechat`：沙箱内回环监听被拒；经批准在沙箱外重跑，微信 1 项、Provider 13 项全部通过，doc tests 通过。现有测试尚未覆盖 I1/I2/I3 场景。
- I1/I2/I3 已在 `/private/tmp/micnext-integrations-review-20261022` 的当前源码副本追加私有复现测试，**没有修改原工作区的业务源码/测试**。运行 `cargo test -p mic-channel-wechat -p mic-store review_repro -- --nocapture`，3 项均成功复现下表缺陷；测试断言的是“缺陷存在”，通过不代表实现正确。

| 问题 | 实际调用与观测 | 验证边界 |
|---|---|---|
| I1 | 生成有效 1×1 PNG，经实际 `mic_media::inspect`、内存 Store 的 `append_input`、`messages_after` 取得正规 `ContentPart::Image`，调用副本原有私有 `quote::source_text`，报 `微信磁盘数据无效：引用入站包含未支持的内容类型` | 实测正常落盘 Image 的引用函数错误；全服务退出由上文现有 `?`/Service/Assembly 调用链确认，未连接真实微信 |
| I2 | 两次调用副本原有私有 `media::write`，先写 `report.txt.part`，确认内容存在，再写 `report.txt`；第二次写入后，首个用户文件路径已不存在，新目标是第二份内容 | 真实临时目录和实际异步文件操作；不依赖手工模拟截断逻辑；测试结束清理其自身目录 |
| I3 | 实际当前 schema 的 `Store::open_in_memory` 创建 Notification，依次调用原有 `mark_delivered`、`mark_delivery_failed`，得到 `sqlite: CHECK constraint failed: delivery_failure IS NULL OR delivered_at IS NULL` | 实测与 recovery 相同的 Store 写入冲突；没有注入真实进程崩溃；两个提交点之间确有可退出窗口由源码确认 |

临时副本测试位于 `crates/mic-channel-wechat/src/quote.rs`、`media.rs` 和 `crates/mic-store/src/messages.rs` 的末尾 `review_repro` 模块，可重复运行。建议实施后把这些功能场景改成预期正确行为的正式验收；完整重启验收还应覆盖 I3 的各个提交点，不以现有协议映射测试替代。
- 微信发送两次同 `client_id` 的服务端去重能力仍未实测；当前至多两次且允许重复是批准策略，不在本次列缺陷。新设计规则“未知副作用不重放”与此已有明确例外应由 human 决定是否调整。
- Provider 工具 ID 透传与 Web 结果关联的跨层问题，统一见同目录 `gateway-web.md`，本报告不重复计数。
- 本报告仅增加审查文档，无 Rust/前端变更，因此不额外运行格式化和全工作区 Clippy；根审查可统一记录全量检查。

# B2：微信 Channel（v0b）

**状态：Phase 1 CLOSED；Phase 2–5 已获准并实现，本地检查通过，真实扫码与收发待服务器人工验收；Phase 6 未开始。**
起草日期：2026-10-08；阶段拆分与本次进度更新：2026-10-09。整份蓝图尚未 CLOSED，当前交接见 §11.5。

**来源**：[B1 决定](../brainstorm/wechat-channel.md)、[iLink 协议素材](../brainstorm/wechat-protocol.md)。
代码基线：`bca566c`（服务商/模型分层已落地）。协议类型核对官方包
`@tencent-weixin/openclaw-weixin@2.4.9` 的 `src/api/types.ts`、`src/auth/login-qr.ts`，不学习其框架胶水。

## 一、行为与范围

- Web 设置页扫码，完成后得到一份完整配置；凭据写 SQLite，不进入 config.toml，不返回浏览器。
- 一个微信对象对应一个扫码用户、一条 Root 会话。同一用户重新扫码只更新凭据，继续原会话。
  换用户启用另一对象；旧对象保留历史，旧号被顶、发送失败均可接受，不迁移积压。
- 本版只有一个启用对象。对象内部状态相互独立，为以后两个用户各运行一个 ClawBot 保留边界；
  本版不提供多对象管理界面、群聊或白名单管理。
- 入站接文本和服务端语音转写；普通 emoji 属于文本。图片、表情包、视频、文件接入延期到 V2。
- 每条落盘 Reply 的全部文本块合并发送；Notification 同一路径。超长文本分段，不流式、不做 Markdown 转换。
- 微信发送首次失败后只重试一次，再失败则跳过并保留失败事实，后面的回复继续发送。
- 重启不自动重跑旧任务、不自动补发旧回复。新消息可启动新一轮，带已有上下文及故障描述。
  手动继续指用户明确发送新指令，例如“根据上次故障现场继续”；不恢复旧 run，也不自动重试未知副作用的工具。
- 自动压缩是正交的 core 功能，另起契约；本蓝图不增加微信专属压缩、上下文截断或 /clear。
- typing 尽力而为，不影响入站、执行和投递；不缓存 ticket，不承诺长 run 的周期续发。

## 二、模块、依赖与装配

```text
mic-message ← mic-store ← mic-core ← mic-channel-wechat
mic-message ← mic-tool  ← mic-core ← mic-gateway
bin/micnext 装配以上模块；Gateway 与微信模块互不依赖。
```

新增 `crates/mic-channel-wechat`，公开 API 仅 `pub struct WechatModule;`。
`Module::name() = "wechat"`，`activation() = Always`；安装解析空配置、登记模块迁移、一个 Service 和一个登录 port。
若用户写入未知微信 TOML 字段，启动报错，不能静默忽略。

二进制新增 Cargo feature `wechat = ["dep:mic-channel-wechat"]`，默认启用；
`--no-default-features` 不登记该模块，Web 显示“本构建未包含微信”。feature 决定能力是否编入，SQLite 决定哪个对象启用。
`-p` 不启动任何 Service 或微信登录任务，仍走已有一次性执行路径。

内部依赖：微信仅依赖 `mic-core`、`mic-store`、`mic-message`；不依赖 Gateway、Provider 或工具实现。
外部依赖：tokio/tokio-util、reqwest（rustls、JSON）、serde/serde_json、thiserror、tracing、getrandom、base64（编码 X-WECHAT-UIN）。
二维码由 Web 使用本地打包的二维码库生成，不访问第三方二维码服务。
新增模块：core `channel_setup`、微信 `client/wire/login/account/service/delivery/limits`、Gateway `channels`；
lib.rs 只声明模块并 re-export 公开 API，其余类型默认 pub(crate)。

## 三、公开契约

### 3.1 core 的登录 port

port 只服务当前已观察到的扫码/验证码交互，不设计通用 Channel 配置框架。
二维码登录的协议细节由微信解析；Gateway 只呈现状态和提交用户输入。

```rust
// 以下均由 mic-core re-export。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SetupAttemptId(pub String);

#[derive(Clone, Debug, Serialize)]
pub struct LinkedChannel {
    pub account_id: String,
    pub user_id: String,
    pub session_id: SessionId,
    pub connection: ChannelConnection,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChannelConnection { Connected, NeedsLogin, Faulted }

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum SetupProgress {
    Preparing,
    Waiting { qr_content: String },
    Scanned { qr_content: String },
    NeedsCode { qr_content: String },
    Expired,
    Cancelled,
    Connected { account: LinkedChannel },
    Failed { reason: SetupFailure },
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SetupFailure { Network, VerificationBlocked, ExistingBinding, Protocol }

#[derive(Clone, Debug, Serialize)]
pub struct SetupAttempt {
    pub id: SetupAttemptId,
    pub progress: SetupProgress,
}
#[derive(Clone, Debug, Serialize)]
pub struct ChannelSetupView {
    pub account: Option<LinkedChannel>,
    pub login: Option<SetupAttempt>,
}

pub trait ChannelSetup: Send + Sync + 'static {
    fn status(&self) -> BoxFuture<Result<ChannelSetupView, ChannelSetupError>>;
    fn begin(&self) -> BoxFuture<Result<SetupAttempt, ChannelSetupError>>;
    fn submit_code(&self, id: SetupAttemptId, code: String)
        -> BoxFuture<Result<(), ChannelSetupError>>;
    fn cancel(&self, id: SetupAttemptId)
        -> BoxFuture<Result<(), ChannelSetupError>>;
}

impl Registry {
    pub fn channel_setup(&mut self, setup: impl ChannelSetup);
}
impl Kernel {
    pub fn channel_setup(&self, channel: &str) -> Option<Arc<dyn ChannelSetup>>;
}
```

`BoxFuture` 复用现有 core 定义。登记键自动取 `Registry.current`，实现方不能另填一份名字。
重复登记返回新增 `AssembleError::DuplicateChannelSetup { channel: &'static str }`，启动失败。
Kernel 返回 trait 对象；未编入返回 None，未登录则 port 存在且 account=None。

微信另以 `wechat` 登记渠道提示（[channel-prompt](channel-prompt.md)）。模型随工具调用写的进度文字
复用 Reply 投递路径发出；core 不等送达再执行工具，不保证用户先于工具执行收到。

`ChannelSetupError` 是 thiserror 枚举：`AttemptNotFound`、`WrongPhase`、`InvalidCode`、`Unavailable`、
`Internal { source: BoxError }`。前四项为业务错误；Internal 的 source 只写经脱敏的本机日志。
验证码去首尾空白后非空、长度 ≤ 128 个字符；不猜数字位数。未知/旧 attempt id 拒绝，不送到新登录。

begin 创建新的 attempt，取消前一个未完成 attempt，立即返回 Preparing；后台自行推进扫码。
status 只读取快照，刷新、关页面或两个标签页均不推进协议轮询。
提交验证码只在 NeedsCode 有效；取消完成的 attempt 返回 WrongPhase。
过期通过 begin 重取，不自动无限重取。已完成结果保留到下一次 begin 或进程退出。

### 3.2 Kernel 的会话、身份与消息委托

```rust
impl Kernel {
    pub async fn resolve_identity(&self, identity: Identity, display_name: &str)
        -> Result<PersonId, KernelError>;
    pub async fn default_workdir(&self) -> Result<String, KernelError>;
    pub async fn pending_deliveries(&self, channel: &str)
        -> Result<Vec<PendingDelivery>, KernelError>;
    pub async fn mark_delivered(&self, id: MessageId) -> Result<(), KernelError>;
    pub async fn append_notification(&self, session: SessionId, source: &str, text: String)
        -> Result<MessageId, KernelError>;
    pub async fn subscribe_from_now(&self, session: SessionId)
        -> Result<(EventReceiver, Option<MessageId>), KernelError>;
}
```

identity 使用 Store 现有语义，Kernel 提供时间；微信 identity=("wechat", ilink_user_id)，
不同扫码人得到不同 PersonId，不将所有对象硬映射成 kernel.owner()。Web owner 不变；本版不增加多用户授权系统。
display_name 本版取 ilink_user_id，不虚构上游昵称。

default_workdir 从设置读取，展开 ~/ 并创建目录，返回 UTF-8 绝对路径。
这是现有 Gateway 私有 `new_session_workdir` 的共享下沉，两个消费者改用同一来源。
新增 `KernelError::Workdir(WorkdirError)`；公开 WorkdirError：`HomeMissing`、`NonUtf8 { path: PathBuf }`、
`Create { path: PathBuf, source: std::io::Error }`。不新增重复的配置校验。

pending_deliveries 是 Store 同名查询的薄委托；mark_delivered 同名委托并由 core 提供时间。
对象归属和失败跳过由微信解释，core 不识别微信 payload。
append_notification 使用既有 MessageBody::Notification，run=None，落盘后发布既有 MessageAppended，不唤醒模型。
它的当前消费者是不支持内容的提示；框架执行失败继续使用既有 Engine 通知，不绕到这个接口。
subscribe_from_now 与稳定消息发布共用 Events 的发布锁：锁内建立订阅并读取该 session 最新消息 id，
再释放锁。所有运行期稳定消息写入/发布均持有该锁，因此切点之后的新消息不会被算进历史前缀。
接收器仍是全局事件，消费者按 session 过滤；会话不存在返回新增 KernelError::SessionNotFound。
这个接口由微信 sender 消费，用来跳过旧积压而不漏掉初始化期间的新输出；原 subscribe 不变。

### 3.3 Store 的启动待命契约

```rust
impl Store {
    pub async fn hold_unclaimed_inputs(&self, at: i64) -> Result<Vec<Message>, StoreError>;
}
```

在既有 recovery 收尾 Executing 后、任何 Service/worker 启动前调用。
以事务将所有遗留未认领输入登记为 held，并为受影响会话追加一条既有 Notification，说明旧输入未执行、现场保留。
返回本次新增的通知，供启动日志计数；此时没有订阅者，不发实时事件。
通知只说明“上次有消息未执行”，不添加“现场保留”或“发送新指令”等自然行为说明。

新增 core 迁移表 `core_input_holds(message_id PRIMARY KEY REFERENCES core_messages(id), held_at NOT NULL)`。
held 是输入的调度处置，不是新 run 终态：claim_next、absorb、sessions_with_unclaimed_input 排除 held；
context_window 包含 held 输入，故新消息仍能带上旧现场。无模型调用、无假 Executing run、无设置快照伪造。
修改未认领输入的内部谓词时，调度与上下文各使用对应语义，不让“run_id=NULL”同时承载两个事实。

Assembly 不再用遗留输入预填 scheduler 的 initial 队列；运行期新输入仍经同一 wake、claim_next、Engine 路径。
旧 run 仍变 Interrupted，悬空工具仍补结果未知/取消说明。用户明确发新指令手动继续，仍创建新 run。
本轮不新增恢复按钮或恢复旧 run 的 API。

这是所有 Channel 的统一行为变更，覆盖 Web 和 CLI 遗留输入；不是微信专属开关。
`Message`、`RunState`、`KernelEventKind` 形状不变。held 表只被启动恢复、调度和上下文读取消费。

## 四、微信对象与持久数据

### 4.1 身份与凭据

对象键是扫码用户 `ilink_user_id`，内部强类型 `WechatUserId(String)`；公开视图 account_id 同样取它。
bot id、token、API baseurl 是对象可更新的连接配置，不作为会话主键。
Root 的 channel="wechat"、chat=ilink_user_id；同号 bot id 变化仍得到原 Session。
DeliveryTarget：channel="wechat"、version=1、payload 为强类型 `WechatTarget { user_id: WechatUserId }` 的 JSON。
磁盘版本非 1、payload 非法或与 session 归属不一致 → Err，不猜旧格式。

登录流程在 confirmed 边界一次构造私有 `WechatCredentials { user_id, bot_id, token, base_url }`。
四项缺失/为空或 base_url 不是无用户信息的 HTTPS URL → Protocol 失败，不落部分配置，不静默套默认 baseurl。
token 用私有包装，Debug 脱敏，不实现向 HTTP 视图序列化；登录结果不经过浏览器回传。

本 B2 提案将 bot token 放模块 SQLite 列，不增加第二套密钥管理；磁盘访问沿用既有本机 Store 边界。
API 不读出 token，日志不记录 token、context_token、验证码、二维码内容或原始响应正文；
例外：响应解码失败时，debug 级别输出原始响应体供协议对齐（可能含 token 与正文，仅调试时开启）。
已有模型 API key 的加密契约不变；若 human 要求微信 token 同样磁盘加密，应先修订本文凭据契约再实现。

### 4.2 模块迁移（module="wechat"，version=1）

| 表 | 字段与事实 | 写者/消费者 |
|---|---|---|
| wechat_account | user_id 主键、bot_id、bot_token、base_url、person_id、session_id、connection、updated_at | 对象协调者保存完整配置；收发 client 读取 |
| wechat_settings | 单行 active_user_id，可空，引用 account | 登录完成切换；Service 决定启用对象 |
| wechat_state | user_id 主键、get_updates_buf、context_token 可空、longpoll_timeout_ms | 本对象入站；发送/typing 读 token |
| wechat_inbound_batch | 本地 batch id、user_id、版本化强类型响应 snapshot、phase（importing/completed/interrupted）、created_at、finished_at | 入站与启动收尾；开发查看中断批次 |
| wechat_delivery | message_id 主键、user_id、版本化文本分段计划、最终 outcome（sending/sent/skipped/interrupted）、finished_at | 本对象唯一 sender；防止后续事件重新投递已跳过消息 |
| wechat_delivery_attempt | message_id、chunk_index、attempt_no（1/2）、client_id、started_at、finished_at、外部 message_id 可空、失败类别可空 | sender 写；本机审计读取 |

person_id/session_id/message_id 引用 core 对应主键，不由微信写 core 表。
所有 user_id 外键归属一致；模块表的版本与 payload 均在磁盘边界一次解析成强类型。
这里的 batch 是接收现场，不是按外部 message_id 建的去重表；不承诺服务端永不重复。
completed 批次可在成功导入后删除；interrupted 批次和发送失败记录保留，不自动清理。
删除 completed 批次的四项：消息已在 core、隐私边界不扩大、失败批次不删、失败审计仍在尝试表。

### 4.3 登录与切换的原子交接

本版只激活一个 user_id。扫码未完成不影响旧对象；登录失败不替换旧配置。
确认完成后取消并等待旧对象的网络任务退出，再保存新配置与 active_user_id，启动对应对象。
同号替换保留 person/session/history；首次创建按默认工作目录、默认人设/模型、ToolScope::All 建会话。
identity/session 创建后模块事务失败，最多留下无输入的空会话，沿用现有会话列表隐藏规则。

重新扫码后重置该对象的游标为空、context_token=None，不猜它们能跨凭据复用；原会话历史保留。
等待下一条有效入站取得新 token 后再发新回复；旧积压不自动补发。
每个在途请求持有所属对象及该连接代次。旧代次结果不能更新新连接；不将这种检查散落给 Gateway。
以后两个对象可各自运行同一对象流程，无全局投递锁；双对象的启用 UI 与协议限制届时另起 B2。

## 五、入站、重启与新一轮执行

1. getupdates 使用本对象的游标。仅长轮询超时视为“无新响应”，保持原游标；不制造成功响应。
2. 成功非空响应一次 parse 为强类型。先在模块事务保存完整 batch snapshot 与新游标，再逐条导入。
   空成功批次只更新游标/服务端 timeout。先提交游标明确选择至多一次导入，不保证崩溃窗口内每条都执行。
3. 只接本对象扫码人、无 group_id、message_type=User 的消息；其他身份/群/bot 消息忽略，不回复。
4. 已准入消息先更新 context_token，再按 item_list 原顺序收集文本和 voice.text，作为一次 UserInput 写入。
   不支持的媒体与无转写语音按原位置打扁为文本占位，与处置说明经 `append_recorded_input` 同事务写入（见 conversation-parity.md）；
   含占位的入站只记录（held），不启动模型。
5. 使用本对象 session/person 调 append_user_input，沿用全部既有事件、落盘、取消和执行行为。
6. 完成导入后标 batch completed。模块或 Store 不变量/写入错误 → Service Err，进程退出，保留 importing snapshot。

启动时 importing 批次转 interrupted，不重新调用 append_user_input；next cursor 已持久化，继续等待新消息。
已入 core 但未执行的旧输入被统一 held；已执行 run 被 recovery 收尾。
尚未导入的 batch 内容只作本机故障现场，不偷偷作为新指令提交；本版不为它造模型摘要。
崩溃后可能有部分输入未导入，用户手动重发即可。批次接收与 core 写入不宣称跨事务恰好一次。
服务端本身重复投递仍属已接受的无入站去重边界，启动待命不能证明上游无重复。

新消息入站后正常 wake 新一轮；模型读原历史、held 输入、中断通知及工具未知结果。
这个模型上下文与 Web 同源，微信不维护第二份上下文，也不自动认定旧工具可以安全重试。

## 六、出站、有限重试与 typing

### 6.1 一条消息的发送路径

sender 在对象接受新入站之前调用 subscribe_from_now，得到原子订阅切点；不把此前 pending 自动补发。
对象重新登录/启用也重新建立切点。落盘消息实时事件和 Lagged 补查只处理切点之后、归属本对象的 Reply/Notification。
Lagged 时重新查询 pending_deliveries 并按模块 outcome 排除已成功/跳过/中断的项，再按 id 顺序处理；
切点之后的消息不会因初始化期间查询而被误算成旧积压。

一条 Reply 只提取 Text 块，保持块顺序合并；不发送推理或工具参数。Notification 发送 text。
空文本直接标 delivered，不生成外部发送请求。超长按 Unicode 字符边界分段，分段计划先持久化。
每段首次发送之前登记 attempt 1；失败等待固定 2 秒后登记 attempt 2。每段最多两次请求，client_id 两次相同。
仅 ret 缺失或为 0 且返回有效 message_id 才成功；只表示服务端接受，不承诺手机已送达。

所有段成功后 mark_delivered，再将模块 outcome 标 sent；两步间崩溃不能重发，启动按 interrupted 留现场。
某段两次均失败 → outcome=skipped，后续段不发，继续下条 core 消息；先前已发送的段保留尝试事实。
skipped 不写 delivered_at；以后查询可能再次返回，但 sender 按模块 outcome 排除，不能无限重试。
sender 唯一且每对象串行，保证该对象尝试顺序；与入站长轮询和其他对象互不阻塞。

重启时所有 sending 与未结束 attempt 转 interrupted，不自动重试，不拿一次新进程预算再发两次。
发送返回成功但本地记录前崩溃的结果仍为未知，不能伪造成功，也不自动补发。
发送重试发生在活进程内；stop 可取消 HTTP 与 2 秒等待，剩余现场留到启动收尾。

### 6.2 登录失效与其他错误

- sendmessage 返回 -14 也是发送失败，仍完成仅一次重试；再失败则 skipped。
  当前两次预算完成后本对象 NeedsLogin，停止其后续收发；其他认证请求的 -14 立即走同一失效转换。
  不阻塞别的对象，重新登录仍不自动补发旧积压。
- 其他正常发送失败执行一次重试后跳过；Network/Timeout/Rejected 按强类型类别记录。
- getupdates 的网络错误按有限固定退避继续等待入站；正常长轮询超时直接重发。
  数据库、不变量、磁盘版本与边界解析错误 fail fast，不能伪装成空消息。
- Protocol 专指响应结构、必需字段或已知枚举契约破坏；必须向 Service 返回 Err，不进入发送重试或静默跳过。
  结构合法的非零业务返回属于 Rejected，按正常发送失败处理。client 是这一分类的唯一来源。
- 对外错误文案只包含类别和操作提示，不拼接含凭据的上游原文。

### 6.3 typing

本对象 RunStarted → 用其 user_id/context_token 取一次 ticket → sendtyping(1)。
RunFinished 任意终态 → 使用该 run 的 ticket sendtyping(2)。正常网络/业务失败只记脱敏日志，不落 Notification，不重试。
明确 -14 优先执行连接失效转换，Protocol 优先返回 Service Err；不能被 typing 的尽力而为吞掉。
只处理本对象 session；Lagged 后用 executing_run 校准，取消已知 ticket 后按当前执行状态重新开始指示。
启动没有 active run 时不发 typing；恢复中的断网/进程退出无法保证手机立即取消，明确为尽力而为。
typing 不排在回复发送队列里，不因一个慢请求拖住入站或回复。

## 七、登录协议与 HTTP

登录流程自行持有二维码轮询、验证码和重定向状态；后台任务归微信 Service 管理，stop 时取消并等待退出。
多个 begin 仅最后未取消的 attempt 可保存配置。begin/submit/cancel 由 port 进入协调者，
内部完成结果用独立 task 完成通知；协调者不向自己的带 ack 命令通道发送并等待 ack。

confirmed 必须有完整凭据。binded_redirect 不发新凭据：本版返回 Failed(ExistingBinding)，
提示“已有绑定，请重新获取二维码登录”，不把缺配置或已失效配置视为成功；该分支接入实测后再核对。
scaned_but_redirect 必须有合法 redirect_host；轮询地址变更由 client 解析成 HTTPS URL。
need_verifycode 进入 NeedsCode；verify_code_blocked 进入 Failed(VerificationBlocked)；expired 进入 Expired。
网络失败仅在二维码有效期内继续等待，未知状态或字段不匹配返回 Protocol。

复用 Gateway Bearer 鉴权，不引入 cookie/CORS，所有接口同源：

| 方法与路径 | 请求 | 成功结果 |
|---|---|---|
| GET /api/channels/wechat | 无 | 200 ChannelSetupView；未编入返回 200 `{available:false}`，编入为 `{available:true, ...view}` |
| POST /api/channels/wechat/login | 空 JSON 对象 | 202 SetupAttempt，通常 Preparing |
| POST /api/channels/wechat/login/{id}/code | `{code:string}` | 204 |
| DELETE /api/channels/wechat/login/{id} | 无 | 204 |

HTTP DTO 是 Gateway 私有类型；available 是能力事实，不作为核心行为分支参数。
SetupProgress 用 serde 内部标签 state，类型定义即 wire 真相，Web TS 对齐。
错误均为 `{error:"中文说明"}`：未编入的写接口/AttemptNotFound → 404；WrongPhase → 409；
非法 JSON/未知请求字段/InvalidCode → 400；Unavailable → 503；Internal → 500（隐藏 source）；鉴权失败 → 401。
GET 只读状态，任何视图都无 bot token、context token、验证码或完整配置。
二维码过期/验证码封禁后 UI 提供“重新获取”；刷新页面读取现有 attempt，网页关闭不取消登录。
Web 继续只能查看微信历史，不能经原 messages/persona/model 写接口续聊微信会话。

## 八、协议边界与常量

client 完整接收源码所有已声明字段；暂无消费者的媒体、引用、工具 item 仍建强类型，不用 Value 漂流。
wire DTO 拒绝未知字段，新增上游字段先补边界模型，不在运行期静默丢掉。
所有响应的 ret/errcode 与 SDK 判定一致：缺失视为成功，出现非 0 为 BusinessRejected，-14 为登录失效。
可选字段在 wire 层保留 Option；业务需要的 user_id、context_token、游标等在相应成功分支一次验证后交给内部流程。

**P3 契约补正（2026-10-09，human 已确认按 SDK 接口逐步调试）**：SDK 的 QRCodeResponse、StatusResponse 没有 `ret`：
取二维码以 HTTP 成功且两个必需字符串非空为成功；轮询以 HTTP 成功、已知 status 及该状态的必需字段为准。
confirmed 仍须完整凭据，未知字段/状态及缺失必需字段仍为 Protocol。
**实测补正（2026-10-09，服务器）**：`get_bot_qrcode` 返回 `"ret":0`（SDK 未声明），两个登录响应补收 `ret`；
`getupdates` 成功响应不带 `ret`，原「缺失 ret 即 Protocol」与 SDK（`monitor.ts` 仅在出现非 0 时判失败）及实测均不符，
改为上文统一判定。入站实测另见：item `msg_id`/ref `svr_id` 为不透明字符串（如 `v1:…`），
消息带 `root_id`/`parent_id`（数字），item 带 `button_item_list`（仅见空数组）与 `at_bot_username_list`。
依据为本地 Bun 缓存的 `@tencent-weixin/openclaw-weixin@2.4.9/src/auth/login-qr.ts` 两个响应接口。

认证头/base_info 见协议素材 §二，协议值集中于 client/limits，不散落给 Gateway 或调用方。
bot_agent 为 micnext/<version>；iLink-App-Id、bot_type 等实测项见 §十一，未验证前不声称协议兼容。

wire 接收字段清单（字段 optional 性保留源码声明，嵌套类型独立）：

| 类型 | 完整字段 |
|---|---|
| GetUpdatesResponse | ret, errcode, errmsg, msgs, sync_buf, get_updates_buf, longpolling_timeout_ms |
| WeixinMessage | seq, message_id, from_user_id, to_user_id, client_id, create_time_ms, update_time_ms, delete_time_ms, session_id, group_id, message_type, message_state, item_list, context_token, run_id, root_id†, parent_id† |
| MessageItem | type, create_time_ms, update_time_ms, is_completed, msg_id, ref_msg, text_item, image_item, voice_item, file_item, video_item, tool_call_start_item, tool_call_result_item, button_item_list†, at_bot_username_list† |
| RefMessage / PartialText | message_item, title, svr_id, partial_text / start, end, startindex, endindex, quotemd5 |
| CDNMedia / TextItem | encrypt_query_param, aes_key, encrypt_type, full_url / text |
| ImageItem | media, thumb_media, aeskey, url, mid_size, thumb_size, thumb_height, thumb_width, hd_size |
| VoiceItem | media, encode_type, bits_per_sample, sample_rate, playtime, text |
| FileItem / VideoItem | media, file_name, md5, len / media, video_size, play_length, video_md5, thumb_media, thumb_size, thumb_height, thumb_width |
| ToolCallStartItem / ToolCallResultItem | tool_name, tool_call_id / tool_name, tool_call_id, status |
| QRCodeResponse / QRStatusResponse | ret†, qrcode, qrcode_img_content / ret†, status, bot_token, ilink_bot_id, baseurl, ilink_user_id, redirect_host |
| SendMessageResponse / GetConfigResponse / SendTypingResponse | message_id, ret, errmsg / ret, errmsg, typing_ticket / ret, errmsg |

† 为 SDK 未声明、实测补收字段；button_item_list 元素字段未知，非空时按报错路径补模型。
message_id/root_id/parent_id 边界接受十进制字符串或整数，parse 为 u64，序列化为字符串；不经 f64。
msg_id/svr_id 按 SDK 为不透明字符串。
seq/尺寸/时长/索引使用无符号整数，时间毫秒使用 i64，ret/errcode 使用 i32；file.len 按源码接字符串。
message_type/item_type/state 等数字在 wire 层完整接收，进入领域时 parse 成已知枚举；未知值为 Protocol，
不猜未知媒体为文本。RefMessage 递归使用 Box，按 serde 已有深度约束，不新增手写递归机制。

build 后不变的旋钮统一放微信 limits.rs：长轮询/二维码轮询超时 35s、二维码有效期 5min、
文本分段 4000 Unicode 字符、每段最多 2 次、发送重试等待 2s、入站网络错误退避 2s（连续 3 次后 30s）。
核心消息/输入 held 查询无需新增容量常量。Gateway 验证码长度限制放它的 limits.rs。

## 九、写者、状态与关键场景

| 实体 | 唯一协调写者 | 状态 |
|---|---|---|
| 登录 attempt | 微信协调者接收协议任务结果 | Preparing → Waiting/Scanned/NeedsCode → Connected/Expired/Failed/Cancelled |
| 对象连接 | 本对象协调者 | 未配置 → Connected → NeedsLogin/Faulted；同号新配置接回 Connected |
| 入站 batch | 本对象入站任务；启动收尾在任务出现前 | importing → completed；重启遗留 importing → interrupted |
| delivery | 本对象唯一 sender；启动收尾在任务出现前 | sending → sent/skipped；重启遗留 sending → interrupted |
| run | core 调度与执行；启动 recovery | Executing → 既有终态；遗留 Executing → Interrupted |
| 输入调度处置 | core 启动持久化 held；运行期 claim/absorb | 遗留未认领 → held；新输入 → 既有认领路径 |

```mermaid
sequenceDiagram
    participant W as Web
    participant L as 登录流程
    participant O as 对象协调者
    participant K as Kernel
    W->>L: begin / submit_code
    L-->>W: 状态快照
    L->>O: confirmed 完整配置
    O->>O: 停止旧连接任务并等待退出
    O->>K: resolve_identity / resolve_root_session
    O->>O: 模块事务保存配置和启用对象
    O-->>W: Connected（无秘密）
    Note over O,K: 同号沿用 session；换号独立 session
```

```mermaid
sequenceDiagram
    participant I as 微信入站
    participant S as SQLite
    participant K as Kernel
    I->>S: 保存 batch snapshot + next cursor
    I->>K: append_user_input
    Note over I,K: 进程在批次中途崩溃
    K->>S: 启动：旧 run Interrupted，旧未认领输入 held
    I->>S: 启动：遗留 batch interrupted，不再导入
    I->>K: 用户新消息
    K->>K: 新 run 读取历史、held 输入及故障描述
    Note over K: 用户明确要求才手动继续旧工作
```

不变量：对象间不串凭据/token/游标/队列；同号凭据更新不改 session；旧连接结果不能写新连接；
输入只有一条执行主路径；启动不自行执行 held；每段最多两次请求；失败跳过不写 delivered_at；
实时事件可丢而稳定消息查询可补查；登录/typing/长轮询不能串行阻塞回复；任务退出受 Service stop 管理。

## 十、调用方与迁移兼容性

| 调用方/消费者 | 改动与兼容判断 |
|---|---|
| bin/micnext | 新可选依赖/feature/模块登记；Assembly/OneShot 签名不变，-p 不启动 Service |
| mic-core Registry / Assembly / Kernel | 新 setup 贡献、装配重复检查、Kernel getter；构造参数为 crate 私有，按同一蓝图迁移 |
| mic-gateway Service / error / settings / 新 channels | 添加登录路由和错误映射；工作目录改调用共享 helper；原 Web HTTP 形状保持，启动自动补跑行为改变 |
| web client/types/SettingsView | 新微信设置区、状态轮询/二维码/验证码；复用 token 与历史 UI，不自行解释协议或凭据 |
| mic-channel-wechat | 新消费者，按本蓝图所有边界建模；经 Kernel 写 core，经模块事务写 wechat_ 表 |
| mic-store row/store/schema | 新 held 表与接口；claim/absorb/context/window 查询区分 held；既有方法签名保持，新增迁移号取实施时下一个版本 |
| mic-core recovery / scheduler / Assembly.run | 启动收尾后 held，初始调度为空；运行期 worker/Engine 复用，不加第二个 runner |
| mic-core Engine / request / Provider | 消费 context_window，自动得到故障现场；公开 ModelRequest/Provider/ProviderFactory 无签名变化 |
| mic-provider-openai | 工厂及模型接口不变；作为运行期回归消费者核对，无微信依赖 |
| mic-tool / mic-tool-shell / mic-tool-fs / mic-tool-web-fetch | Tool/ToolContext 无变化；故障后未知副作用仍保留，不自动重跑 |
| mic-message / Gateway SSE / CLI 展示 | Message/RunState/事件无形状变化；既有 Notification/ToolResult 可直接呈现 |
| 现存 Module/Service 实现 | trait 原方法不变；Registry 新方法为纯新增，不要求所有模块实现 setup |
| 现有错误 match | AssembleError 新变体由二进制 anyhow 收口；KernelError::Workdir 显式映射、SessionNotFound 映射 404，其余内部 catch-all 核对 |
| 既有源码内测试（若覆盖上述模块） | 实施时按覆盖关系跑，必要更新 held/启动预期；Phase 1 涉及的 core/store 当前无既有测试文件 |

parallel change：先新增 port/委托/held 接口并保持旧调用可编译，迁移 Gateway helper 与 Assembly 调度，再装配微信。
一次迁移完成即删除旧 workdir helper 和旧启动补跑路径，不提供双行为开关。
现有 pending_deliveries 保留，不在 core 新增微信专属筛选/发送状态。

本文批准实施后同步修订 mic-core-module、mic-store、run-execution、gateway、web-ui、v0a-module-map
中的调用方/启动语义；“微信经 Gateway 入站和发送”统一改为“微信直接经 Kernel，Gateway 仅承载 Web 登录 UI”。
旧 roadmap/cross-check 中重启生效、可靠补发和入站重复描述以本次批准契约为准，不留两套规范。
自动压缩、V2 媒体和双对象 UI 不并入此次实现。

## 十一、实施顺序与人工验收

### 11.1 推进约定

human 负责每阶段的人工验收与行为矫正。方向/契约确定后先写完代码、完成 fmt/clippy，再交 human 验收；
验收通过后更新文档，不在实现期间不断扩写验收文档或主动补测试。
默认阶段交付后等待 human 验收结论再进入下一阶段。本轮 human 明确批准 Phase 2–5 连续推进至能对话，
再一起扫码与人工验收；该调整不覆盖 Phase 6 的 typing，也不把 SDK 核对等同于真实协议验收。
批准某阶段仅覆盖下表列出的契约，不把未定协议事实或后续阶段视为一并批准。
矫正在已批准契约内则直接修；改变公开契约或副作用边界则先修订本文，方向有真实分歧退回 B1。

每阶段均保持可编译，完成后执行 `cargo fmt`、`cargo clippy -- -D warnings`；
仅改动覆盖既有测试时运行相关测试，不为阶段拆分新增测试框架、临时公开 API 或行为开关。
新增 crate/feature 后补查默认构建与 `--no-default-features`，依赖方向以 `cargo tree` 核对。
SQLite 迁移只前进；代码可回滚不等于数据库可降级。故障验收使用独立数据目录，避免改动日常历史。
只在 human 明确要求时提交；全阶段验收通过并完成检查后才将整份本文标 CLOSED、更新 todo。

协议不确定项先查本地参考 SDK；SDK 不足以定论时把具体问题交 human 搜索或实测，AI 不自行联网搜索。
已定位本机完整 SDK：`/Users/xingji/.bun/install/cache/@tencent-weixin/openclaw-weixin@2.4.9@@registry.npmmirror.com@@@1`。
已核对 `src/api/types.ts`、`src/api/api.ts`、`src/auth/login-qr.ts` 与 monitor 源码的字段、请求头和流程。
真实账号请求属于接入验收；每次验收先说明请求目的和需要 human 操作的扫码/验证码步骤，不以搜资料替代实测。

### 11.2 阶段与验收边界

| 阶段 | 对应契约与交付范围 | human 验收后可确认的行为 | 状态 |
|---|---|---|---|
| Phase 1：统一启动待命 | §3.3；Store held 迁移/事务、调度谓词与上下文分离、Assembly 启动收尾、删除 scheduler 初始补跑路径；同步 core/store/run 蓝图 | 重启不执行旧输入；历史与故障说明保留；新输入仍正常启动一轮；Web 与 `-p` 正常 | CLOSED：已实现，human 功能验收通过 |
| Phase 2：共享 Kernel 能力 | §3.2；身份/投递/通知委托、原子订阅切点、工作目录共享下沉及 Gateway 错误映射；删除旧 workdir helper | Web 建会话工作目录行为等价；既有聊天/SSE 正常；原子切点与通知不唤醒的代码/日志证据齐全 | 已实现，本地能力检查通过；human 已批准继续接入 |
| Phase 3：Web 扫码登录闭环 | §二、§3.1、§四、§七、§八登录部分；新 crate/feature、完整模块迁移、登录 client、port/协调者/Service、完整凭据交接、Gateway 路由、设置页 | 实际扫码后保存账号；刷新不新建 attempt；验证码/过期/取消明确；同号会话不变、换号会话独立；浏览器无凭据 | 已实现；本地接口/迁移检查通过，真实扫码待服务器验收 |
| Phase 4：微信入站 | §五、§八入站部分；getupdates、批次与游标事务、准入过滤、文本/语音转写、媒体 Notification、入站网络退避/-14、启动批次收尾 | 微信新文本在 Web 历史中出现并执行；纯媒体只落提示；群/他人/bot 不启动模型；崩溃后旧批次不重导 | 已实现；初始游标与真实入站行为待验收 |
| Phase 5：微信回复投递 | §6.1～6.2、§八出站部分；sender 原子切点、Reply/Notification 提取、分段计划/尝试记录、一次重试/跳过、Lagged 补查、发送 -14、启动投递收尾 | 每次模型调用出话就发微信；失败预算正好两次；跳过不冒充 delivered；后续回复可发送；重登/重启不补发旧积压 | 已实现；真实投递与失败/重启场景待验收 |
| Phase 6：typing 与整体验收 | §6.3；getconfig/sendtyping、run 生命周期与 Lagged 校准；复验停止/换号/断网/崩溃，回填协议实测、文档与 todo | 输入指示随 run 开始/结束；慢 typing 不阻塞收发；所有对象隔离、故障与退出场景符合本文 | 待开始 |

Phase 3 内按 3a 协议边界与模块构建 → 3b 登录/账号/HTTP → 3c 设置页依次推进，每步保持编译通过。
3a 先核对 SDK 的真实字段、optional 性、枚举、请求头和登录重定向；
`iLink-App-Id`/`bot_type` 若仍无法确定，交 human 决定或查询，不能写一个猜测值后调用真实账号。
3b 不引入 CLI 登录入口；3c 完成后才交付 Web 扫码验收。
登录阶段的 Connected 表示已接受完整凭据，不宣称已完成后续入站、回复或 typing 阶段。

Phase 4 首次启用轮询前必须核实新凭据空游标是否会重放旧历史；若会，先修订连接起点契约。
Phase 4/5 的现场持久化、重启处置和取消随对应主路径一起实现，不能延期到 Phase 6 再补。
阶段间的未实现能力只作为交付缺口说明，不新增永久的收发开关或另一条调试主路径。

### 11.3 Phase 1 的具体交付与人工验收

批准范围仅 §3.3：新增 `Store::hold_unclaimed_inputs(at)`，启动为旧未认领输入登记 held 并落一条会话通知；
claim/absorb/待调度查询排除 held，上下文仍包含它们；统一覆盖常驻和 `-p` 的启动收尾。
不新增 Channel port、微信 crate、登录路由、设置页改动或模型压缩。
移除旧启动补跑机制后，调度责任全部由运行期的新输入 wake 承担，遗留输入的处置由持久化 held 承担；
不把同一复杂度挪到微信分支，也不改变 run 终态或工具恢复语义。

验收结果（2026-10-09）：human 已确认中断提示与旧输入待命功能正常。
人工验收关注中断后重启是否待命，以及新指令能否带着旧历史与故障现场正常继续。

实现为 core schema v5；holds 和会话通知同事务，调度/并入排除 held，上下文保留 held。
`Assembly::start` 在 recovery 后统一处置遗留输入，scheduler 不再接初始补跑队列。
删除旧补跑的四项核对：新输入 wake/claim 不变（正确性）；日志只记会话数（隐私）；
recovery/holds 任一失败仍使启动 Err（失败行为）；输入、holds 时间和会话通知保留（可审计性）。

通知文案按 human review 收口为“上次执行已中断”；仅有悬空工具时附加“部分工具结果未知”；
未认领输入通知为“上次有消息未执行”。每个工具仍保留原有结果未知说明。
Web 将相邻 Notification 放在一张左对齐的系统提示卡片中，原消息事实与顺序不变。

开发检查：fmt、clippy 和构建通过；独立数据目录的 v4→v5 迁移、Web/CLI/Completion 待命、
重复启动幂等、新输入/absorb、模型请求中的故障现场、`-p` 不起 Service 均通过。
涉及的 core/store 无既有测试文件，未新增项目测试。临时验收脚本与本机目录不作为长期验收依赖。

### 11.4 全阶段人工场景

人工场景：

- 未编入/未登录：Web 正确展示；-p 不连接微信。扫码、验证码、过期重取、页面刷新/关闭均有明确行为。
- 同号重新扫码会话 id 不变；换号新会话，旧请求结果/待发项不串到新号。
- 文本、语音转写进入一次 UserInput；不支持媒体打扁为占位并附处置说明，含占位的入站只记录；非扫码人/群/bot 消息不触发模型。
- 一次模型调用的正文作为 Reply 发送，工具无文本不发，失败/中断 Notification 走同一路。
- 模拟 sendmessage 两次失败：尝试记录正好 2 条、skipped 且 delivered_at=NULL，后面的回复继续发。
  分段消息记录成功前缀及失败段；重启不重获预算或自动补发。
- 在 batch 持久化后、写入中、发送记录前分别终止进程：现场可查，启动无旧工具执行，旧 batch 不重新导入。
  之后发新消息，模型可见旧上下文和故障说明；显式发“继续”开启新 run。
- -14 只影响所属对象；重新登录不迁移旧积压。正常断网有日志，Store/不变量失败退出。
- RunStarted/Finished、Lagged、stop：typing 尽力校准；HTTP 等待和重试可取消，不出现自等待死锁。
- 依赖用 cargo tree 核对：core 无具体 Channel，Gateway 与微信不互相依赖。

仍需真实协议验证：App-Id/bot_type、context_token 时效、文字上限、上游顺序与重复、
新凭据的初始游标、binded_redirect 的重取登录行为、typing 持续时间和发送错误分类。
空游标若会重放旧连接历史，则本版不能直接按该起点启用收发：先修订连接起点契约，不以去重或静默丢弃绕过。
这些不以 fallback 掩盖；缺配置/协议不匹配先返回显式错误，实测若要求改变公开契约则修订 B2 后再实现。

### 11.5 当前交接（2026-10-09）

#### 已交付的实现

Phase 2–5 的主链路已提交并推送：`d87db45 feat(wechat): 接通扫码登录与消息收发`。
默认构建包含微信；Gateway 通过 core port 访问登录状态，微信模块独立负责协议与收发。

- Kernel 已提供身份解析、默认工作目录、通知落盘、待投递查询/确认和原子订阅切点；通知不唤醒模型。
- 设置页已提供二维码、验证码、取消与连接状态；账号与完整凭据存 SQLite，同号复用会话、换号交接由协调者串行处理。
  六张微信模块表覆盖账号、启用对象、游标/context_token、入站批次、投递计划和尝试记录。
- 入站先持久化响应快照与游标，再导入获准的文本/语音转写；不支持媒体按占位与说明同事务落盘。旧未完成批次启动时收尾，不重新导入。
- sender 从原子切点后的 Reply/Notification 提取正文，按 4000 个 Unicode 字符分段；每段最多两次尝试，共用 client_id。
  失败跳过仍保留记录，重启不补发旧积压。收发任务使用固定账号快照，取消与交接等待数据库写入完成。
- HTTP 失败日志已补齐接口名、连接阶段、I/O 类别、系统错误码与 TLS EOF 标记；HTTP 拒绝记录状态码。
  不输出凭据、请求查询串、二维码内容或原始响应正文（解码失败的 debug 例外见凭据节）。页面仍使用简短失败提示。

上述是实现事实；同号/换号、入站过滤、失败预算、重启处置等真实运行行为仍须按 §11.4 人工验收。
Phase 6 的 getconfig/sendtyping、typing 生命周期与整体验收尚未实现，不据此关闭蓝图。

#### 已完成的本地检查

- fmt/clippy、默认构建和无默认 feature 的 clippy 检查通过；cargo tree 核对内部依赖方向符合 §二。
- 独立目录的 Kernel 能力检查通过：身份隔离、工作目录错误、通知不唤醒、投递确认、订阅切点并发场景。
- 临时实例的启动、微信状态只读、鉴权/非法请求/无效 attempt、六张模块表迁移与 SQLite 完整性检查通过。
- 前端类型检查和构建通过；release 二进制包含 Web 页面，复制到独立目录后能提供 HTML 与 JS/CSS。
- 未新增项目测试；本地检查不替代扫码、微信入站/出站或故障场景验收。

#### 真实接口与服务器现场

本机获取二维码失败发生在 TLS 握手阶段，日志为 `get_bot_qrcode`、`connect=true`、`tls_handshake_eof=true`，
尚未收到微信 HTTP 响应。清掉进程代理变量后仍失败；当时域名解析到 `198.18.0.0/15` 的 Fake IP，路由走 `utun5`。
HTTP 代理及公司 CONNECT 代理的探测也出现 TLS EOF。未确定最终拦截来源，不能据此判定公司屏蔽或微信接口不兼容。
SIT 转发脚本只处理指定 Redis/MySQL 地址，未发现直接处理微信流量的配置。

human 已改用自己的腾讯云干净服务器验收。首次启动已生成 `/root/.config/micnext/config.toml` 并打开 SQLite，
随后因 Web 产物缺失退出；旧提示要求 pnpm，但服务器未安装。当前尚无服务器成功扫码或对话的验收结果。

#### 构建方式与下一步

`f7612e5` 已加入独立 `build.sh` 与 debug/release 的产物缺失提示。
随后 human 指定统一 Bun：已改为 Bun 1.3.14、`web/bun.lock` 与 Bun runtime，删除 pnpm 锁文件和专属配置。
迁移核对的 143 个包版本不变；在 PATH 无 Node.js/pnpm 的环境中完成了前端和 Rust release 构建。
这次 Bun 切换已提交为 `f995558 build(web): 统一前端使用 Bun 构建`，已推送；部署时拉取最新 master 即可。

服务器手动安装 Rust 工具链和 Bun 后，在源码目录执行：

```sh
./build.sh
./target/release/micnext
# 或构建成功后直接启动：
./build.sh run
# debug 构建后启动：
./build.sh debug run
```

脚本只检查工具，缺失即提示手动安装；依次执行 frozen-lockfile 安装、Bun 前端构建、
默认 release 模式清理 gateway 的 release 产物并进行 Rust release 构建，确保最新 Web 资源嵌入二进制。
debug 模式使用 `cargo build --locked`，不清理 gateway；运行时从磁盘读取 Web 产物。运行二进制不需要前端构建工具。
`run` 后的参数原样传给 micnext，例如 `./build.sh run --config /path/to/config.toml`；
用法与失败行为见 [build-script-run](build-script-run.md)。

下一次 human 验收先完成服务启动与默认模型配置，再到「设置 → 微信」扫码，确认连接后发“你好”，
核对微信收到回复及 Web「其他渠道 → 微信」历史；随后验证一次工具调用。
首次连接还须核对空初始游标是否带回旧历史，该事实仍未实测；若发生重放，先修订连接起点契约。
基础对话通过后再验证 §11.4 的验证码、取消/过期、同号/换号、媒体、断网、重启等场景，最后进入 Phase 6。

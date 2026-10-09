# B2：模型设置与 Web 图片输入

**状态**：服务商/模型分层（原 A 批与 v3 修订）已实现并合并为本稿正文；图片输入（B 批）内核部分（mic-message → store → core → openai）已实现；HTTP 与 Web 部分已实现并手工验收，**CLOSED**。遗留：模型不支持图片时的上游报错呈现未处理（见 todo.md）。
**方向记录**：[B1](../brainstorm/model-settings.md)。阶段二以本稿替代 [runtime-settings.md](runtime-settings.md) §四。

## 一、决定与收窄

| 项 | 决定 |
|---|---|
| 两层实体 | **服务商**（代码叫 `Endpoint`，避免与运行期 `Provider` trait 混淆：类型、预设、地址、key）与**模型**（挂在服务商下：模型名、max_tokens、推理强度）。会话/默认/run 快照引用模型；run 快照名为“服务商名 / 模型名” |
| API key | Web 可填；AES-256-GCM 加密入 SQLite，AAD 绑定 `endpoint-key:{endpoint_id}`；主密钥为 0600 本地文件，不在 data_dir 内 |
| key 来源 | 服务商保存了 key 用保存值，否则读环境变量（DeepSeek `DEEPSEEK_API_KEY`、通用 `OPENAI_API_KEY`、Ollama 无）；两处都没有属正常，不发鉴权头，上游拒绝时才表现为鉴权失败 |
| DeepSeek | 地址固定，接口与 UI 都不提供 `base_url`；Ollama 留空取 `http://localhost:11434/v1`，通用必填 |
| 测试连接 | 用表单当前值（未保存也可）请求 `{base_url}/models`，返回模型名列表，UI 逐个生成模型行；不自动保存 |
| 多模态 | 不建能力概念：无能力字段、无探测、无切模型限制；模型不支持就是普通 ProviderFailed |
| 模型呈现 | `ModelView` 留在 mic-message 原位，图片只是新的 `ContentPart`；不搬家、不重构 request |
| Provider | 仅 openai 协议（Generic/vLLM、DeepSeek、Ollama 预设）；无自定义 headers |
| 推理来源 | 保持现状（按请求模型名比较）；跨服务商同名误回传不处理 |
| 迁移 | 不取消现有 schema_migrations；只扩展当前 schema；配置 JSON 不版本化 |

仅有的跨实体规则：**执行中不能切会话模型（409）**。其余模型/图片规则一律不加。

## 二、用户行为

| 场景 | 结果 |
|---|---|
| 零模型启动 | 常驻服务照常启动，设置页可用，默认为 NULL |
| 创建首个模型 | 同事务设为默认 |
| 删模型 | 软删除；删默认须先换默认，最后一个可删（默认回 NULL）；会话保留已删引用 |
| 删服务商 | 级联软删其下全部模型并清密文；其中含默认模型须先换默认（`DefaultInUse`） |
| 新会话 | 建时取默认，无默认允许 NULL |
| NULL 会话收到输入 | 认领时取当时默认并写回 |
| 无模型/所选已删 | 输入照常落盘，认领后经既有 Notification → ProviderFailed 收尾；Web 提示去设置 |
| 执行中改配置/key | 本轮在认领时读定，后续 run 才用新值 |
| 执行中切模型 | 409 `SessionExecuting`，无待切状态 |
| 测试连接失败 | 脱敏中文原因，分网络不通 / 鉴权不过 / 返回意外 |
| 发图 | 纯图片允许；纯文本去空白后为空且无图则拒；模型不收图 → 普通失败 |

## 三、依赖与所有权

| 位置 | 内容 |
|---|---|
| mic-message | image.rs：`ImageId`/`ImageFormat`/`ImageRef`/`ImageData`；`ContentPart::Image(ImageRef)`；limits.rs：图片限额 |
| mic-store | endpoints.rs、models.rs（模型/默认/选择/认领）、secrets.rs（主密钥与加解密）、images.rs（blob 随消息同事务） |
| mic-core | provider.rs（工厂 trait、`ProbeError`、`resolve_key`）、kernel.rs（服务商/模型命令、`test_endpoint`）、input（图片嗅探）、request（加载图片进 `ModelRequest`） |
| mic-provider-openai | 登记工厂；`probe.rs` 测试连接；`ContentPart::Image` 序列化为 `image_url` data URI |
| mic-gateway | 服务商/模型路由、图片读取路由、消息 parts DTO |
| Web | 设置页服务商卡片内嵌模型行；输入框图片选择/粘贴/拖入；历史图片展示 |
| bin/micnext | 提供 `core.key_file`（缺省 config.toml 同目录 `master.key`） |

无新 crate、无新跨 crate 依赖。**不引入 `image` crate**（只嗅探魔数，不解码）；Gateway 加 `base64`。

## 四、Provider 工厂

```rust
pub trait ProviderFactory: Send + Sync + 'static {
    fn kind(&self) -> &'static str;
    fn display_name(&self) -> &'static str;
    fn check_endpoint(&self, json: &str) -> Result<String, ConfigError>;      // 纯本地，返回规范 JSON
    fn check_model(&self, endpoint_json: &str, name: &str, model_json: &str)
        -> Result<String, ConfigError>;
    fn key_env(&self, endpoint_json: &str) -> Option<&'static str>;
    fn build(&self, endpoint_json: &str, key: Option<SecretValue>, name: &str, model_json: &str)
        -> Result<Arc<dyn Provider>, ConfigError>;                            // key 已含 env 回退
    fn list_models(&self, endpoint_json: &str, key: Option<SecretValue>)
        -> BoxFuture<Result<Vec<String>, ProbeError>>;                        // 唯一联网的非 run 路径
}
pub enum ProbeError { Network(String), Auth, Unexpected(String) }            // 文案面向用户，不含 key/响应体
```

Registry `provider_factory(impl ProviderFactory)`；重复 kind → `DuplicateProviderKind`。工厂启动登记一次。
env 读取在 core 侧统一做（`resolve_key(stored, factory.key_env())`）。

openai 配置（`deny_unknown_fields`）：服务商 `{preset, base_url?}`（DeepSeek 禁填地址，规范形态不存地址）；模型 `{max_tokens?, reasoning_effort?}`，模型名单列，推理强度只许 DeepSeek。
`GET {base}/models` 解析 `data[].id`，其余字段不消费；401/403 → `Auth`，连接失败/超时 → `Network`。
HTTP 边界以 RawValue 交工厂解析一次，`serde_json::Value` 不进入调度/Engine/表单。

## 五、存储与类型（mic-store）

```rust
pub struct EndpointId(pub i64);  pub struct ModelId(pub i64);
pub struct SecretValue; // 无 Debug/Display/Serialize，drop 时 zeroize
pub enum CredentialWrite { Keep, Clear, Set(SecretValue) } // 创建时 Keep → InvalidSecretEdit
pub struct EndpointView { id, name, kind, config_json, key_set }
pub struct EndpointWrite { name, kind, config_json, credential }
pub struct ModelView { id, endpoint_id, name, config_json }
pub struct ModelWrite { endpoint_id, name, config_json }
pub enum ClaimedModel { Missing, Deleted{..}, Selected { id, name /*服务商 / 模型*/, kind, endpoint_json, model_name, model_json, key } }
// Store：endpoints/endpoint/create_endpoint/update_endpoint/delete_endpoint/endpoint_key；
//        models/model/create_model/update_model/delete_model/default_model/set_default_model/set_session_model；
//        claim_next -> ClaimedRun { run, settings, model }
```

| 表 | 字段与约束 |
|---|---|
| core_endpoints | id、name、kind、config_json、key_cipher（可空）、deleted_at、created_at、updated_at；活动名唯一 |
| core_models | id、endpoint_id FK、name、config_json、deleted_at、…；同服务商活动模型名唯一 |
| core_settings | default_model_id 可空 FK；必须活动；零活动模型恰好 NULL |
| core_sessions | model_id 可空 FK；软删除不清引用 |
| core_runs | model_id、model_name 快照 |
| core_images | id、session_id、format、size_bytes、bytes blob；不可变 |

联动（首个/默认/最后一个删除/级联/切换与 executing 检查）各在同一事务。`claim_next` 在认领事务内解密并写 run 快照（无秘密）。

**主密钥**：`core.key_file`。`Store::open(path, modules, key: SecretKeyFile)`；`open_in_memory` 用随机内存 key。
首次且库内无密文时原子创建 32 字节 key，权限 0600；已有密文而 key 缺失 → 报错，不重建；格式/权限/AEAD 失败 → 启动 Err；open 时校验全部密文。
密文 = 12 字节 nonce + ciphertext + tag，无版本。替换/软删除清空密文。备份须同时保留库与 key。

## 六、图片

```rust
// mic-message
pub struct ImageId(pub i64);
pub enum ImageFormat { Png, Jpeg, WebP }
pub struct ImageRef { pub id: ImageId }
pub struct ImageData { pub format: ImageFormat, pub bytes: Arc<[u8]> }
// ContentPart 新增 Image(ImageRef)；旧磁盘文本/文件可解码
```

限额（mic-message/limits.rs）：每条最多 4 图、每图 ≤ 4 MiB；格式仅 PNG/JPEG/WebP，由 core 按魔数识别，
不信任文件名与浏览器 MIME，不解码、不缩图、不换格式；不合法或超限整条拒收。
Gateway 图片消息路由体积 24 MiB，设置路由仍 1 MiB，文本沿用 100000 字符。Web 从 `GET /model-kinds` 的 `input_limits` 读限额。

入站：
```rust
pub enum IncomingPart { Text(String), Image(Vec<u8>) }
pub async fn append_user_input(&self, s: SessionId, p: PersonId, parts: Vec<IncomingPart>)
    -> Result<MessageId, KernelError>; // 所有现有文本生产者改为 Text
pub async fn image(&self, s: SessionId, id: ImageId) -> Result<Option<ImageData>, KernelError>;
```
core 嗅探出 `ImageData` 后交 store `append_input(session, person, Vec<NewInputPart>, disposition, notice, now)`（后两参见 conversation-parity.md），图片行与消息同事务；
`image(session, id)` 按 `core_images.session_id` 校验归属，不匹配返回 None。不接受 URL、path、现有 image id 作为上传。
会话创建仍先建会话再追加输入，不新增合并命令。

呈现：`ModelView`/`Message::model_view` 原位不动，`Image` 作为普通片段保留顺序。
`ModelRequest` 新增 `images: HashMap<ImageId, ImageData>`，core 的 `request::build` 按窗口内引用一次性读取（缺失报 `ImageMissing`）。
openai：纯文本 user 仍为字符串；含图时 content 数组按序发 text/image_url，`data:<MIME>;base64,...`。
工具结果里的 Image 与现有 File 一样显式拒绝映射。Boundary/压缩只改上下文，图片原件与 Web 历史保留。
**已接受的风险**：不限制窗口内图片总量，长会话多图请求可能很大，出问题再收紧。

## 七、Kernel、执行与错误

Kernel：`provider_kinds`(同步) / `endpoints` / `endpoint` / `key_env` / `create_endpoint` / `update_endpoint` / `delete_endpoint` / `test_endpoint` /
`models` / `model` / `default_model` / `create_model` / `update_model` / `delete_model` / `set_default_model` / `set_session_model`。
`EndpointDraft { name, kind, config_json, credential }`、`ModelDraft { endpoint_id, name, config_json }`；`now` 由内核产生。
写命令：工厂 `check` → Store 事务；失败不落部分状态。`test_endpoint(draft, existing)`：`Keep` 取库内 key，再回退 env，不写任何状态。

Engine 持有工厂注册表。`claim_next` 后 `resolve_key` + 一次 `build` 创建实例，本轮重试/工具往返/absorb 复用。
`Missing`/`Deleted`/`build` 失败：RunStarted → Notification 落盘 → ProviderFailed → RunFinished；未真正调用则不写 `core_model_calls`。

| 归属 | 变体 | 结果 |
|---|---|---|
| ModelSettingsError | NotFound / Deleted / EndpointNotFound / EndpointDeleted / NameTaken / DefaultInUse / SessionExecuting / InvalidSecretEdit / Store | 404/409/422 |
| InputError | Empty / TooLarge / InvalidImage / ImageLimit | 400/413/422，不落消息 |
| ConfigError | field + 脱敏 message | 422 |
| ProbeError | Network / Auth / Unexpected | 422 |
| StoreError 扩展 | SecretKeyIo / SecretKeyInvalid / SecretIntegrity / ImageMissing | 明确 Err |
| AssembleError 扩展 | DuplicateProviderKind | 启动失败 |

HTTP 与日志不含 SecretValue、密文、上游原始响应体。

## 八、HTTP 与 Web

| 路由（/api） | 说明 |
|---|---|
| GET /model-kinds | `{ items:[{kind,display_name}] }`；`input_limits:{max_images,max_image_bytes}` |
| GET/POST /endpoints；PUT/DELETE /endpoints/:id | 服务商 CRUD；`credential` 为 keep/clear/set；返回含 `key_set`、`key_env`；DELETE 级联 |
| POST /endpoints/test | `{endpoint_id?, kind, config, credential}` → `{models:[string]}`；失败 422 + 中文原因 |
| GET /models；POST /models；PUT/DELETE /models/:id | 模型 CRUD（`endpoint_id,name,config`） |
| GET/PUT /models/default | `{model_id}` |
| PUT /sessions/:id/model | `{model_id}`；执行中 409 |
| POST /sessions；POST /sessions/:id/messages | `parts:[{kind:"text",text},{kind:"image",base64}]`；创建额外 `model_id`（可空取默认） |
| GET /sessions/:id/images/:image_id | 既有鉴权 + 会话归属；原始字节，`Cache-Control: no-store` |

DTO 均 `deny_unknown_fields`；会话 DTO 含 `model_id`，Message 的 Image 只含 id。
设置页：服务商卡片（名称、类型、地址、key、测试）内嵌模型行；测试成功按返回生成模型草稿行；选择框按服务商分组。执行中禁用模型选择器。
**B 批 Web**：输入框支持选择/粘贴/拖入图片，本地预览可移除，成功后才清空；发送失败保留草稿。零模型时提示先去设置。
历史图片用带 Authorization 的 fetch 取 blob 创建 object URL，切页 revoke；不把 token 拼 URL。

## 九、兼容审查（B 批）

| 调用方 | 处理 |
|---|---|
| Message / ContentPart | 新 `Image`；穷举分支同步 |
| Kernel append_user_input / CLI / Gateway | 改 `IncomingPart`，文本生产者改 `Text` |
| Store append_input | 接 `NewInputPart`，图片行与消息同事务 |
| ModelRequest / openai request | 新 `images`；其余不变 |
| Gateway 消息 DTO 与 Web | 同次替换，不留双轨 |

分步：**内核**（mic-message → store images → core 入站/request → openai wire，可用 CLI/单测式手工验证）→ **HTTP + Web**（上传路由、读取路由、输入框与历史展示）。

| 人工验收 | 结果 |
|---|---|
| 空库启动、`-p` 纯文本 | Web 可设置；输入落盘并 ProviderFailed；无伪造调用记录；进程继续 |
| 首条/换默认/删默认/删最后条 | 默认不变量成立；旧选择不自动换服务商 |
| 修改 key 后继续工具往返 | 本轮用旧值，下一轮才用新值 |
| 执行中切模型 | 409，选择不变 |
| DB/响应/日志、key 文件丢失/密文篡改 | 无明文；明确 Err，不重建 key |
| 文字/纯图/混合消息、重启回放 | 顺序稳定，消息与图同事务可读 |
| 无 token、跨会话 image id、上传 path/URL | 拒绝 |
| 超数量/字节、非法格式 | 整条拒收，无孤立 blob；前端草稿保留 |
| 不支持图片的模型收到图 | 普通 ProviderFailed，不改任何状态 |

收尾 `cargo fmt`、`cargo clippy -- -D warnings`；改动覆盖已有测试时跑相应测试。不默认新增测试或提交；验收后 CLOSED。

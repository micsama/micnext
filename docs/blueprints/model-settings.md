# B2 草案：模型设置与 Web 图片输入

**状态**：A 批与 §十（服务商/模型分层、env 取 key、连通测试）已批准并实现；B 批（图片）未开工。§十 与前文冲突处以 §十 为准，批准后合并回正文。
**方向记录**：[B1](../brainstorm/model-settings.md)。阶段一契约有效，阶段二以本稿替代 [runtime-settings.md](runtime-settings.md) §四。

## 一、决定与收窄

| 项 | 决定 |
|---|---|
| API key | Web 可填；AES-256-GCM 加密入 SQLite；主密钥为 0600 本地文件，不在 data_dir 内 |
| 多模态 | 不建能力概念：无能力字段、无探测、无切模型限制；模型不支持就是普通 ProviderFailed |
| 模型呈现 | `ModelView` 留在 mic-message 原位，图片只是新的 `ContentPart`；不搬家、不重构 request |
| Provider | 仅 openai 协议（Generic/vLLM、DeepSeek、Ollama 预设）；工厂只两个方法；无模型列表加载、无自定义 headers、无 Env 取 key |
| 推理来源 | 保持现状（按请求模型名比较）；跨 endpoint 同名误回传不在本次处理 |
| 迁移 | 不取消现有 schema_migrations；只扩展当前 schema；模型 JSON 不版本化 |

仅有的跨实体规则：**执行中不能切会话模型（409）**。其余模型/图片规则一律不加。

## 二、用户行为

| 场景 | 结果 |
|---|---|
| 零模型启动 | 常驻服务照常启动，设置页可用，默认为 NULL |
| 创建首条 | 同事务设为默认 |
| 删除 | 软删除；删默认须先换默认，最后一条可删（默认回 NULL）；会话保留已删引用 |
| 新会话 | 建时取默认，无默认允许 NULL |
| NULL 会话收到输入 | 认领时取当时默认并写回 |
| 无模型/所选已删 | 输入照常落盘，认领后经既有 Notification → ProviderFailed 收尾；Web 提示去设置 |
| 执行中改条目 | 本轮配置与 key 在认领时读定，后续 run 才用新值 |
| 执行中切模型 | 409 `SessionExecuting`，无待切状态 |
| 发图 | 纯图片允许；纯文本去空白后为空且无图则拒；模型不收图 → 普通失败 |

## 三、依赖与所有权

| 位置 | 新增/调整 |
|---|---|
| mic-message | image.rs：`ImageId`/`ImageFormat`/`ImageRef`/`ImageData`；`ContentPart::Image(ImageRef)`；limits.rs：图片限额 |
| mic-store | models.rs（条目/默认/选择/认领）、secrets.rs（主密钥与加解密）、images.rs（blob 随消息同事务） |
| mic-core | model_settings.rs（工厂注册表与设置命令）、input.rs（图片嗅探）、request.rs（加载图片进 `ModelRequest`） |
| mic-provider-openai | 登记工厂；`ContentPart::Image` 序列化为 `image_url` data URI |
| mic-gateway | 模型 CRUD 路由、图片读取路由、消息 parts DTO |
| Web | 设置页“模型”表单；输入框图片选择/粘贴/拖入；历史图片展示 |
| bin/micnext | 提供 `core.key_file`；拒绝旧 `[models]` |

无新 crate、无新跨 crate 依赖。新外部依赖：store 加 `aes-gcm`（0.11 系，不兼容则改稿）、`getrandom`、`zeroize`；
Gateway 加 `base64`。**不引入 `image` crate**（只嗅探魔数，不解码）。

## 四、Provider 工厂

```rust
pub trait ProviderFactory: Send + Sync + 'static {
    fn kind(&self) -> &'static str;
    fn display_name(&self) -> &'static str;
    /// 保存前纯本地校验，返回规范化 JSON；不联网。
    fn check(&self, json: &str) -> Result<String, ConfigError>;
    /// 认领后创建本轮实例；key 缺省即无鉴权头。
    fn build(&self, json: &str, key: Option<SecretValue>)
        -> Result<Arc<dyn Provider>, ConfigError>;
}
pub struct ConfigError { pub field: String, pub message: String } // message 不含秘密
```

Registry 新增 `provider_factory(&mut self, impl ProviderFactory)`；删除 `Registry::provider`、Assembly 固定 provider/model、
`ModelsConfig` 与 TOML 条目装配。OpenAiModule 空配置也登记工厂。重复 kind → `DuplicateProviderKind`。
工厂只在启动登记一次，运行期设置不重跑 install/Service/迁移。

openai 配置（`deny_unknown_fields`）沿用现有字段，去掉全部 key/env 相关项：
`preset`（Generic/DeepSeek/Ollama；vLLM 用 Generic）、`base_url`（预设展开后显式保存，http(s)、无 userinfo/query/fragment）、
`model`（非空）、`max_tokens`、`reasoning_effort`。有 key 即 Bearer，无 key 不发鉴权头；key 无效由上游 401 走既有 Account 失败。
HTTP 边界以 RawValue 交工厂解析一次，之后只持强类型，`serde_json::Value` 不进入调度/Engine/表单。

## 五、存储类型与签名（mic-store）

```rust
pub struct ModelId(pub i64);
pub struct SecretValue; // 无 Debug/Display/Serialize，drop 时 zeroize
pub enum CredentialWrite { Keep, Clear, Set(SecretValue) } // 创建时 Keep → InvalidSecretEdit
pub struct ModelView { pub id: ModelId, pub name: String, pub kind: String,
                       pub config_json: String, pub key_set: bool }
pub struct ModelWrite { pub name: String, pub kind: String, pub config_json: String,
                        pub credential: CredentialWrite }
pub enum ClaimedModel {
    Missing,
    Deleted { id: ModelId, name: String },
    Selected { id: ModelId, name: String, kind: String,
               config_json: String, key: Option<SecretValue> },
}
pub struct ClaimedRun { pub run: Run, pub settings: RunSettings, pub model: ClaimedModel }

pub async fn models(&self) -> Result<Vec<ModelView>, StoreError>;
pub async fn model(&self, id: ModelId) -> Result<Option<ModelView>, StoreError>;
pub async fn create_model(&self, m: ModelWrite, now: i64) -> Result<ModelId, ModelSettingsError>;
pub async fn update_model(&self, id: ModelId, m: ModelWrite, now: i64) -> Result<(), ModelSettingsError>;
pub async fn delete_model(&self, id: ModelId, now: i64) -> Result<(), ModelSettingsError>;
pub async fn default_model(&self) -> Result<Option<ModelId>, StoreError>;
pub async fn set_default_model(&self, id: ModelId) -> Result<(), ModelSettingsError>;
pub async fn set_session_model(&self, s: SessionId, id: ModelId) -> Result<(), ModelSettingsError>;
pub async fn claim_next(&self, s: SessionId, now: i64) -> Result<Option<ClaimedRun>, StoreError>;
```

`Keep` 只是不动密文列，无需读出；`claim_next` 在认领事务内解密并写 run 快照（id + name，无秘密）。
`model()`/`models()` 不解密。

| 表 | 字段与约束 |
|---|---|
| core_models | id、name、kind、config_json、key_cipher（可空）、deleted_at、created_at、updated_at；活动名唯一 |
| core_settings | default_model_id 可空 FK；必须活动；零活动条目恰好 NULL |
| core_sessions | model_id 可空 FK；软删除不清引用 |
| core_runs | model_id、model_name 快照 |
| core_images | id、session_id、format、size_bytes、bytes blob；不可变 |

首条/默认联动、最后一条删除、切换与 executing 检查各在同一事务。

**主密钥**：`core.key_file`（config.toml，缺省为 config.toml 同目录 `master.key`）。
`Store::open(path, modules, key: SecretKeyFile)`（`SecretKeyFile(PathBuf)` 为 store 公开类型）；`open_in_memory` 签名不变，用随机内存 key。
首次且库内无密文时原子创建 32 字节 key，权限 0600；已有密文而 key 缺失 → 报错，不重建；格式/权限/AEAD 失败 → 启动 Err。
open 时校验全部现有密文。密文 = 12 字节 nonce + ciphertext + tag，无版本；AAD 绑定 model_id。
替换/软删除清空密文。备份须同时保留库与 key。

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
core 嗅探出 `ImageData` 后交 store `append_input(session, person, Vec<NewInputPart>, now)`，图片行与消息同事务；
`image(session, id)` 按 `core_images.session_id` 校验归属，不匹配返回 None。不接受 URL、path、现有 image id 作为上传。
会话创建仍先建会话再追加输入，不新增合并命令。

呈现：`ModelView`/`Message::model_view` 原位不动，`Image` 作为普通片段保留顺序。
`ModelRequest` 新增 `images: HashMap<ImageId, ImageData>`，core 的 `request::build` 按窗口内引用一次性读取（缺失报 `ImageMissing`）。
openai：纯文本 user 仍为字符串；含图时 content 数组按序发 text/image_url，`data:<MIME>;base64,...`。
工具结果里的 Image 与现有 File 一样显式拒绝映射。Boundary/压缩只改上下文，图片原件与 Web 历史保留。
**已接受的风险**：不限制窗口内图片总量，长会话多图请求可能很大，出问题再收紧。

## 七、Kernel、执行与错误

Kernel 新增：`provider_kinds`(同步) / `models` / `model` / `default_model` / `create_model` / `update_model` /
`delete_model` / `set_default_model` / `set_session_model`。写入参数 `ModelDraft { name, kind, config_json, credential }`；
`now` 由内核产生。写命令持一个异步写锁：取锁 → 工厂 `check` → Store 事务；失败不落部分状态。

Engine 持有工厂注册表。`claim_next` 后一次 `build` 创建实例，本轮重试/工具往返/absorb 复用。
`Missing`/`Deleted`/`build` 失败：RunStarted → Notification 落盘 → ProviderFailed → RunFinished；未真正调用则不写 `core_model_calls`。
CLI `-p` 走同一路径，仅取持久化默认，不新增管理命令。

| 归属 | 变体 | 结果 |
|---|---|---|
| ModelSettingsError | NotFound / Deleted / NameTaken / DefaultInUse / SessionExecuting / InvalidSecretEdit / Store | 404/409 |
| InputError | Empty / TooLarge / InvalidImage / ImageLimit | 400/413/422，不落消息 |
| ConfigError | field + 脱敏 message | 422 |
| StoreError 扩展 | SecretKeyIo / SecretKeyInvalid / SecretIntegrity / ImageMissing | 明确 Err |
| AssembleError 扩展 | DuplicateProviderKind / RemovedModelsConfig | 启动失败 |

KernelError 增加 ModelSettings / Input / Config 变体。HTTP 与日志不含 SecretValue、密文、上游原始响应体。

## 八、HTTP 与 Web

| 路由（/api） | 说明 |
|---|---|
| GET /model-kinds | `{ kinds:[{kind,display_name}], input_limits }` |
| GET /models；GET /models/:id | 只读 `ModelView`（`key_set` 不含 key） |
| POST /models；PUT /models/:id | `ModelDraft`；创建返回 `{id}`，修改 204；`credential` 为 keep/clear/set |
| DELETE /models/:id | 软删除；违反默认规则 409 |
| GET/PUT /models/default | `{model_id}` |
| PUT /sessions/:id/model | `{model_id}`；执行中 409 |
| POST /sessions；POST /sessions/:id/messages | `parts:[{kind:"text",text},{kind:"image",base64}]`；创建额外 `model_id`（可空取默认） |
| GET /sessions/:id/images/:image_id | 既有鉴权 + 会话归属；原始字节，`Cache-Control: no-store` |

DTO 均 `deny_unknown_fields`；新 body 不兼容旧 text-only，Web 与嵌入资源同次更新。会话 DTO 增加 `model_id`，Message 的 Image 只含 id。
设置页“模型”：增删改、默认、预设/地址/模型名（手填）/key 输入（写后只显示“已设置”）、清除 key。
输入框支持选择/粘贴/拖入图片，本地预览可移除，成功后才清空；发送失败保留草稿。零模型时提示先去设置。
执行中禁用模型选择器。历史图片用带 Authorization 的 fetch 取 blob 创建 object URL，切页 revoke；不把 token 拼 URL。

## 九、兼容审查与替换

| 调用方 | 处理 |
|---|---|
| Assembly / Module / Registry / bin | 公开贡献接口不兼容；同次迁移，旧 `[models]` 明确报错 |
| Store open / open_in_memory | open 增加主密钥参数；逐项核对调用方 |
| NewSession / Session | 增加可空 model_id；Root/Task/Triggered 创建点填明确值 |
| claim_next / Scheduler / run_once / Engine | 返回 `ClaimedRun`，全部更新 |
| Kernel append_user_input / CLI / Gateway | 改 `IncomingPart`，文本生产者改 `Text` |
| Message / ContentPart | 新 `Image`；穷举分支同步 |
| ModelRequest / openai request | 新 `images`；其余不变 |
| Web | 同次替换 DTO、选择器与图片展示，不留双轨 |

批准后按可编译小步：**A 批**（密钥与 Store → 工厂与认领 → 模型管理 UI）→ **B 批**（图片提交、读回、wire）。采用 parallel change，迁移完即删旧入口。
完成后同步更新既有专题蓝图（本稿批准前不改它们的已生效签名）。

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

## 十、修订 v3：服务商 / 模型分层（已实现）

**动机**：一个服务商（DeepSeek、某台 vLLM）下常有多个模型，连接与 key 不该每个模型重复填；服务商还需要“测试连接并拉取模型列表”。
替代前文：§一“无模型列表加载、无 Env 取 key”两项；§四、§五、§七、§八中以“模型条目”为单位的部分。

### 10.1 决定

| 项 | 决定 |
|---|---|
| 两层 | **服务商**（连接：类型、预设、地址、key）与**模型**（挂在服务商下：模型名、max_tokens、推理强度）。会话/默认/run 快照引用**模型**，不引用服务商 |
| 代码命名 | 实体叫 `Endpoint`（避免与运行期 `Provider` trait 混淆）；UI 文案用“服务商”“模型” |
| DeepSeek | 地址固定 `https://api.deepseek.com`，接口与 UI 都不提供 `base_url`；Ollama/通用保持可填（Ollama 留空取 `http://localhost:11434/v1`，通用必填） |
| key 来源 | 服务商保存了 key 用保存值；没保存则读环境变量：DeepSeek → `DEEPSEEK_API_KEY`，通用 → `OPENAI_API_KEY`，Ollama 无。UI 在 key 输入框旁标注“不填则读环境变量 X”。两处都没有属正常情况（如本地无鉴权服务）→ 不发鉴权头，上游拒绝时才表现为鉴权失败 |
| 测试 | 服务商表单“测试”按钮：用表单当前值（未保存也可）请求 `{base_url}/models`，成功返回模型名列表，UI 按返回**逐个生成模型行**（可删行/改参数后保存）；失败显示脱敏原因。不自动保存 |
| 推理强度/max_tokens | 归模型（同一 DeepSeek key 下不同模型可不同参数）；仍只有 DeepSeek 服务商的模型可填推理强度 |
| 删除 | 删服务商 = 级联软删其所有模型（清密文）；若其中含默认模型须先换默认（`DefaultInUse`）；删模型规则同前（默认须先换，最后一个可删） |
| 其余 | 执行中改配置/key 下一轮生效、执行中切模型 409、图片规则（B 批）不变 |

### 10.2 工厂（mic-core）

```rust
pub trait ProviderFactory: Send + Sync + 'static {
    fn kind(&self) -> &'static str;
    fn display_name(&self) -> &'static str;
    /// 服务商配置：纯本地校验，返回规范 JSON（预设展开 base_url）。
    fn check_endpoint(&self, json: &str) -> Result<String, ConfigError>;
    /// 模型配置：结合服务商配置校验（如推理强度只许 DeepSeek），返回规范 JSON。
    fn check_model(&self, endpoint_json: &str, model_json: &str) -> Result<String, ConfigError>;
    /// 认领后创建本轮实例；key 为已解析结果（存储值或 env），None=无鉴权头。
    fn build(&self, endpoint_json: &str, key: Option<SecretValue>, model_json: &str)
        -> Result<Arc<dyn Provider>, ConfigError>;
    /// 联网：请求模型列表；唯一联网的非 run 路径，带 CONNECT/总超时。
    async fn list_models(&self, endpoint_json: &str, key: Option<SecretValue>)
        -> Result<Vec<String>, ConfigError>;
    /// key 缺省时读哪个环境变量（用于 UI 标注与解析）；None=无约定。
    fn key_env(&self, endpoint_json: &str) -> Option<&'static str>;
}
```
`list_models` 因 trait 对象需要，用 `BoxFuture` 返回。env 读取在 **Kernel/Engine 侧**统一做（`resolve_key(stored, factory.key_env())`），
工厂只收最终 `Option<SecretValue>`；DeepSeek“必须有 key”由 `build/list_models` 在收到 `None` 时报 `ConfigError{field:"api_key"}`。

openai 配置：服务商 `{preset, base_url?}`（DeepSeek 禁填 `base_url`，`deny_unknown_fields` 之外再由 check 拒绝）；
模型 `{model, max_tokens?, reasoning_effort?}`。`GET {base}/models` 解析 `data[].id`，其余字段不消费（边界强类型，缺 `data` 即报错）。

### 10.3 存储（mic-store，直接改尚未提交的 CORE_V3，不另起迁移）

| 表 | 字段与约束 |
|---|---|
| core_endpoints | id、name、kind、config_json、key_cipher（可空）、deleted_at、created_at、updated_at；活动名唯一 |
| core_models | id、endpoint_id FK、config_json、deleted_at、created_at、updated_at；`model` 名在 config 内，同服务商活动模型名唯一（建 `model_name` 列并建唯一索引，由 store 从规范 JSON 外传入，避免解析 JSON） |
| core_settings / core_sessions / core_runs | 不变（仍引用 model id；run 快照 `model_name` 改为“服务商名 / 模型名”） |

```rust
pub struct EndpointId(pub i64);
pub struct EndpointView { id, name, kind, config_json, key_set }
pub struct EndpointWrite { name, kind, config_json, credential: CredentialWrite }
pub struct ModelView { id: ModelId, endpoint_id: EndpointId, name: String /*模型名*/, config_json }
pub struct ModelWrite { endpoint_id, name, config_json }
// ClaimedModel::Selected 增加 endpoint 配置：{ id, display_name, kind, endpoint_json, model_json, key: Option<SecretValue> }
// Store：endpoints/endpoint/create_endpoint/update_endpoint/delete_endpoint；
//        models(Option<EndpointId>)/model/create_model/update_model/delete_model/set_default_model/set_session_model（后四者签名不变）
```
密文 AAD 改绑 `endpoint-key:{endpoint_id}`；`verify_keys` 与主密钥规则不变。

### 10.4 Kernel 与 HTTP

Kernel：`endpoints/endpoint/create_endpoint/update_endpoint/delete_endpoint`（`EndpointDraft{name,kind,config_json,credential}`），
`create_model/update_model` 的 `ModelDraft` 改为 `{endpoint_id, name, config_json}`，新增
`test_endpoint(draft: EndpointDraft, existing: Option<EndpointId>) -> Result<Vec<String>, KernelError>`（`credential=Keep` 且给了 existing 时取库内 key，否则取 Set 值，再回退 env）。
`ModelView` 对外展示名 = “服务商名 / 模型名”由 Gateway 组合，store 不拼。

| 路由（/api） | 说明 |
|---|---|
| GET /model-kinds | 不变（另含各 kind 的预设元数据：是否可填地址、key 环境变量名，供 UI 标注） |
| GET/POST /endpoints；GET/PUT/DELETE /endpoints/:id | 服务商 CRUD；`credential` 为 keep/clear/set；DELETE 级联 |
| POST /endpoints/test | `{endpoint: EndpointDraft, existing_id?}` → `{models:[string]}`；失败 422 + 脱敏原因 |
| GET /models?endpoint_id= ；POST /models；PUT/DELETE /models/:id | 模型 CRUD（`endpoint_id,name,config`） |
| 其余 | `/models/default`、`/sessions/:id/model`、会话 `model_id` 不变 |

错误：`ModelSettingsError` 增 `EndpointNotFound`（404）、`EndpointDeleted`（409）；测试失败走 `ConfigError`（422）。日志/响应不含 key 与上游响应体。

### 10.5 Web

设置页“模型”改为**服务商列表**，每个服务商卡片下嵌套其模型行：
- 服务商表单：名称、类型预设（DeepSeek 隐藏地址；Ollama/通用显示地址）、key 输入（占位“已设置”或“不填则读环境变量 DEEPSEEK_API_KEY”）、清除 key、**[测试]**。
- 测试成功：显示“连通，N 个模型”，并把返回的模型生成为表单内的模型草稿行（已存在的同名行保留不重复），每行可删、可改 max_tokens/推理强度；保存时一并提交。
- 模型选择器分组显示 `服务商 / 模型`，默认标记在模型行上。
- 服务商/模型任一表单有未保存修改，沿用现有离开提示。

### 10.6 兼容审查与步骤

| 调用方 | 处理 |
|---|---|
| ProviderFactory（仅 mic-provider-openai 实现） | 签名整体替换；同次迁移 |
| Store 模型 API / `ModelView` / `ModelWrite` / `ClaimedModel` | 同次改，调用方仅 core、gateway |
| Kernel 模型命令 / `ModelDraft` | 改签名，仅 gateway 调用 |
| 网关 `/models*` DTO 与 Web | 同次替换，不留双轨；旧 `/models` 单层语义删除 |
| 迁移 | 开发库可删；CORE_V3 尚未提交，直接改 |

实现分步（均可编译）：store（表/类型/认领）→ core（工厂/Kernel/Engine 取 key）→ openai（拆配置、`list_models`、env）→ gateway → Web。
验收：DeepSeek 服务商无地址输入；不填 key 且设了 `DEEPSEEK_API_KEY` 可测试通过并跑通一轮；未设则测试/运行均给出可照做的中文提示；
测试返回 N 个模型即生成 N 行；删服务商级联且默认保护生效；DB/日志无明文 key。

### 10.x 连通测试错误（已实现）

`ProbeError`：`Network`（连不上/超时）、`Auth`（401/403）、`Unexpected`（其他状态或返回不是模型列表）；文案面向用户，不含 key 与上游响应体。网关映射 422。

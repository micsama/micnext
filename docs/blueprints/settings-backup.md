# B2：设置备份与库版本不符时自动重建

**状态：契约及补充已批准；第一阶段已实现并通过本地自动验收（2026-10-16），待 human 页面与真实微信验收、导出备份；第二阶段待之后实施。**

分两次发布：先「一、设置导出与导入」，human 导出备份后再发「二、自动重建」。

## 问题

库版本低于基线或高于程序时启动直接报错退出，只能手动删库；删库连同服务商 key、模型、人设、微信登录一起丢，得全部重配。

## 一、设置导出与导入

### 1.1 内容

只含设置，不含会话、消息、trace、图片：

- 服务商：名称、类型、配置、key 明文；其下模型：名称、配置、上下文窗口、支持图片、单价。
- 自建人设：名称、提示词（内置人设随程序自带，不导）。
- 通用设置：默认人设、通用提示、默认工作目录、最大轮数、压缩参数、默认模型、标题模型与推理强度。人设按名称引用；模型引用以 `{endpoint, model}` 分别存服务商名与模型名，避免名称包含 `/` 时歧义。
- 各 Channel 的账号：微信为当前账号的 `user_id`、`bot_id`、`bot_token`、`base_url`；无账号则不含。

### 1.2 加密与格式

- 明文 JSON 只在服务端内存里出现，HTTP 上只走密文。
- 用 `master.key`（AES-256-GCM，与 API key 同一把），AAD 为 `settings-export`，与 API key 的密文互不可替换。
- 文本形式：`micnext-settings:` + base64(`nonce || ciphertext || tag`)。
- 只能导入到持有同一 `master.key` 的实例。换机器需同时拷 `master.key`。
- 不带格式版本号；JSON 字段对不上即导入失败并说明原因。导入同时复用当前 Provider 的本地配置解析，写入前拒绝无效配置。
- 导出引用的模型须仍有效；已删除的标题模型须先重新选择或清空，导出报错，不生成引用缺失的备份。

### 1.3 导入语义

- 导入一律是覆盖：页面先确认「将用备份覆盖当前的服务商、模型、人设、通用设置与 Channel 账号，确定吗？」，确认后才提交。接口不判断库是否为空，也没有覆盖参数。空库确认一次无妨，这样也不会漏掉只改过通用设置的情况。
- 覆盖 = 让设置与备份一致，按名称对齐：
  - 同名服务商、模型、人设：原地更新，id 不变，已有会话的模型与人设选择继续有效。
  - 新的：创建。
  - 备份里没有的：软删除，与手动删除相同。
  - 通用设置与标题模型：整体替换。
  - 备份内引用的人设或模型名不存在 → 400，整个 core 事务不写。
- 写入前，gateway 拒绝备份中当前未注册的 Channel（400，说明名称），不静默丢弃账号。随后按注册的每个 Channel 调 `prepare_restore`，传该 Channel 的内容，缺席即 `None`。Channel 在这里把 JSON 一次解析为自己的强类型，返回持有该数据的恢复操作；格式、必填字段与配置约束不符返回 400。准备阶段不写库、不取消登录、不改变连接，也不请求网络。
- 所有 Channel 准备成功后，core 部分一个事务写入（key 用库内加密重新加密）；core 引用检查也在该事务内完成，失败时回滚。至此任何备份内容错误都不会改变设置或 Channel 状态。
- core 提交后，gateway 依次执行已准备的恢复操作，Channel 内部直接使用强类型，不再解析 JSON：
  - 微信 `Some`：走与扫码登录成功相同的路径。同一账号更新 token，不同账号新建身份与根会话并切为当前账号；取消进行中的登录，重新连接。token 已失效时显示「需重新登录」，与现在一致。
  - 微信 `None`：断开连接，清空当前账号（`active_user_id = NULL`），账号行与历史保留。页面回到未登录状态，可重新扫码。
- Channel 执行恢复时的存储或运行错误不回滚 core，其余 Channel 照常恢复；汇总失败的 Channel 与原因，返回 500「设置已导入；微信账号恢复失败：<原因>」，页面原样显示并刷新设置页与 Channel 状态。失败原因使用可公开的说明，不含 token 或备份原文。token 失效属于连接状态，不算备份格式错误。

### 1.4 契约

**mic-store**

```rust
pub struct SettingsExport { /* 1.1 的 core 部分，Serialize + Deserialize，deny_unknown_fields */ }

impl Store {
    pub async fn export_settings(&self) -> Result<SettingsExport, StoreError>;
    /// 按 1.3 覆盖；引用不存在 → SettingsError::ImportInvalid(说明)。
    pub async fn import_settings(&self, s: SettingsExport, now: i64) -> Result<(), SettingsError>;
    /// master.key 加解密，AAD `settings-export`；文本含 `micnext-settings:` 前缀。
    pub fn seal_export(&self, plain: &SecretValue) -> String;
    /// 前缀、base64、认证任一失败 → StoreError::ExportUndecryptable。
    pub fn open_export(&self, text: &str) -> Result<SecretValue, StoreError>;
}
```

**mic-core**

```rust
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SettingsBackup {
    pub core: SettingsExport,
    pub channels: BTreeMap<String, serde_json::Value>,
}

impl Kernel {
    /// 已注册的 Channel 登录能力，按名称排序；gateway 导入、导出编排使用。
    pub fn channel_setups(&self) -> BTreeMap<&'static str, Arc<dyn ChannelSetup>>;
    pub async fn export_settings(&self) -> Result<SettingsExport, KernelError>;
    pub async fn import_settings(&self, s: SettingsExport) -> Result<(), KernelError>;
    pub fn seal_backup(&self, b: &SettingsBackup) -> String;
    /// 解密失败或 JSON 不符 → KernelError::BackupInvalid(说明)。
    pub fn open_backup(&self, text: &str) -> Result<SettingsBackup, KernelError>;
}

pub trait ChannelSetup {
    // 既有方法不变
    fn export(&self) -> BoxFuture<Result<Option<serde_json::Value>, ChannelSetupError>>;
    /// 只解析输入，返回持有 Channel 强类型数据的恢复操作，无副作用。
    fn prepare_restore(
        &self,
        account: Option<serde_json::Value>,
    ) -> Result<Box<dyn ChannelRestore>, ChannelSetupError>;
}

/// 消费已解析数据；唯一调用方是 gateway 的导入流程。
pub trait ChannelRestore: Send {
    fn restore(self: Box<Self>) -> BoxFuture<Result<(), ChannelSetupError>>;
}
```

`ChannelSetupError` 增加 `BackupInvalid { reason: String }`（内容不符本 Channel 格式，仅在准备阶段返回）。微信的恢复操作持有 `Option<微信账号强类型>` 与现有命令发送端，执行时交给既有账号协调流程；不用 `Any`、向下转型或重复反序列化。

**HTTP**（gateway 编排：导出 = core + 各 Channel `export` → `seal_backup`；导入 = `open_backup` → 检查 Channel 名称 → 各 Channel `prepare_restore` → `import_settings` → 执行各恢复操作）

| 方法与路径 | 请求 | 成功 | 失败 |
|---|---|---|---|
| `POST /api/settings/export` | 无 | 200 `{"text": "micnext-settings:…"}` | 500 |
| `POST /api/settings/import` | `{"text": "…"}` | 204 | 400 无法解密、密钥不符、内容不符、引用缺失、未注册的 Channel（均未写入）；500 存储或运行错误，core 已提交时明确提示部分成功（文案如 1.3） |

**Web**
- 设置页加「导出设置」：复制到剪贴板。剪贴板不可用（非 HTTPS 的远程访问）时在只读文本框里显示，供手动复制。
- 加「导入设置」：粘贴，确认覆盖后提交；成功或提示 core 已导入时刷新设置页与微信状态，保留结果提示。

## 二、库版本不符时自动重建

- 启动打开主库遇到 `SchemaRebuild`（低于基线）或 `SchemaAhead`（高于程序）时：
  1. 把 `micnext.db` 及 `-wal`、`-shm` 改名为 `micnext.db.bak-<YYYYMMDD-HHMMSS>`（同名后缀）。
  2. 新建空库，继续启动。
- `master.key` 不动，所以本机导出的设置可直接导入新库。
- 用户可见：启动日志 WARN「库版本 X 与程序 Y 不符，旧库已备份为 …，可在设置页导入设置」，日志页可看。页面不另加提示（需要新接口，不值得）。
- 只处理这两种版本不符；其他打开错误（权限、损坏、密钥不符）仍报错退出。
- `logs.db` 不在此列。
- 契约：`bin/micnext` 内部行为，无公开接口变化。

## 三、调用方影响

- `mic-store`：新增 `base64 = "0.22"`（复用项目已有版本，用于备份文本编码）；`SettingsExport`、四个方法；`SettingsError::ImportInvalid`、`StoreError::ExportUndecryptable`。
- `mic-core`：`SettingsBackup`、`Kernel` 四个备份方法与 `channel_setups` 枚举接口、`KernelError::BackupInvalid`；`ChannelSetup` 加 `export`/`prepare_restore`，新增 `ChannelRestore`；`ChannelSetupError::BackupInvalid`。
- `mic-channel-wechat`：实现导出、准备恢复与恢复操作；准备时解析账号，执行时有账号则复用登录成功分支，无账号则断开并清空当前账号。
- `mic-gateway`：两条路由。
- Web：设置页导出与导入。
- `bin/micnext`：打开主库的错误分支。

## 四、验收

1. **store 测试**：
   - 导出后导入空库，服务商、key、模型、人设、设置一致。
   - 覆盖时同名保留 id、多余项软删除、引用缺失整体不写。
   - 篡改密文、换 key 时解密失败。
2. **隔离实例**（真实库副本）：
   - 导出，删库重启后导入。
   - 模型可用，微信账号恢复并连接；导入不含微信的备份后微信断开、回到未登录。
   - 导出文本里不出现任何 key 明文。
3. **导入编排测试**：
   - 微信账号格式错误或含未注册的 Channel → 400，core、账号、连接与进行中的登录不变。
   - core 引用缺失 → 400，准备好的 Channel 操作不执行。
   - core 提交后一个 Channel 恢复失败，其余仍执行；返回 500 且明确 core 已导入，原因不含凭证。
4. **自动重建**：把库的 core 版本改高后启动，旧库被改名、新库可用、日志有提示。
5. **收尾**：`cargo fmt`、`clippy`、`bun run check`。页面交 human。

## 五、实现准备补充（已批准）

现有 `Kernel` 只提供按名称查询单个 Channel 的 `channel_setup(name)`，没有枚举入口。gateway 无法据此实现「遍历全部已注册 Channel」及「拒绝未知 Channel」。补充上面的 `channel_setups()`，返回已有注册事实的有序视图，不在 gateway 另存 Channel 名单。另明确备份 base64 编码依赖，沿用项目已有版本。主体导入语义不变。

## 六、第一阶段验收记录（2026-10-16）

- 保留 store 的 3 项模块功能测试（往返及覆盖、事务回滚、密钥与 AAD 隔离）及 gateway 的 1 项多 Channel 编排测试；字段解析细节由隔离功能验收覆盖。
- 真实主库经 SQLite 在线备份得到隔离副本，在副本导出、加密、解密并导入另一空库；core 设置完全一致。原库与原密钥未修改。
- 隔离 HTTP 实例：加密导出/导入、同名 ID 与已有会话引用保留、恢复配置后的本机模拟模型调用、同一密钥下空库重启恢复通过。
- 格式错误、未知 Channel、引用缺失、无效 Provider 配置、负单价、未知嵌套字段均返回 400，库内设置不变。
- 微信仅用本地假账号与本机不可达 HTTPS 地址验收：同账号更新 token 保留根会话，不同账号创建新根会话，缺席账号断开并清空当前选择，历史账号保留。未连接真实微信。
- Channel 运行失败场景：core 已提交、微信因不可用工作目录无法建会话时，返回 500 与明确的部分成功提示，页面可见连接状态同步为异常，凭证不出现在响应中。
- `cargo test --workspace --offline` 全部 45 项通过；`cargo fmt --check`、`cargo clippy --offline --all-targets -- -D warnings`、`bun run check`、Web 构建与 `git diff --check` 通过。
- 页面观感、剪贴板实际体验与真实微信登录恢复交 human；完成导出备份后再实施第二阶段。此主题尚未 CLOSED，保留 todo 条目。

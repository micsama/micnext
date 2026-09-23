# B2: mic-core 模块装配（Module / Registry / Service / Kernel 句柄）

**状态**: 待批准；微信设置采用保存后重启生效，凭据写入和重启按钮仍待 Channel/Gateway B2
**来源**: [`product-roadmap.md`](../brainstorm/product-roadmap.md) §2.1、§2.2、§2.4、§四-1
**依赖不变量**: 定义在 `mic-core`；模块 crate 依赖 `mic-core`（+ 需要的下层 crate），
模块之间不互相依赖，只由二进制装配。

本文只写现行契约；修订过程见 git 历史。

## 一、用户视角的效果

- **加功能** = 新 crate + 装配根一行 + cargo feature；**删功能** = 关 feature。
- **开关功能** = 配置文件里写/删一个段。配置文件本身就是"这个 agent 能做什么"的清单。
- 配错（启用了没编译进来的模块、工具重名、配置字段写错）→ 启动直接报错并指明模块，
  不会带病运行。
- 后台任务（如 cron 调度器）自身出错退出 → 整个进程报错退出，由外部守护（launchd 等）
  拉起；不静默失去某项能力。

## 二、范围

本文只定装配框架：`Module`、`ModuleConfig`、`Registry`、`Service`、`Kernel` 句柄、
启动顺序与错误。**各 port 的 trait 本身不在本文**：`Tool`（mic-tool B2）、`Channel` /
`DirectRoute`（roadmap §四-2～3 Gateway B2）、`Provider`、`Hook`、`ContextContributor`
（roadmap §四-8）各自在对应 B2 定义，批准时连同 `Registry` 上的登记方法一起加入——不预留
没有实现的方法。本文立即生效的登记项只有 `migrations` 与 `service`。

## 三、公开类型与签名

```rust
pub type BoxError = Box<dyn std::error::Error + Send + Sync>;

pub trait Module {
    /// 配置段名、迁移 module 名、表前缀共用此名。
    fn name(&self) -> &'static str;
    /// 第一阶段：同步、纯声明。解析自己的配置段并登记贡献；不做 I/O，拿不到 Store。
    fn install(&self, reg: &mut Registry, cfg: ModuleConfig) -> Result<(), BoxError>;
}

/// 本模块配置段的原始内容，只能由模块自己一次 parse 成强类型。
pub struct ModuleConfig(/* toml::Table */);
impl ModuleConfig {
    pub fn parse<T: serde::de::DeserializeOwned>(self) -> Result<T, BoxError>;
}

pub struct Registry { /* 私有 */ }
impl Registry {
    pub fn migrations(&mut self, m: &'static [mic_store::Migration]);
    pub fn service(&mut self, s: impl Service);
    // 以下随各 port 的 B2 加入，名字先占定：
    // tool / channel / provider / hook / context(slot, …)
}

pub trait Service: Send + 'static {
    /// 第二阶段：Store 已打开、迁移已跑完。`stop` 触发后应尽快返回 `Ok(())`。
    /// 返回 `Err` 或在未收到 `stop` 时返回 → 进程以错误退出（见 §五）。
    fn run(
        self: Box<Self>,
        kernel: Kernel,
        stop: tokio_util::sync::CancellationToken,
    ) -> Pin<Box<dyn Future<Output = Result<(), BoxError>> + Send>>;
}

/// 模块拿到的内核窄接口。Clone 廉价（内部 Arc）。核心表只经这些方法写。
#[derive(Clone)]
pub struct Kernel { /* 私有 */ }
impl Kernel {
    pub async fn create_session(&self, s: mic_store::NewSession) -> Result<SessionId, KernelError>;
    /// 写入后唤醒调度；与 Channel 入站同一路径。
    pub async fn append_user_input(&self, i: mic_store::UserInput) -> Result<SessionEntryId, KernelError>;
    /// 与 `send_message` 工具同一执行路径（roadmap §2.4），随 Channel B2 一起落地。
    pub async fn send_message(&self, m: SendMessage) -> Result<SessionEntryId, KernelError>;
    /// 只许访问本模块 `{name}_` 前缀的表；由模块单元测试把关（mic-store §4.5）。
    pub async fn with_module_tx<R, F>(&self, f: F) -> Result<R, KernelError>
    where
        F: FnOnce(&mic_store::rusqlite::Transaction) -> mic_store::rusqlite::Result<R> + Send + 'static,
        R: Send + 'static;
}

/// 装配入口，由二进制调用。
pub struct Assembly { /* 私有 */ }
impl Assembly {
    pub fn new(modules: Vec<Box<dyn Module>>, config: toml::Table) -> Result<Self, AssembleError>;
    /// 打开 Store、启动 Service 与内核主循环，直到 `stop` 或任一 Service 失败。
    pub async fn run(self, stop: CancellationToken) -> Result<(), RunError>;
}
```

`SendMessage`、`KernelError` 的具体变体随调度器 / Channel B2 定；本文只定它们挂在
`Kernel` 上。

## 四、配置

单个 TOML 文件。`[core]` 段归内核（数据目录、模型、persons 等，形状随各 B2 定），
其余每个顶层段对应一个模块：

```toml
[core]
data_dir = "~/.micnext"

[fs]            # 空表也算启用

[shell]

[cron]
tick_secs = 30
```

- 段存在 = 启用；段缺失 = 编译进来但不启用（不 install，不建表，不起 Service）。
- 顶层段名不是 `core` 也不是任何已编译模块名 → `AssembleError::UnknownModule`。
- 模块名不得为 `core`：内部互信，不做运行时检查。

Web 设置页需要微信注册/连接入口。配置保存后重启生效，不做热更新；注册凭据的
保存格式、写入错误和连接状态回显仍待接入接口核实后在 Channel/Gateway B2 定义。

## 五、启动与运行顺序

1. 二进制按 cargo feature 组装 `Vec<Box<dyn Module>>`，读配置文件（外部输入，TOML 解析失败即报错）。
2. `Assembly::new`：逐个已启用模块 `install`，冲突即返回错误。
3. `run`：汇总 migrations → `Store::open` → `interrupt_stale_queries` 等内核启动步骤 →
   为每个 Service `tokio::spawn`，同时跑内核主循环。
4. 退出：外部 `stop`（Ctrl-C/SIGTERM）→ 广播给所有 Service 并等待返回；任一 Service
   失败 → 广播 `stop`，等其余返回后 `RunError::Service{module, source}`。

## 六、错误

```rust
#[derive(Debug, thiserror::Error)]
pub enum AssembleError {
    #[error("config section [{name}] has no compiled-in module")]
    UnknownModule { name: String },
    #[error("module `{name}` registered twice")]
    DuplicateModule { name: &'static str },
    #[error("module `{module}` failed to install: {source}")]
    Install { module: &'static str, source: BoxError },
    // 随各 port 加入：DuplicateTool / DuplicateChannel / DuplicateProvider …
}

#[derive(Debug, thiserror::Error)]
pub enum RunError {
    #[error(transparent)]
    Store(#[from] mic_store::StoreError),
    #[error("service of module `{module}` failed: {source}")]
    Service { module: &'static str, source: BoxError },
}
```

`install` 里配置 parse 失败直接经 `ModuleConfig::parse` 的 `BoxError` 归入 `Install`。

## 七、约束

- 模块之间不互相依赖。模块 A 需要模块 B 的能力 → 在 core 定一个窄 port，B 贡献实现
  （同 roadmap §2.4 `send_message` 模式）。
- 一个模块可贡献任意多种 port；一个模块 = 一个 crate。
- 同一槽位的多个贡献（context、hook）按模块在装配根里的顺序排列，不设数字优先级。

## 八、副作用与依赖

- `install` 无副作用；`run` 打开数据库、起后台任务。
- `mic-core` 新增外部依赖：`toml`（配置解析）、`tokio-util`（仅 `CancellationToken`，
  体积小）。已有：`mic-message`、`mic-store`、`mic-tool`。
- `mic-store` 需要公开 `Migration`、`NewSession`、`UserInput` 与 `rusqlite` re-export
  ——已在 mic-store B2 内，无新增。

## 九、调用方

| 调用方 | 用途 | 兼容性 |
|---|---|---|
| `bin/micnext` | 装配根：`Assembly::new(mods, cfg)?.run(stop)` | 新契约（现为空壳） |
| `mic-gateway`、微信适配模块 | 实现 `Module` 或通过 Gateway 接入；Channel 登记随 Gateway B2 | 新契约（`mic-channel-web` 改名为 `mic-gateway` 随 Gateway B2；微信进程形态待核实） |
| `mic-cron` 等功能模块 | `Module` + `migrations` + `Service` + `Kernel` | 新 crate，建时各走 B2 |

当前无下游代码，不需要 parallel change。

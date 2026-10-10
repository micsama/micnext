# B2: mic-core 模块装配（Module / Registry / Service / Kernel 句柄）

**状态**: 已实现部分 CLOSED（2026-09-23，`crates/mic-core`、`bin/micnext`）；`[models]` 分组与 `provider` 登记（随 provider-port）、`tool` 登记与 `DuplicateTool`（随 [mic-tool](mic-tool.md) §三.5）、`Module::activation` 与 `Activation`（随 [tools-basic](tools-basic.md) §三.1）修订已批准并实现；占定方法见 §三。执行主路径（M6）对本文 §三、§四、§六、§七 的修订（`run_once`、`Kernel::owner/append_user_input/subscribe`、`[core] owner`（`max_turns` 已移到网页设置，见 [runtime-settings](runtime-settings.md)）、`[models] default` 必填、新错误变体、`-p`）见 [run-execution](run-execution.md)，以该文为准。微信设置采用保存后重启生效，凭据写入和重启按钮仍待 Channel/Gateway B2
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
配置位置、启动顺序与错误，以及二进制入口的命令行。**各 port 的 trait 本身不在本文**：
`Tool`（mic-tool B2）、`Channel` / `DirectRoute`（Gateway B2）、`Provider`、`Hook`、
`ContextContributor`（roadmap §四-8）各自在对应 B2 定义，批准时连同 `Registry` 上的登记
方法一起加入。本文立即生效的登记项为 `migrations`、`service` 与 `provider`（trait 见 provider-port B2）。

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
pub struct ModuleConfig(/* toml::Value */);
impl ModuleConfig {
    pub fn parse<T: serde::de::DeserializeOwned>(self) -> Result<T, BoxError>;
}

pub struct Registry { /* 私有 */ }
impl Registry {
    pub fn migrations(&mut self, m: &'static [mic_store::Migration]);
    pub fn service(&mut self, s: impl Service);
    /// 模型模块按 `[models.<name>]` 条目名登记（provider-port B2）。
    pub fn provider(&mut self, name: impl Into<String>, p: impl Provider);
    /// 渠道 Root 会话的呈现说明，键显式传入（[channel-prompt](channel-prompt.md)）。
    pub fn channel_prompt(&mut self, channel: &'static str, prompt: &'static str);
    // 以下随各 port 的 B2 加入，名字先占定：
    // tool / channel / provider / hook / context(slot, …)
}

pub trait Service: Send + 'static {
    /// 第二阶段：Store 已打开、迁移已跑完。`stop` 触发后应尽快返回 `Ok(())`。
    /// 返回 `Err`、panic、或在未收到 `stop` 时返回 → 进程以错误退出（见 §五）。
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
    /// Channel 用：按 (channel, chat) 原子取得或新建 Root 会话。
    pub async fn resolve_root_session(&self, channel: &str, chat: &str, init: mic_store::NewSession)
        -> Result<mic_store::Session, KernelError>;
    /// Task/Triggered 用。
    pub async fn create_session(&self, s: mic_store::NewSession) -> Result<SessionId, KernelError>;
    /// 只许访问本模块 `{name}_` 前缀的表；由模块单元测试把关（mic-store §4.5）。
    pub async fn with_module_tx<R, F>(&self, f: F) -> Result<R, KernelError>
    where
        F: FnOnce(&mic_store::rusqlite::Transaction) -> mic_store::rusqlite::Result<R> + Send + 'static,
        R: Send + 'static;
}

#[derive(Debug, thiserror::Error)]
pub enum KernelError {
    #[error(transparent)]
    Store(#[from] mic_store::StoreError),
    // 调度、投递相关变体随 M6 / Channel B2 加入。
}

/// 装配入口，由二进制调用。
pub struct Assembly { /* 私有 */ }
impl Assembly {
    pub fn new(modules: Vec<Box<dyn Module>>, config: toml::Table) -> Result<Self, AssembleError>;
    /// 打开 Store、启动 Service（M6 起含内核主循环），直到 `stop` 或任一 Service 失败。
    pub async fn run(self, stop: CancellationToken) -> Result<(), RunError>;
}
```

### 占定、暂不实现的 `Kernel` 方法

| 方法 | 语义 | 落地时机 |
|---|---|---|
| `append_user_input(session, person, parts: Vec<ContentPart>) -> MessageId` | 写入未认领输入（时间戳由内核打）后唤醒调度；与 Channel 入站同一路径 | 已随 M6 实现 |
| `send_message(SendMessage) -> MessageId` | 与 `send_message` 工具同一执行路径（roadmap §2.4）；结果只承诺已排队 | 随该工具与出站设计（v0b） |

`SendMessage` 字段先占定为 `text`、`to: Option<_>`（收件人）、`via: Option<_>`（Channel；
省略 = 回来源会话），以后可能加文件等附件；各字段的具体类型随落地时的 B2 定。

## 四、配置

单个 TOML 文件。`[core]` 段归内核，其余每个顶层段对应一个模块：

```toml
[core]
data_dir = "~/.micnext"   # 可省略

[fs]            # 空表也算启用

[cron]
tick_secs = 30
```

- 段存在 = 启用；段缺失 = 编译进来但不启用（不 install，不建表，不起 Service）。
- 顶层段名不是保留段（`core`、`models`）也不是任何已编译的普通模块名 →
  `AssembleError::UnknownModule`。
- 模块名不得为 `core`、`models`：内部互信，不做运行时检查。
- **拒绝未知字段**：`[core]` 与各模块的配置结构体一律 `#[serde(deny_unknown_fields)]`；
  框架无法替模块强制，靠 review 守。
- **`[core]` 现有字段**：`data_dir`（可选）。owner person 等随 M6 B2 加入。
  `[core]` 段本身可省略；解析失败 → `AssembleError::Core`。

### 按用户概念分组的段：`[models]`

用户眼里是一类东西的配置放在同一个表下，不按 crate 分段（CLAUDE.md "配置合并同类项"）。
目前只有模型：

```toml
[models]
default = "ds"          # 当前用哪个

[models.ds]
kind = "openai"         # 由哪个模型模块实现
model = "deepseek-chat" # 其余字段归该模块解析
```

- `[models]` 与 `core` 一样是保留段名，由内核解析：字符串键 `default`，其余每个子表是
  一个模型条目，必须有 `kind`。
- 模型模块（如 `mic-provider-openai`，`name() = "openai"`）**没有顶层段**：有条目的
  `kind` 等于它的名字才 install，`ModuleConfig` 为 `{条目名 → 去掉 kind 的条目}`；
  模块按条目名 `Registry::provider` 登记。
- `kind` 没有对应的已编译模块 → `AssembleError::UnknownModelKind`；`default` 不是任何
  条目名 → `AssembleError::UnknownDefaultModel`；`[models]` 形状不对 → `AssembleError::Models`；
  模型模块又出现在顶层段 → `AssembleError::UnknownModule`（它不接受顶层段）。
- `default` 与 `[models]` 目前可省略；由第一个消费者 M6 改为必填。
- 以后的模型路由（fallback、按复杂度选模型）是另一种 `kind`，引用其它条目名，形状不变。

### 位置（XDG 约定，macOS 同样适用）

| 内容 | 默认 | 覆盖 |
|---|---|---|
| 配置文件 | `$XDG_CONFIG_HOME/micnext/config.toml`，未设则 `~/.config/micnext/config.toml` | `micnext --config <path>` |
| 数据库 | `$XDG_DATA_HOME/micnext/micnext.db`，未设则 `~/.local/share/micnext/micnext.db` | `[core] data_dir`（库为 `<data_dir>/micnext.db`；须为绝对路径或 `~/` 开头，相对路径报错——常驻时工作目录不可预期） |
| 日志 | stderr；以后落文件放 `$XDG_STATE_HOME/micnext/` | — |

- 配置文件不存在：未给 `--config` 时在默认位置生成带注释的模板（权限 600，模板即
  `bin/micnext/src/default-config.toml`），在 stderr 用中文提示路径后照常启动；显式
  `--config` 指向的文件不存在 → 报错（多半是路径写错，不替用户猜）。
- 数据目录不存在 → `run` 自动创建。
- 配置文件路径解析归二进制，数据目录解析归 `mic-core`（`[core]` 的一部分）。

Web 设置页需要微信注册/连接入口。配置保存后重启生效，不做热更新；注册凭据的
保存格式、写入错误和连接状态回显仍待接入接口核实后在 Channel/Gateway B2 定义。

## 五、启动与运行顺序

1. 二进制按 cargo feature 组装 `Vec<Box<dyn Module>>`，读配置文件（外部输入，TOML 解析失败即报错）。
2. `Assembly::new`：检查模块重名 → 解析 `[models]`、按 `kind` 分组 → 检查未知段 →
   解析 `[core]` → 按装配根顺序逐个已启用模块 `install` → 核对 `default`，冲突即返回错误。
3. `run` / `run_once` 共用启动：创建数据目录、独占锁 → 汇总 migrations → `Store::open` → 收尾遗留 run →
   `hold_unclaimed_inputs` 将遗留未认领输入登记为 held 并追加会话通知，不自动执行；
   之后 `run` 启动调度与 Service，只由运行期新输入 wake；`run_once` 不起 Service，只执行新 prompt。
   此修订随 [微信 B2 Phase 1](wechat-channel.md#33-store-的启动待命契约) 于 2026-10-09 批准。
4. 退出：外部 `stop`（Ctrl-C/SIGTERM）→ 广播给所有 Service 并等待返回；任一 Service
   失败 → 广播 `stop`，等其余返回后报第一个错误。没有 Service 时 `run` 等待外部 `stop`。

时间戳单位统一为 Unix 毫秒（见 mic-store B2）。

## 六、错误

```rust
#[derive(Debug, thiserror::Error)]
pub enum AssembleError {
    #[error("配置段 [{name}] 没有对应的模块（名字写错，或该模块未编译进来）")]
    UnknownModule { name: String },
    #[error("模块 `{name}` 重复注册")]
    DuplicateModule { name: &'static str },
    #[error("[core] 配置有误")]
    Core { source: BoxError },
    #[error("模块 `{module}` 装配失败")]
    Install { module: &'static str, source: BoxError },
    #[error("[models] 配置有误")]
    Models { source: BoxError },
    #[error("模型 `{model}` 的 kind = \"{kind}\" 没有对应的模块（写错，或未编译进来）")]
    UnknownModelKind { model: String, kind: String },
    #[error("[models] default = \"{name}\" 不是任何模型条目")]
    UnknownDefaultModel { name: String },
    // 随各 port 加入：DuplicateTool / DuplicateChannelSetup / DuplicateChannelPrompt …
    // 新增变体不破坏调用方：二进制只经 anyhow 收口，无穷尽匹配。
}

#[derive(Debug, thiserror::Error)]
pub enum RunError {
    #[error("无法创建数据目录 {path}")]
    DataDir { path: PathBuf, source: std::io::Error },
    #[error(transparent)]
    Store(#[from] mic_store::StoreError),
    /// `Err` 返回或 panic。
    #[error("模块 `{module}` 的后台任务失败")]
    Service { module: &'static str, source: BoxError },
    #[error("模块 `{module}` 的后台任务未收到停止信号就退出了")]
    ServiceExited { module: &'static str },
}
```

`install` 里配置 parse 失败直接经 `ModuleConfig::parse` 的 `BoxError` 归入 `Install`；
模块段不是表同样归入 `Install`。

## 七、二进制入口（M11）

- 命令行：`micnext [--config <path>]`；其它参数 → 报用法错误。一次性调试 `-p` 随 M6 B2。
- Ctrl-C 与 SIGTERM 触发 `stop`，干净退出码 0；装配或运行错误 → 非 0。
- 面向用户的提示与错误（首次生成配置、用法、配置错误）用中文直接写 stderr：
  `错误：…` 加逐行 `原因：…`，先说明怎么修；运行日志仍走 tracing。
- 日志 `tracing` 输出到 stderr，默认 `info`，`RUST_LOG` 覆盖（格式非法即报错）。

## 八、约束

- 模块之间不互相依赖。模块 A 需要模块 B 的能力 → 在 core 定一个窄 port，B 贡献实现
  （同 roadmap §2.4 `send_message` 模式）。
- 一个模块可贡献任意多种 port；一个模块 = 一个 crate。
- 同一槽位的多个贡献（context、hook）按模块在装配根里的顺序排列，不设数字优先级。

## 九、副作用与依赖

- `install` 无副作用；`run` 创建数据目录、打开数据库、起后台任务。
- `mic-core` 外部依赖：`toml`（配置解析）、`tokio-util`（仅 `CancellationToken`）、
  `tokio`、`serde`、`thiserror`、`tracing`。内部：`mic-message`、`mic-store`、`mic-tool`。
- `bin/micnext` 外部依赖：`anyhow`、`tokio`、`tokio-util`、`toml`、`tracing`、
  `tracing-subscriber`；内部只依赖 `mic-core`（其余 crate 随各模块装配时加入）。
- `mic-store` 需要公开 `Migration`、`NewSession`、`UserInput` 与 `rusqlite` re-export
  ——已在 mic-store B2 内，无新增。

## 十、调用方

| 调用方 | 用途 | 兼容性 |
|---|---|---|
| `bin/micnext` | 装配根：`Assembly::new(mods, cfg)?.run(stop)` | 新契约（现为空壳） |
| `mic-gateway`、微信适配模块 | 实现 `Module` 或通过 Gateway 接入；Channel 登记随 Gateway B2 | 已改名为 `mic-gateway`（[gateway](gateway.md)）；微信进程形态待核实 |
| `mic-cron` 等功能模块 | `Module` + `migrations` + `Service` + `Kernel` | 新 crate，建时各走 B2 |

当前无下游代码，不需要 parallel change。

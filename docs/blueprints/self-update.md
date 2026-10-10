# B2：Web 一键更新（原地 pull → 构建 → exec）

**状态：r3 已批准并实现，fmt/clippy/web check 通过，本机假部署仓库端到端通过；待服务器人工验收后 CLOSED（2026-10-10）。r2 的代理处理、previous 备份、构建超时与进程组取消均已删除，见 §七。**

依据：[B1](../brainstorm/self-update.md)。手动部署的个人服务：在网页点一下，原部署目录 pull、复用缓存构建，成功后在原终端以原参数 exec 新程序。恢复由人工处理。

## 一、用户行为

开发者诊断页新增「更新」页签：

- 未配置 `update_repo`：按钮不可用，提示在配置里加哪一行。
- 点击 → 确认框「会中断正在进行的对话，服务将重启」→ 显示阶段：检查 → 拉取（第 n/5 次）→ 构建 → 重启；运行中显示命令输出末尾。
- 拉取后提交未变：显示「已是最新（<commit>）」，不构建不重启。
- 失败：显示失败阶段、原因、输出末尾和更新前提交号；旧服务继续运行。可再次点击。
- 重启：页面核对新进程报告的版本，与目标提交一致才显示「已更新」；随机 token 时需到终端取新地址重新登录。
- 页面关闭不取消更新。

部署约定：启动前先在 shell 里开代理，例如 `proxy && ./build.sh run --config /path/config.toml`。程序不读、不改、不检查代理；git/bun/cargo 子进程继承启动环境。忘开代理的表现就是 pull 失败、页面可见。

## 二、流程与副作用

全部子进程固定 argv、`current_dir = update_repo`、环境变量原样继承、stdin 为 null；stdout/stderr 合并进输出末尾缓冲。

1. **检查**（不重试）：
   - `git status --porcelain` 输出非空 → 失败「工作区有未提交修改或未跟踪文件」。不 stash/reset。
   - `git rev-parse --abbrev-ref --symbolic-full-name @{u}` 失败 → 失败「当前分支没有 upstream」（同时覆盖 detached HEAD/rebase 中）。
   - `git rev-parse HEAD` 记为 `from`，写入状态与终端日志。
2. **拉取**：`git pull --ff-only`，总共最多 5 次，各次超时依次为 20s、30s、40s、50s、60s，超时杀掉 git；失败或超时立即重试，不等待；不解析 stderr。全部失败 → 失败（最坏约 3 分 20 秒）。
3. `git rev-parse HEAD` 记为 `to`；`to == from` → 「已是最新」结束。
4. **构建**：`./build.sh` 一次（无参数 = release）。非零 → 失败，不重试。源码已是新版、旧进程仍在跑，属预期。
5. **重启**：`try_send(RestartRequest)` 给 main；失败（不应发生）→ 失败。main 收到后取消全局 stop，等 Assembly 正常停止（释放数据库、监听端口、Channel），然后 `exec <update_repo>/target/release/micnext` + 原 argv[1..]，cwd 与环境不变。

pull 之外不设超时：检查与构建卡住时页面一直显示当前阶段与输出，人工在终端 Ctrl-C（终端信号发给整个前台进程组，git/cargo 一并收到）。

Linux 实测（Ubuntu 24.04 / GNU ld 2.42 / cargo 1.99）：服务运行中原地 `./build.sh` 成功，`target/release/micnext` 换成新 inode，旧进程存活，无 ETXTBSY。若将来出现 ETXTBSY，按构建失败处理。

## 三、配置

`[gateway]` 新增可选 `update_repo = "/absolute/path"`，不可网页改。

- `RawConfig` → `Config.update_repo: Option<PathBuf>`；`install` 只 parse：非绝对路径 → Err。
- `Gateway::serve` 启动时：配置了但目录不存在或缺 `build.sh` → Err；配置了但装配根没给 restart sender → Err（实现不匹配）。没配置 → 更新不可用，不报错。
- `-p` 模式：Gateway 不运行，与本功能无关。

## 四、公开契约

### 4.1 Rust（mic-gateway re-export）

```rust
/// 构建成功后请装配根停止并 exec 的目标。
pub struct RestartRequest {
    pub executable: std::path::PathBuf, // <update_repo>/target/release/micnext
}

impl GatewayModule {
    pub fn new(logs: DeveloperLogs) -> Self; // 不变
    pub fn with_restart_requests(
        self,
        requests: tokio::sync::mpsc::Sender<RestartRequest>,
    ) -> Self;
}
```

main（私有）：创建容量 1 的通道；信号任务 `select!` SIGINT / SIGTERM / restart 三者，先到者生效，记录是否为重启后 `stop.cancel()`。`assembly.run` 返回后：

- 外部信号先到 → 正常退出，已收到的重启请求忽略。
- 重启先到 → 终端打印「正在重启：<路径>」（from → to 已由 Gateway 的 `update restarting` 日志打印），`CommandExt::exec`；exec 返回即失败 → 打印原因，非零退出，不循环拉起。

只有 fire-and-forget 的 `try_send`，无 ack，不涉及自等待。

### 4.2 HTTP（Bearer 鉴权，`Cache-Control: no-store`）

`POST /api/developer/update`：无正文。202 空体；409 已有任务在跑；503 未配置。

`GET /api/developer/update` → 200，`state` 标签：

```jsonc
{"state":"unavailable"}
{"state":"idle","running":"<sha>|null"}
{"state":"checking","output":"..."}
{"state":"pulling","from":"<sha>","attempt":2,"output":"..."}
{"state":"building","from":"<sha>","to":"<sha>","output":"..."}
{"state":"restarting","from":"<sha>","to":"<sha>"}
{"state":"up_to_date","commit":"<sha>"}
{"state":"failed","stage":"checking|pulling|building|restarting","from":"<sha>|null","error":"中文原因","output":"..."}
```

`running` 为当前进程构建时的提交（build.sh 注入 `MICNEXT_COMMIT`，直接 cargo build 为 null），启动日志 `gateway listening` 同样打印。`output` 为合并输出末尾，最多 64 KiB（按字符边界截断）。状态只在内存，新进程为 idle；`failed` / `up_to_date` 保留到下次 POST。

### 4.3 前端

`web/src/api/developer.ts` 新增 decoder 与两个调用；开发者诊断页新增「更新」页签。running 时每秒 GET；见到 building/restarting 即记下目标 `to`（浏览器存储，随机 token 重新登录后仍在），之后见到 idle 时核对 `running == to`（本页轮询中途见到重启结果或协议不符，说明界面是旧版，先重载再由新页面核对）：一致显示「已更新并重启，当前运行 <sha>」，否则标红。重启很快，轮询不保证撞上连接失败，不能以此判断。401 由全局 token 失效处理。

## 五、实体、状态与不变量

| 实体 | 唯一写者 | 状态 |
|---|---|---|
| 更新任务 | Gateway（`Mutex` 单任务） | Idle/Failed/UpToDate → Checking → Pulling(1..5) → Building → Restarting；任一步错误 → Failed；提交未变 → UpToDate |
| 部署源码/target | 更新任务（禁止同时手工构建） | 原提交 → ff 新提交 → 新产物；失败保留现状 |
| 常驻进程 | main | Running → Stopping → exec，或 外部信号/错误 → Exit |
| 浏览器 | 页面 | 轮询 → 失败/已是最新/等待恢复；离开只停轮询 |

不变量：构建成功前不停旧服务；同一时刻只一个进程持有数据库（exec 在 Assembly 停止之后）；外部信号先到则不 exec；工作区不干净不 pull；程序不处理代理、不直连兜底。

不承诺：失败期间源码与运行版本一致；新版本若迁移了数据库，回退旧版本会 SchemaAhead，需人工处理。

人工恢复：终端/页面有 `from` 提交号 → `git checkout <from> && ./build.sh run --config ...`。

## 六、改动清单与兼容

| 位置 | 改动 | 兼容 |
|---|---|---|
| mic-gateway `config.rs` | `update_repo` | 缺省不启用，旧配置不变 |
| mic-gateway `lib.rs` | `RestartRequest`、`with_restart_requests` | additive，唯一调用方 main |
| mic-gateway 新 `update.rs` | 任务、子进程、状态 | 私有 |
| mic-gateway `service.rs` | 启动校验 + 两条路由 | 新路由 |
| mic-gateway `limits.rs` | `PULL_TIMEOUTS = [20s, 30s, 40s, 50s, 60s]`（次数 = 长度）、`UPDATE_OUTPUT_BYTES = 64 KiB` | — |
| mic-gateway `Cargo.toml` | tokio 加 `process`、`io-util`、`time` feature | 无新 crate |
| bin/micnext `main.rs` | 通道、信号 select、exec | `-p` 与无配置路径不变 |
| bin/micnext `default-config.toml` | 注释示例 `# update_repo = ...` | — |
| web | decoder、页签 | 新增 |

core / store / tool / Channel 不改；不新增 crate、依赖或 run 终态。`build.sh` 只在构建前导出 `MICNEXT_COMMIT`，参数契约不变。

## 七、相对 r2 的删减（human 已确认）

| r2 | r3 | 理由 |
|---|---|---|
| ProxyEnv、Cargo/Git 代理对齐、大小写冲突检查 | 删除，纯继承启动环境，部署约定先 `proxy` | 实测 bun/git/cargo 均认环境变量；用户 bashrc 大小写同值 |
| previous 备份（`/proc/self/exe`） | 删除，改为记录 `from` 提交号 | 编译通过后起不来概率低；git checkout + 增量构建即可恢复，且全平台同一路径 |
| pull 120s / build 30min 超时 + 进程组 + `/bin/kill` | build 不设超时、人工 Ctrl-C；pull 逐次超时只杀 git 本身 | 终端信号天然覆盖前台进程组 |
| 任务 id | 删除 | 单任务，重启后状态清空，无消费者 |
| pull 3 次、等 2s/5s | 5 次，超时 20s 起每次加 10s，立即重试 | 服务器实测：首次常秒失败（`CONNECT tunnel failed, response 502`），第二次卡一会儿后成功；被提前杀掉无妨，下次超时更长 |

## 八、人工验收

1. 本机：配置 `update_repo`，工作区留一个未跟踪文件 → 点更新 → 「检查」失败且提示原因。
2. 服务器：`proxy && ./build.sh run --config ...`；远端无新提交 → 「已是最新」，不重启。
3. 服务器：推一个提交 → 点更新 → 依次看到拉取/构建/重启；终端打印 from → to，新进程起来，页面「服务已恢复」。
4. 不开代理启动 → pull 五次全部失败，页面显示 git 输出，旧服务继续可用。
5. 构建中在终端 Ctrl-C → 进程退出，不 exec。
6. 未配置 `update_repo` → 页签提示配置方法，POST 返回 503。

# 测试耗时与保留价值审查

日期：2026-10-23。服务器验收后开展；先审查，human 同意后落实文末三项调整。

## 结论

当前没有值得为了耗时直接删除的测试。95 项 Rust 测试按 crate 正常并行运行，测试二进制的墙钟时间合计约 **2.23 秒**；Web 4 项测试整次约 **0.29 秒**。最明显的可省等待是一项后台输出通知测试，约 1 秒，建议保留覆盖、改用虚拟时间。

日常验证应按修改范围选择测试，收尾完成仓库要求的检查；没有新修改或新疑点时，不重复整套验证。本次计时没有测量冷编译、Clippy 或 Web 类型检查，不能用测试执行时间解释此前全部等待。

## 测量方法与结果

基于 `b4cfb2c` 工作区，复用已编译的 debug 测试二进制；先各自按默认并发运行，再每个用例以 `--exact` 独立启动计时。独立耗时包含进程启动，毫秒级差异不作性能结论。Rust 测试只用临时文件、SQLite 和本地模拟上游，没有调用真实服务商。Provider 回环监听在沙箱外测量。

| crate | 项数 | 整组墙钟耗时 |
|---|---:|---:|
| mic-core | 45 | 1.376 秒 |
| mic-store | 24 | 0.613 秒 |
| mic-tool-shell | 7 | 0.131 秒 |
| mic-channel-wechat | 3 | 0.059 秒 |
| mic-provider-openai | 13 | 0.036 秒 |
| mic-tool-fs | 1 | 0.006 秒 |
| mic-gateway | 2 | 0.006 秒 |

整组 95 项均通过。独立运行 94 项通过；1 项暴露出下述初始化依赖。独立运行耗时合计约 3.17 秒，不能与默认并发墙钟耗时相加。

## 审查建议（已获批准）

| 建议 | 用例与证据 | 收益与覆盖影响 |
|---|---|---|
| **优先提速，保留覆盖** | `mic-core/src/jobs/output/tests.rs::growth_inside_throttle_window_is_notified_later`，约 1.01 秒；等待真实的 `JOB_NOTIFY_INTERVAL` | 改用 Tokio 虚拟时间，并断言窗口结束前无通知、结束后补发。生产代码的节流时钟目前是 `std::time::Instant`，需一起改为 `tokio::time::Instant`，否则只暂停测试时钟不能完整控制时间。测试涉及 blocking 文件写入，先完成写入，再推进时间。预计省去约 1 秒实等，无需新增可配置间隔或注入框架。 |
| **可合并，低优先级** | `mic-store/src/store/tests.rs` 的 `starts_at_round_head`、`cap_cuts_inside_round`、`whole_session_when_short`、`earliest_round_exact`、`empty` | 改为一项表驱动分页测试，保留全部输入与断言。减少零碎函数，但几乎不省执行时间，不应为了测试数量单独发动整理。 |
| **保留真实进程测试** | Shell 的 `dropping_background_kills_the_process_group` 约 129ms、`foreground_timeout_has_a_terminal_timeout_with_partial_output` 约 106ms | 分别验证子进程组清理，以及真实超时后的部分输出与终态；两者不能互替。虚拟时间不能证明 OS 子进程已退出，不建议为省这点时间改成 mock。 |
| **保留执行与存储回归** | 停止重试/压缩约 99ms、并行工具顺序约 70ms、后台超时与重复停止约 60ms；其余单项均更短 | 覆盖停止边界、终态唯一性、工具顺序、迁移保留与回滚等具体故障。没有发现耗时足以抵消其价值的用例，也不建议将不同生命周期测试硬拼成一个大场景。 |

其他短测试也未发现足够证据支持直接删除。例如 Provider 的 reasoning 配置测试与完整 HTTP 测试看起来相近，但前者覆盖显式参数、旧配置拒绝及不支持 reasoning 的分支，后者覆盖模型默认值和实际 wire 行为；不是相同覆盖。Web 工具结果测试覆盖已发生的跨轮串联回归，Markdown 测试已有一项表驱动测试，都不构成耗时负担。

## 另发现：一个测试不能独立运行

`crates/mic-provider-openai/src/error/tests.rs::network_error_omits_secret_url` 独立运行会在 `reqwest::Client::new()` 处 panic：未安装 rustls crypto provider。整组运行通过，是因为 factory 测试调用了 `install_crypto()`，建立了进程全局状态。

建议在该测试构建 Client 前自行安装现有 ring provider，让测试能独立筛选运行。不要依靠测试顺序，也不要因这项失败删除凭据脱敏覆盖。这是测试初始化问题，不是本次发现的生产网络故障。

## 实施与验证

- 后台通知改用 Tokio 时钟，测试直接触发通知逻辑并推进虚拟时间，验证窗口结束前不补发、结束后只补发一次。避免异步文件写入导致虚拟时钟自动推进；文件写入与封口覆盖仍由相邻测试保留。相关 4 项测试合计约 0.02 秒。
- 分页 5 项合并为 1 项表驱动测试，7 组输入和原有断言全部保留；Store 相关 2 项测试通过。
- 脱敏测试自行初始化 ring provider，单独筛选运行通过。
- `cargo fmt`、`cargo clippy --offline -- -D warnings`、`git diff --check` 通过。未改前端，未重复执行整仓库测试。

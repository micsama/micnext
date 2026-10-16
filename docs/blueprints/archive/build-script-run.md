# B2：构建后启动

**状态：CLOSED（2026-10-09，human 要求增加 debug；release/debug 工具桩验收、shell 语法、fmt/clippy 通过）**

## 契约

```sh
./build.sh                        # 只构建
./build.sh run                    # 构建成功后启动
./build.sh run --config xxx.toml  # run 后参数原样传给 micnext
./build.sh debug                  # 构建 debug 版本
./build.sh debug run              # 构建 debug 版本后启动
```

可选前缀 `debug` 选择 debug 构建，缺省 release；其后无参数只构建，`run` 后参数原样传给 micnext。
其他脚本参数输出用法并非零退出，不执行构建。`run` 后所有参数由 micnext 解释。
`run` 复用完整构建流程，任一步失败即退出；成功后用 `exec` 启动所选版本的二进制。
两种版本均构建前端。release 保留清理 gateway 以重新嵌入资源；debug 运行时从磁盘读资源，无需清理 gateway。
脚本仍从仓库目录构建并启动，相对配置路径按仓库目录解释，与现有脚本的工作目录约定一致。
micnext 自己解析运行参数；退出码和信号直接交给它，不增加运行模式、后台进程或重启机制。

副作用：保留依赖安装、前端产物和 Rust 构建；`run` 额外启动所选版本的二进制，其副作用沿用原契约。
调用方：无参数调用兼容；手动构建后启动可改用 `run`；二进制入口、crate API 与依赖不变。

## 验收

用临时目录中的工具桩核对无参数只构建、run 参数含空格时完整透传、构建失败不启动、未知参数不构建，
以及二进制退出码透传；覆盖 debug 构建不清理、不传 --release，debug run 启动正确版本。
不启动真实服务。shell 语法检查、fmt 与 clippy 均通过。

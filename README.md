# micnext（占位名，待定）

单进程个人 agent 工具，micbot 的下一代重写。全新独立仓库，不复用 micbot 的
git 历史；micbot 保留作代码参考。

- 设计蓝图：[`docs/brainstorm/next-gen-architecture.md`](docs/brainstorm/next-gen-architecture.md)
  （B1 头脑风暴，从 micbot 仓库同步过来，后续在本仓库继续演进）。
- 当前状态：v0a 进行中，Web 闭环已打通（网关 + 内嵌前端 + `-p` 调试调用）。进度与模块划分见
  [`docs/blueprints/v0a-module-map.md`](docs/blueprints/v0a-module-map.md)，待办见 [`todo.md`](todo.md)。

## 起步方式

沿用 micbot 的 Semantic Blueprint Protocol：Blueprint → Implementation 不可跳。
每个 crate 真正动工（定义公开类型/trait/签名）前，先写一份 `docs/blueprints/<topic>.md`
说清楚签名、错误、强类型契约、副作用，人工批准后再实现。强制约束见根目录
[`CLAUDE.md`](CLAUDE.md)（从 micbot 裁剪，去掉了 TUI 依赖、旧 crate 名和历史 BP
编号，保留通用部分）。

## 查看 release 体积

构建后运行 `./scripts/size.sh`，查看 `target/release/micnext` 的文件大小、segment/section
组成、临时副本 strip 后的大小，以及当前前端资源大小。默认只分析现有文件，不重新构建，
不修改原二进制；需要 macOS Command Line Tools 和 Python 3，仅支持单架构 64 位 Mach-O。

运行 `./scripts/size.sh --bloat` 额外查看前 20 个 crate 和大函数；先安装
`cargo install cargo-bloat --locked`。此选项会按默认 features 重新构建 release，
报告针对重新构建后的产物；crate 归因只覆盖机器码，不代表整个文件占比。

## 项目名

`micnext` 是占位名，还没定下来，之后随时可改（改名不影响 crate 内部结构）。

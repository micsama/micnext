# micnext（占位名，待定）

单进程个人 agent 工具，micbot 的下一代重写。全新独立仓库，不复用 micbot 的
git 历史；micbot 保留作代码参考。

- 设计蓝图：[`docs/brainstorm/next-gen-architecture.md`](docs/brainstorm/next-gen-architecture.md)
  （B1 头脑风暴，从 micbot 仓库同步过来，后续在本仓库继续演进）。
- 当前状态：**空壳**。crate 目录已按蓝图 §7.1 的依赖图搭好，每个 crate 只有一行
  占位注释,没有任何公开类型/trait/实现。

## 起步方式

沿用 micbot 的 Semantic Blueprint Protocol：Blueprint → Implementation 不可跳。
每个 crate 真正动工（定义公开类型/trait/签名）前，先写一份 `docs/blueprints/<topic>.md`
说清楚签名、错误、强类型契约、副作用，人工批准后再实现。强制约束见根目录
[`CLAUDE.md`](CLAUDE.md)（从 micbot 裁剪，去掉了 TUI 依赖、旧 crate 名和历史 BP
编号，保留通用部分）。

## 项目名

`micnext` 是占位名，还没定下来，之后随时可改（改名不影响 crate 内部结构）。

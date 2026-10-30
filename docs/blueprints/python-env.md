# B2：命令里的 Python 用固定的 uv venv

状态：实现完成，待验收（2026-10-30）。方向见 [B1](../brainstorm/python-env.md)，human 选 A：有 uv 就用，没有就回退到系统 python，不让启动失败。

## 目的

模型和 skill 脚本里的 `python` 有一个确定的环境：缺包装一次以后就一直在，不用每个会话重新试 `pip --user`、`--break-system-packages` 这些办法。

## 行为

- **准备**：启动时检查 PATH 上有没有 `uv`。
  - 有：`<data_dir>/python` 里还没有 venv（没有 `pyvenv.cfg`）时，执行 `uv venv` 建一个；已经有就直接用。创建有超时（`limits.rs`，2 分钟，机器上没有 python 时 uv 会下载一个）。
  - 没有 uv，或者创建失败、超时：回退到系统 python，记一条 warn 日志（开发者诊断里能看到原因），不阻止启动。
  - 不检查已有 venv 是否损坏；坏了就删掉目录重启，会重新建。
- **日常用法**：直接装、直接跑。缺包用 `uv pip install 包名`，运行用 `python script.py`。venv 不带 pip，也不建 uv 项目。
- **特殊需求**：依赖冲突等情况，模型自己另建 venv，用显式的解释器路径安装和运行；框架不管这些环境。
- **生效**：用 venv 时，所有 `bash` 命令（前台、后台、子 agent）都会：
  - 把 `<venv>/bin` 加到 PATH 最前面；
  - 设 `VIRTUAL_ENV`；
  - 设 `UV_DEFAULT_INDEX`，默认是腾讯云 PyPI 镜像 `https://mirrors.cloud.tencent.com/pypi/simple/`（放在 `limits.rs`）。服务进程本身已经设了这个变量时，沿用进程的值，换机器部署不用改代码。

  镜像有同步延迟：需要刚发布的版本时，在那条命令里显式指定官方源，不做自动切源。回退到系统 python 时，以上都不注入，和现在一样。
- **系统提示**：用 venv 时，Environment 部分写明事实：
  - `python` 来自共享的 uv venv（写上路径）；
  - 缺包用 `uv pip install` 安装，装了一直保留；
  - 依赖冲突时可以自己另建 venv，用显式路径运行。

  回退时不写。
- 所有会话共用这一个 venv，不按会话分；设置页不做管理入口。

## 契约

- **mic-core**：
  - 装配时准备 venv，结果存在 Engine 上（有 venv 时记路径，回退时为空），启动后不再变化。
  - 执行工具时，交给 `ToolContext` 的环境变量表改成「命令额外环境」：`PATH`、`VIRTUAL_ENV`、`UV_DEFAULT_INDEX`，再加上设置里的环境变量。脱敏仍然只用设置里的变量值，这些框架变量不参与。
  - `request::system_prompt` 增加 venv 路径参数，用来生成上面的说明。
  - `limits.rs` 新增创建超时和默认镜像地址。
- **mic-tool / mic-tool-shell**：不改接口，`bash` 照常注入 `ToolContext` 里的环境变量。
- **无数据转换**：venv 只是数据目录下的一个文件夹，不进数据库，也不进设置备份。

## 验收

- **自动**：不写测试。准备逻辑只是调一次外部命令，靠人工验收。
- **人工**：
  - 有 uv 的机器启动后出现 `<data_dir>/python`；让模型 `uv pip install` 一个包，确认从镜像下载，新会话里能直接 `import`；系统提示里有那几句说明。
  - 把 uv 移出 PATH 后启动，服务正常起来，开发者诊断里有 warn，命令走系统 python。

# B1：让 agent 用本机浏览器（Zigzag MCP）

状态：调研完成，方向待定。不急，先记下。

来源：human 2026-11-12 提出。在 Mac 上用浏览器打开 micnext 的 Web 页面时，希望服务器上的 agent 能调用 Mac 本机的 MCP 服务 `http://127.0.0.1:39217/mcp`，也就是 Zigzag 浏览器自带的 MCP。

## 实测结论（2026-11-12）

- **服务**：Zigzag 0.8.5，Streamable HTTP，协议版本 `2025-06-18`。
  - 响应是普通 JSON，不是 SSE。
  - `initialize` 返回 `Mcp-Session-Id`，后续请求要带上。
- **鉴权**：`Authorization: Bearer <key>`，key 在 Zigzag「设置 › 自动化」里。key 不写进仓库。
- **拒绝网页来源**：带 `Origin` 的请求（包括浏览器的 CORS 预检）一律返回 403「requests must come from this Mac, not a web page」。
  - 所以不能用 Web 页面去中转。这与 Safari 和是否使用域名都无关。
- **工具（14 个）**：
  - 只读：`list_tabs`、`read_page`、`get_links`、`screenshot`。
  - 操作：`open_tab`、`navigate`、`click`、`type`、`scroll`、`activate_tab`、`close_tab`、`go_back`、`go_forward`、`reload`。
  - 自带说明里的约束：
    - 不给 id 时操作用户眼前的标签。
    - 隐私窗口、Zigzag 自身页面和密码框都不能碰。
    - 被操作的标签上有「Stop」提示条，用户点了 Stop，agent 应停手并询问用户。
- **后台可用**：在后台标签里走完了以下流程，全程没切走用户眼前的页面：
  1. `open_tab`（`background: true`）
  2. `read_page`
  3. `click`（按文字查找）
  4. `get_links`
  5. `close_tab`
  - 每一步都等页面加载完才返回。
  - 返回结构化 JSON，`read_page` 还附带表单字段及其选择器。
  - 错误会直接说明修法。
  - 例外：`screenshot` 要求标签在屏幕上（先 `activate_tab`），会抢用户的前台。

## 连接方式：SSH 反向隧道

服务器无法直接访问 Mac 的回环地址。由 Mac 发起反向隧道：

- **`~/.ssh/config` 里加一个 Host**：
  - `RemoteForward 127.0.0.1:39217 127.0.0.1:39217`
  - `ServerAliveInterval 30`、`ServerAliveCountMax 3`
  - `ExitOnForwardFailure yes`
- **常驻**：用 launchd LaunchAgent 运行 `ssh -N -o BatchMode=yes <Host>`。
  - 设 `RunAtLoad` 和 `KeepAlive`，`ThrottleInterval 15`。
  - 断线后自动重连，不需要 autossh。
- **服务器端 sshd**：加 `ClientAliveInterval 30` 和 `ClientAliveCountMax 3`，让断线后残留的转发端口约 90 秒内释放，否则重连会因端口被占用而反复失败。
- **转发只绑定服务器的 127.0.0.1**：没有鉴权的端口不会暴露到公网。
- **隧道发来的请求不带 `Origin`**：Zigzag 会把它当作本机进程，正常放行。

## 能做什么、和 computer use 的差距

- **适合**：
  - 读用户已登录才能看的页面，比如 GitHub 通知、后台、付费站点。
  - 总结或整理用户当前打开的标签。
  - 搜索、翻页、填表等简单操作。
  - 和定时任务、推送配合。前提是 Mac 醒着、Zigzag 开着、隧道通着；不满足时调用失败，如实告诉用户。
- **相比按像素操作的 computer use**：
  - 优势：
    - 快、省 token。
    - 读到的是原文。
    - 现成带着用户的登录状态。
  - 不如：
    - 拖拽、悬停、canvas、富文本编辑器、自定义控件。
    - 截图能看但不能按坐标点。
    - 没有按键和文件上传。
    - 只能操作浏览器。
  - 差距主要在 Zigzag 提供的工具集，不在 micnext。
- **后台限制**：后台标签可能被浏览器降速，懒加载、无限滚动的页面可能读不全。

## micnext 要补的（未来 B2）

micnext 目前没有 MCP 客户端，早期已砍掉（见 [next-gen-architecture](next-gen-architecture.md)）。接入需要：

- 一个 Streamable HTTP 的 MCP 客户端：
  - 地址和请求头（Bearer）在设置里配置。
  - 维护 `Mcp-Session-Id`。
  - 服务不可达时，工具调用返回错误，不影响其他功能。
- 把远端工具挂给 agent。工具结果里的图片（`screenshot`）要能交给模型。
- 在工具描述或提示里告诉模型：
  - 优先自己开后台标签，并始终带 id 操作。
  - 优先读文字，需要看画面时才截图。
  - 用户点了 Stop 后停手询问。

待定：
- 是做通用 MCP 接入，还是只为这一个服务。
- 工具挂给所有会话，还是按会话或人设开启。

# 待验收清单

汇总 `todo.md` 里待人工验收的 B2，按场景排，一次过完。括号里是对应蓝图；一个蓝图的条目全部通过后标 `CLOSED` 并归档，然后从这里删掉。

## 1. Web 电脑端

- [ ] 任务做到一半 `/compact`，出现「压缩后重新加载 skill」且模型接着做；已做完的任务再压缩不重新加载（skills-after-compaction）
- [ ] 设置「环境变量」里配好钉钉 token，让模型用 skill 列表格：命令里直接用 `$NAME`，不再去 `env` 里找；Trace 实际输入里只见 `<NAME>`（env-vars）
- [ ] 启动后数据目录下有 `python/`；让模型 `uv pip install` 一个包（走腾讯云镜像），新会话里直接 `import`；系统提示有 Python 一行。拿掉 uv 再启动，服务照常，开发者诊断有 warn（python-env）
- [ ] 让模型 `send_file` 一个 HTML 泳道图：预览里脚本能跑、拿不到登录态；图片和 PDF 能预览；能下载（send-file）
- [ ] 分别发大照片、截图、GIF，页面可见，模型能描述（image-normalize）
- [ ] `ask_user` 卡片作答正常（ask-user-question）
- [ ] A、B、新会话各写文字、附件、引用，来回切换各自恢复；发送中切走再回来不能重发；待发送图片点开放大；刷新后草稿消失（composer-drafts-preview）
- [ ] 生成中插入新消息、切页再回来、重试、停止后重连：流式草稿位置在新消息上方，不重复、不残留（stream-draft-snapshot）
- [ ] 新会话页关掉脱敏开关，发一条带示例凭据的消息，Trace 里模型看到的是原文（new-chat-settings）
- [ ] 问「某某功能在哪」，Trace 里先加载 micnext-guide 再回答；让 agent 排查一个小问题，看它先读 code-map 再定位源码（micnext-guide）
- [ ] 侧栏：两个标签页各开一个会话，后台跑完亮未读点、切回后消失；一边置顶或归档，另一边随之更新；后台运行时转圈；置顶微信会话、归档已置顶会话；整体观感（session-sidebar）
- [ ] 连续对话几轮，会话页不再自己重新加载、历史消息不闪「排队中」；快速连续切换几个会话、切到后台标签再切回，显示和未读都正常（session-load-diagnostics 回归修复）
- [ ] 点会话标题改名，Enter 保存、Esc 放弃，侧栏和另一个标签页同步；改名后继续对话不被自动标题覆盖；点 ↻ 生成途中改名，最后保留手动标题；中文输入法选词时按 Enter 不会提前保存（session-rename）
- [ ] 贴一张图并说「看这个」，回复后 ✦ 预测的下一句和图片内容相关；Trace 里预测调用的实际输入能看到这张图（预测带图，非 B2）
- [ ] 线上更新后发消息、改标题、置顶、退出再登录都正常，不出现「请求不是从本站页面发起的」（写请求同源校验、cookie Secure，非 B2）

## 2. 手机

- [ ] 输入栏两行（附件、输入、发送一行，设置在下）、标题栏一行 Trace 在右；回复下点耗时胶囊展开统计，点自己的消息出复制和时间；会话行 ⋯ 菜单置顶；设置先列目录再进分类（手机端优化，非 B2）
- [ ] `ask_user` 卡片布局（ask-user-question）
- [ ] 抽屉收起时菜单按钮上的未读红点；电脑上读过后手机上的点也消失（session-sidebar）

## 3. 微信

- [ ] 正常收发；引用 bot 的分段回复；断网时发送失败，在 Web 显示为失败（wechat-wrap-up）
- [ ] 发文件后 Web 会话里显示文件胶囊；引用图片或文件后正常回复（web-file-upload）
- [ ] 收到 bot 发的图片和文件；引用 bot 发出的文件后正常回复（send-file）
- [ ] 发原图，模型能描述（image-normalize）
- [ ] 问「某某功能在哪」，先加载 micnext-guide 再回答（micnext-guide）
- [ ] `ask_user` 以文字呈现，能作答（ask-user-question）

## 4. 定时任务

- [ ] 把会话 25 的任务改成 10 分钟以上并重新启用；到点前压缩一次，之后每次调用的输入明显下降（schedule-limits）
- [ ] 一个 10 分钟任务连续跑一两个小时无缺漏；停服务一段时间再启动只记一条「错过 N 次」；停用后再启用、改规则后不误记错过（scheduled-tasks P2.10。原文写每分钟任务，现在限频规则已不允许，改用 10 分钟）
- [ ] 任务页观感、编辑与启停、任务会话里切换模型、通知中心「去会话里问」、运行记录定位到 Trace；微信延迟送达后页面状态更新（scheduled-tasks P3）
- [ ] 任务 push 带文件，通知中心和微信都能收到（send-file）
- [ ] 对话里让 agent 立即执行一个任务（含已停用的）：任务会话开跑，下次运行时刻不变，停用任务仍停用（scheduled-tasks P2.11）

## 5. 登录

- [ ] 服务器上删掉 `[gateway] token`，运行 `micnext --set-password` 设好密码；电脑浏览器和 iPhone 主屏幕应用各登录一次（web-login）
- [ ] 网页「设置 → 账号」改一次密码，另一台设备刷新后回到登录页；侧栏底部「退出登录」可用（web-login）

## 6. 系统通知

- [ ] iPhone 主屏幕应用里打开「设置 → 对话偏好 → 本设备系统通知」并允许；让 agent `push` 一条，锁屏收到、点开进入来源会话；关掉开关后不再收到（web-push）
- [ ] 让 agent `push` 一条：iPhone 锁屏标题完整显示、正文是推送文字；通知中心显示来源、加粗标题和正文；微信收到「标题、空行、正文」；定时任务结果推送的标题为「⏰ 任务名」，长任务名截断补「…」（push-title）

## 7. 隔天或碰上再看

- [ ] 链路不稳时，对话和 `/compact` 显示重试次数与倒计时，最终失败仍显示原因（model-retry-indicator）
- [ ] 运行日志按 `session load slow` 收集慢记录：Safari 完全退出重开后 `protocol` 应为 `h3`；开 Caddy 访问日志后对照慢请求的 `duration`，归因后删除临时诊断（session-load-diagnostics）

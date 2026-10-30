# 待验收清单

汇总 `todo.md` 里待人工验收的 B2，按场景排，一次过完。括号里是对应蓝图；一个蓝图的条目全部通过后标 `CLOSED` 并归档，然后从这里删掉。

## 1. 服务器更新时

- [ ] 用网页一键更新，转换脚本正常跑完、服务起来（database-conversion）
- [ ] 设置页杂活模型仍是原来选的模型；新会话能生成标题，SQL 写入能走把关（utility-model）

## 2. Web 电脑端

- [ ] 对话结束几秒内出现 ✦ 灰字，Tab 填入后发送；刷新后灰字还在，发新消息后消失；Trace 有预测调用，看板计入花费（input-prediction）
- [ ] `/name` 加载 skill；上下文构成里有「Skill 目录」；`/compact` 后 skill 正文变成占位（skills）
- [ ] 任务做到一半 `/compact`，出现「压缩后重新加载 skill」且模型接着做；已做完的任务再压缩不重新加载（skills-after-compaction）
- [ ] 让模型搜一条最新消息，结果可读，没有滥搜（web-search）
- [ ] 让模型 `send_file` 一个 HTML 泳道图：预览里脚本能跑、拿不到登录态；图片和 PDF 能预览；能下载（send-file）
- [ ] 分别发大照片、截图、GIF，页面可见，模型能描述（image-normalize）
- [ ] `ask_user` 卡片作答正常（ask-user-question）
- [ ] 侧栏：两个标签页各开一个会话，后台跑完亮未读点、切回后消失；一边置顶或归档，另一边随之更新；后台运行时转圈；置顶微信会话、归档已置顶会话；整体观感（session-sidebar）

## 3. 手机

- [ ] 预测灰字末尾点「填入」（input-prediction）
- [ ] `ask_user` 卡片布局（ask-user-question）
- [ ] 抽屉收起时菜单按钮上的未读红点；电脑上读过后手机上的点也消失（session-sidebar）

## 4. 微信

- [ ] 正常收发；引用 bot 的分段回复；断网时发送失败，在 Web 显示为失败（wechat-wrap-up）
- [ ] 发文件后 Web 会话里显示文件胶囊；引用图片或文件后正常回复（web-file-upload）
- [ ] 收到 bot 发的图片和文件；引用 bot 发出的文件后正常回复（send-file）
- [ ] 发原图，模型能描述（image-normalize）
- [ ] `/name` 加载 skill（skills）
- [ ] `ask_user` 以文字呈现，能作答（ask-user-question）

## 5. 定时任务

- [ ] 把会话 25 的任务改成 10 分钟以上并重新启用；到点前压缩一次，之后每次调用的输入明显下降（schedule-limits）
- [ ] 一个 10 分钟任务连续跑一两个小时无缺漏；停服务一段时间再启动只记一条「错过 N 次」；停用后再启用、改规则后不误记错过（scheduled-tasks P2.10。原文写每分钟任务，现在限频规则已不允许，改用 10 分钟）
- [ ] 任务页观感、编辑与启停、任务会话里切换模型、通知中心「去会话里问」、运行记录定位到 Trace；微信延迟送达后页面状态更新（scheduled-tasks P3）
- [ ] 任务 push 带文件，通知中心和微信都能收到（send-file）

## 6. 隔天或碰上再看

- [ ] 隔天回到 Web 或微信会话，看整理通知文字和 Trace 里的请求；用 SQL 看整理后那次调用和下一次调用的 `cache_read_tokens`（cold-trim）
- [ ] 链路不稳时，对话和 `/compact` 显示重试次数与倒计时，最终失败仍显示原因（model-retry-indicator）
- [ ] 真实网络错误在开发者日志里能看到阶段和根因（external-error-observability）

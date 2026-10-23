# B2：ask_user 提问工具

**状态：已批准（human 2026-10-23），已实现，本地检查通过，待 human 验收。** 依据：[B1](../brainstorm/ask-user-question.md)。

## 行为

- 新增 core 工具 `ask_user`，和 `push` 一样给 Root 会话用。问题就是调用入参。
- 入参合法时，工具返回 `Question posted. Your turn ends here; the user's next message is the answer.`，run 随即结束。入参不合法时返回错误，模型自己改。
- 同一批里的其他调用不执行，直接打回：`Not run: ask_user must be called alone.`
- 用户的下一条消息就是回答，模型对照自己的提问理解。

## 入参

照抄 Claude Code AskUserQuestion：`questions` 1–4 题，每题有 `question`、`header`、`options`（2–4 个，每个有 `label` 和 `description`）、`multi_select`。description 从原文改写，删去 Plan mode 相关内容，并加一句「单独调用，问完本轮结束」。

## 展示

- **Web**：
  - 成功的 `ask_user` 调用显示成一张卡片，一题一页：顶部是各题标签，单选点一下就进下一题，多选和「其它」点「下一步」；最后一页汇总答案，点「提交」。
  - 提交时发一条普通消息，每题一行：`「问句」→ 选项`。
  - 卡片后面有了用户消息，就收成一张「已回答」的简要卡片。
- **微信**：投递回复时，把入参排成编号文字；用户回「1 3」或直接打字都行。

## 验收

- **本地**：`ask_user` 成功后 run 结束；同批的其他调用被打回。
- **human**：Web 卡片、手机布局、微信文字。

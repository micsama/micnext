# B2：模型「支持图片」开关

**状态：已实现（2026-10-15）；fmt/clippy、workspace 测试、Web 检查与真实库副本迁移验收通过，设置页与真实模型交 human。**

修订 [model-settings](archive/model-settings.md) 的模型字段与 §十「不支持图片的模型收到图」一行。

## 问题

图片进了对话记录就每轮重发。纯文本模型（如 DeepSeek）一律拒绝，会话从此每轮失败，只能 `/clear` 或换会话。现在模型基本原生多模态，只为少数例外留一个轻量开关。

## 契约

- `core_models` 新增 `images INTEGER NOT NULL DEFAULT 1`（core 迁移 v11），已有模型视为支持。
- `ModelView`、`ModelWrite`、`ClaimedModel::Selected` 增加 `images: bool`。
- HTTP：模型的读写体增加 `"images": bool`，与 `context_window` 同级、必填。
- 设置页模型行加「支持图片」勾选，默认勾上。

## 行为

- 关闭时，组装请求（对话与压缩）把窗口内用户消息的图片换成文字占位「[图片：当前模型不支持查看]」，不加载图片数据。
- 对话记录不变；换回支持图片的模型后照常带图。
- 不识别上游报错、不自动重试；开关开着而模型其实不支持时，仍是原来的「模型拒绝了请求」。

## 验收

core 测试：关闭时请求里图片变为占位、不带图片数据；`cargo fmt`、`clippy`、`bun run check`；设置页交 human。

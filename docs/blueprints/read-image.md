# 让 read 读取本地图片

状态：实现完成，待验收。

给出本机图片路径，视觉模型调用现有 `read` 即可在下一轮看到图片。图片就是工具结果里的 `ContentPart::Image`，历史、Trace、上下文计数、能力降级都沿用用户图片已有的路径。

## 契约

- `read`：先按文本读；遇到二进制时交给 `mic_media::normalize`，成功即返回图片，`Unsupported` 仍报"binary file"，其余按损坏或超限报错。图片读取前先按 `MAX_IMAGE_BYTES` 校验文件大小；给图片传 offset/limit 是参数错误。mic-tool-fs 依赖 mic-media。
- `mic-tool`：新增 `ToolOutput::Image { text, image: ImageData }`，工具只交出字节，不碰库。
- `mic-store`：新增 `append_with_image`，图片与工具结果同事务落库，图片归属会话。
- `mic-core`：执行层把 `Image` 变成 `Completed { output: [Text, Image(ref)] }`；`image_ids` 与 `hide_images` 同时覆盖工具结果，纯文本模型沿用现有占位。
- Provider（原生优先）：Responses 的 `function_call_output.output` 直接用 input_text / input_image 数组；Chat Completions 的 tool 消息只能是文本，回执写明"图片见下"，同批回执之后用一条 user 消息带图片并标明 call id。该投影只存在于出站请求。

## 验收

自动：两种协议请求里含图片；Chat 同批双图与回执对应；纯文本模型出占位；普通文本分页、坏图、超限不受影响。真实识别由 human 验收。

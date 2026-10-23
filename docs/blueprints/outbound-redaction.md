# B2：模型出站脱敏

**状态：已实现，待 human 验收（2026-10-23）。** 来源：[B1 隐私与权限](../brainstorm/privacy-permissions.md) 2026-10-23 节；human 定为常开、只做模型出口一层。

## 契约

- 位置：`Factories::build` 造出的每个模型实例都包一层 `redact::Redacted`，主循环、压缩、标题以及以后新增的调用都经过它。只改每次请求的副本，本地消息、trace、日志保留原文。
- 处理字段：system、各类消息正文、工具结果与失败信息、历史工具参数（JSON 字符串值）、压缩摘要；协议 ID、工具名、推理签名与上游密文推理不动。工具定义是本进程产出，不处理。
- 精确匹配：主密钥（`~/.config/micnext/master.key`，32 字节原始值）的小写/大写 hex 与标准 base64；当前模型实例自己的 API key。
- 正则：服务商 token 前缀（`sk-`、`ghp_`、`github_pat_`、`xox?-`、`AKIA`、`AIza` 等）、JWT、`Bearer …`、凭据字段赋值（`api_key=`、`"password": …`、`TOKEN: …`，值须含数字，避开 `config.api_key` 这类代码）、连接串密码、PEM 私钥。
- 遮蔽：≥20 字符保留首尾各 4 位，中间换成 `***`；更短的整段 `***`；私钥正文整段 `***`。
- 依赖：`mic-core` 新增 `regex`、`base64`（工作区已有）。启动时主密钥读不到直接报错（`RunError::KeyFile`）。

## 已知限制

- 模型看不到完整 key，无法把它填进后续工具调用；需要真实凭据的操作由工具在本地读环境或文件。
- 不覆盖：编码/拆分后的秘密、图片、`cat` 主密钥得到的乱码字节、工具直接联网外传、日志与 Web 展示、Channel 投递。防止工具读到 `~/.config/micnext/` 要靠 B1 的部署隔离。

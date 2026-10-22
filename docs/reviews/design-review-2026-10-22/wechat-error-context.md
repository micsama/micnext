# 微信 R4：网络错误与业务上下文合并

2026-10-22：私有实现整理，不改公开接口、重试次数或用户失败文案。

传输边界只分类并保留去除 URL 后的完整原因链及 endpoint、timeout、connect、io_kind、os_error、tls_handshake_eof 诊断，由业务消费者记录一次。入站轮询附会话与连续失败次数；CDN 每次尝试附会话、上游消息 id、item 位置与 attempt；投递附会话、消息与 chunk；登录附 setup attempt；typing 附会话与接口。长轮询超时继续只记 DEBUG。HTTP 非成功状态不再在 decode 和业务层各记一次。

下载失败与超限已经有具体尝试日志，flatten 不再追加只有枚举文案的 WARN；解密、图片规整等其他失败继续带消息上下文记录。协议解码错误仍由边界记录安全的字段路径、错误类别和长度，不记录原值。用户与模型仍看到此前的失败占位。

没有采用仅加 tracing span 的方案：当前 DeveloperLogs 不采集 span，无法让 Web 日志自然取得这些字段。

验证：Client 功能测试核对原因链保留、URL/凭据移除及用户文案不变；微信现有 schema 测试、cargo fmt、cargo clippy。真实网络日志效果留给 human，与外部交互日志蓝图一起验收。

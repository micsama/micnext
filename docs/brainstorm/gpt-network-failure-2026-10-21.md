# 服务器调用 gpt-6.1-sol 失败排查（2026-10-21）

## 结论

不是代码问题。服务器经 ShellCrash（mihomo）的 hy2 节点访问 chatgpt.com，这条链路从当天下午起双向只有约 7–57 KB/s，抖动也大；约 1 MB 的模型请求在上传途中就被掐断。同期 DeepSeek（国内服务）的调用一直正常。

## 现象

会话 8 的 run 125 用模型 gpt-6.1-sol，三次尝试都在收到响应头之前失败（`error sending request`），没有进入流式接收。

| 尝试 | 开始 | 失败 | 耗时 | 报错 |
|---|---|---|---|---|
| 1 | 16:26:47 | 16:28:02 | 75 s | peer closed connection without sending TLS close_notify |
| 2 | 16:28:04 | 16:28:34 | 30 s | 同上 |
| 3 | 16:28:38 | 16:28:59 | 21 s | Connection reset by peer (os error 104) |

两次重试之间分别等了 2 s 和 4 s，第三次失败后以 `ProviderFailed` 结束，重试和收尾都按设计工作。当天上午，同样的配置调用正常。

## 证据

**数据库**

- 会话 8 在 14:26–14:43 用 deepseek-flash，上下文约 4 万 token，每次 2–7 s 返回，全部成功。
- 会话 8 用 gpt-6.1-sol 的 14:00 和 16:26 两轮调用全部失败。
- 会话 1 用 gpt-6.1-sol 在 14:38 和 14:39 各失败一次，第三次成功；这次只有 3979 token。
- 会话 8 里有 3 张图片，共 0.51 MB，base64 编码后约 0.7 MB，每次请求都要重新上传。

**curl 测试（在服务器上）**

| 测试 | 结果 |
|---|---|
| GET /codex/models，小请求 | 401，1.2–4.9 s；链路能通，但抖动大 |
| POST /codex/responses，小请求 | 401，1.5–11.5 s |
| POST 1 MB，HTTP/2 | 三次都只发出 128–256 KB，16–31 s 后报 PROTOCOL_ERROR |
| POST 1 MB，HTTP/1.1 | 1 MB 全部发出，23–31 s 后 Connection reset / Empty reply |
| 上传 1 MB 到 Cloudflare | 21–57 KB/s，两次都失败 |
| 从 Cloudflare 下载 1 MB | 60 s 只收到 868 KB（14 KB/s） |
| 从 Cloudflare 下载 200 KB | 28.8 s（6.9 KB/s） |

**节点**

- mihomo v1.19.17 里只有一个真实节点 `hy2`（Hysteria2，走 QUIC/UDP），出口 IP 是 178.210.243.236。
- 用 `DIRECT` 访问 chatgpt.com 超时，不可用。
- 全机流量都被 TUN 透明转发，直连和走 127.0.0.1:7890 的结果一样。
- 同一个 hy2 节点在其他机器上使用正常。

## 分析

- **问题在链路，不在请求**：用 HTTP/1.1 把 1 MB 全部发出后仍然被 reset，下载也只有几 KB/s。小请求能过、大请求总是失败，是因为链路慢的时候，请求越大越容易撞上超时和断连。
- **不是我们的客户端主动断开**：客户端的空闲超时是 300 s，而失败都发生在 21–75 s 之间，是对端或中间环节掐断的。
- **hy2 在其他机器上正常**：说明节点本身没问题，问题出在这台服务器到 VPS 这一段。有两种可能：
  - 服务器所在网络对 UDP 限速（上午正常、下午变差，符合高峰时段限速的特征）；
  - 服务器上 hy2 客户端的 `up`/`down` 和其他机器不一样，填得比实际带宽高，导致大量丢包。
- **尚未排除**：服务器自身出口带宽下降。还没测过服务器下载国内文件的速度。

## 处置

**临时缓解**

- [ ] 会话 8 先切回 DeepSeek 继续用。
- [ ] 在设置 → 服务商里关掉 gpt-6.1-sol 的图片开关。历史图片会换成文字占位，每次请求少传约 0.7 MB；代价是 gpt 也看不到新发的图片，线路恢复后再打开。
- [ ] 大会话先压缩，或者开新会话。去掉图片后，4 万 token 的请求仍有一两百 KB，按当前速度仍可能失败。

**根治（按顺序做）**

1. 对比服务器和其他机器上 hy2 的 `up`/`down` 配置。可以先删掉这两项，让 hy2 退回 BBR；或者填成实际带宽的七成。改完重测 1 MB 上传。
2. 在服务器上直连下载一个国内的大文件（比如镜像站的 ISO），排除服务器自身带宽的问题。
3. 在同一台 VPS 上加一个 TCP 节点（VLESS + Reality 或 Trojan），和 hy2 放进同一个 `fallback` 或 `url-test` 组：
   - TCP 也慢，说明是 VPS 出口的问题；
   - TCP 正常，就确认是 UDP 被限速，以后也能自动切到 TCP。

复测命令（正常的话，几秒内返回 `401 up=1000000`）：

```
head -c 1000000 /dev/zero | tr '\0' 'a' > /tmp/body.txt
curl --http1.1 -sS -o /dev/null -w '%{http_code} up=%{size_upload} %{time_total}s\n' \
  -X POST -H 'content-type: application/json' --data-binary @/tmp/body.txt \
  https://chatgpt.com/backend-api/codex/responses
```

## 代码侧后续（未决定）

- **在失败日志里补上请求体字节数、断开阶段（上传中、等响应还是流式中）和耗时**：这次靠 SQL 和 curl 才分辨出来；有了这几项，看一行日志就能定位。改动不大，等 human 确认。
- **历史图片每次调用都重新上传**：可以考虑只在最近几轮里带原图，更早的换成文字占位。这属于上下文策略的改动，要先写 B1，不因为这次就顺手改。

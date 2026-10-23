# 服务器 GPT 卡顿：代理链路吞吐排查（2026-10-23）

接续 [2026-10-21 网络排查](gpt-network-failure-2026-10-21.md)。状态：**待服务器与 Mac 同时段强制走 hy2 对照**。

## 结论（当前）

- 卡顿不是 GPT 特有：服务器经 hy2 隧道访问与 GPT 无关的测速站同样只有 7–30 KB/s，直连国际约 533 KB/s。GPT 请求体大（几百 KB 到 1 MB）、流式时间长，最先暴露问题。
- 瓶颈在「腾讯云服务器 → hy2 节点」这一段。同一节点在其他机器上正常，嫌疑排序：腾讯云对 UDP（443/QUIC）出流量限速 > 中转 / 落地机带宽上限或服务端 `bandwidth` 设置过小。
- 不是 UA 问题：UA 在 TLS 内，隧道层不可见，无法据此限速；UA 不被接受时表现为 403、Cloudflare 质询或 429，而不是低吞吐与上传中途 reset。

## 服务器实测（腾讯云）

| 路径 | 带宽 |
|---|---|
| 直连国内清华镜像 | 约 6023 KB/s（48 Mbps） |
| 直连国际 OVH（`SO_MARK=7894` 绕过 nft 重定向） | 约 533 KB/s（4.3 Mbps） |
| 经 mihomo hy2 隧道 | 7–16 KB/s，4 路并发合计约 14 KB/s |
| 经 hysteria 官方客户端 | 10–31 KB/s，6 路并发合计约 10 KB/s |
| 加 `bandwidth`（brutal 拥塞控制） | 13–23 KB/s，没有提速 |

- 延迟稳定在 36 ms，丢包为 0，MTU 1472 能通过；并发不叠加，总量有上限。这些特征更像中间链路被硬限速，不像拥塞控制问题。
- nft `output` 链会把本机所有 TCP 重定向到 7892，绕过隧道测直连时必须加 `SO_MARK=7894`。之前测得「直连海外 ttfb 3.3 s」实际走的是隧道，结果作废。
- 同一隧道下，mihomo 比官方客户端慢 2–3 倍，可以通过客户端参数改善（见下文）。

## Mac 对照（未强制走 hy2，结论无效）

| 目标 | 结果 |
|---|---|
| OVH 10 MB | 47 KB/s，20 s 超时；OVH 从国内访问本来就慢，不适合做基准 |
| cachefly 10 MB（http） | 1.75 MB/s；按 Clash 规则可能走了直连 |
| speed.cloudflare.com | TLS 握手阶段被断开（`unexpected eof`），在此网络下不可用，不再作为测速点 |

## 代码侧观察

`crates/mic-provider-openai/src/config.rs` 的 Codex 请求只带鉴权头和 `chatgpt-account-id`，没有 `originator` 和自定义 `User-Agent`。这与吞吐问题无关；如果线路修复后 TTFT 仍偏高，再按 [Codex TTFT 排查](../reviews/codex-ttft/README.md) 做有无这两个头的对照。

## 下一步

1. **同一时段对照**：Mac 把代理组切到 hy2，用 `-x` 强制走代理；服务器无需 `-x`。两边各测一次：

   ```bash
   for u in 'http://cachefly.cachefly.net/10mb.test' 'https://proof.ovh.net/files/10Mb.dat'; do
     curl -sS -x http://127.0.0.1:7890 -o /dev/null --max-time 20 \
       -w "%{http_code} ttfb=%{time_starttransfer}s speed=%{speed_download}B/s  $u\n" "$u"
   done
   ```

   Mac 达到几百 KB/s 以上而服务器只有几十 KB/s，就确认是服务器到节点这一段的问题；两边相近，则继续查节点本身。
2. **分段定位**：在香港中转机和澳洲落地机上分别测下载带宽。
3. **TCP 对照**：在同一节点加 TCP 入站（vless / trojan，复用 443）。TCP 明显更快，就能坐实腾讯云限速 UDP；之后长期主用 TCP，hy2 作备用。
4. **客户端整理**（不解决限速）：给 hy2 设置 `up` / `down`，开启 `tcp-concurrent`；修正 `ipv6: true`（本机没有 IPv6）、Mac 残留的 `interface-name: en0` 和 `authentication: [""]`；`PROXY` 组改用 `url-test` 或 `fallback`。

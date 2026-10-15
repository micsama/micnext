import { getStreamTicket } from "./client";

/**
 * 用原生 EventSource 打开实时流（docs/blueprints/web-sse-eventsource.md）：先用 Bearer 换一次性票据再建流。
 * 不用 fetch 读流：Safari 会压住页面忙时到达的尾段（WebKit 322545）。
 * 浏览器自带重连会拿已作废的票据，出错即关，由调用方重新取票。
 * 取票失败抛 `ApiError`；回调抛错（如 `ProtocolError`）原样抛出；连接中断或被取消时正常返回。
 */
export async function readSse(
  path: string,
  query: URLSearchParams,
  events: readonly string[],
  signal: AbortSignal,
  onFrame: (event: string, data: string) => void,
): Promise<void> {
  const ticket = await getStreamTicket(path);
  if (signal.aborted) return;
  query.set("ticket", ticket);
  const source = new EventSource(`/api${path}?${query}`);
  try {
    await new Promise<void>((resolve, reject) => {
      signal.addEventListener("abort", () => resolve(), { once: true });
      source.onerror = () => resolve();
      for (const event of events)
        source.addEventListener(event, (e) => {
          try {
            onFrame(event, (e as MessageEvent<string>).data);
          } catch (err) {
            reject(err);
          }
        });
    });
  } finally {
    source.close();
  }
}

/**
 * 用原生 EventSource 打开实时流（docs/blueprints/web-sse-eventsource.md），同源 cookie 自动带上。
 * 不用 fetch 读流：Safari 会压住页面忙时到达的尾段（WebKit 322545）。
 * 出错即关，不用浏览器自带重连：由调用方带上续传位置重新打开。
 * 回调抛错（如 `ProtocolError`）原样抛出；连接中断或被取消时正常返回。
 */
export async function readSse(
  path: string,
  query: URLSearchParams,
  events: readonly string[],
  signal: AbortSignal,
  onFrame: (event: string, data: string) => void,
  onOpen?: () => void,
): Promise<void> {
  if (signal.aborted) return;
  const qs = query.toString();
  const source = new EventSource(qs ? `/api${path}?${qs}` : `/api${path}`);
  let onAbort: (() => void) | undefined;
  try {
    await new Promise<void>((resolve, reject) => {
      onAbort = () => resolve();
      signal.addEventListener("abort", onAbort, { once: true });
      source.onerror = () => resolve();
      source.onopen = () => onOpen?.();
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
    if (onAbort) signal.removeEventListener("abort", onAbort);
    source.close();
  }
}

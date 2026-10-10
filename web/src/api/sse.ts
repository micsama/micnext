import { authHeaders, ensureOk } from "./client";

/**
 * 带 Bearer 头打开 SSE 流，逐帧回调 `event`/`data`，流结束时返回。
 * `EventSource` 不能带 Bearer 头，故用 fetch 读流并自行分帧（只用到 `event`/`data`）。
 */
export async function readSse(
  path: string,
  signal: AbortSignal,
  onFrame: (event: string, data: string) => void,
): Promise<void> {
  const res = await ensureOk(await fetch(`/api${path}`, { headers: authHeaders(), signal }));
  const reader = res.body!.pipeThrough(new TextDecoderStream()).getReader();
  let buf = "";
  let event = "";
  let data: string[] = [];
  for (;;) {
    const { value, done } = await reader.read();
    if (done) return;
    buf += value;
    let nl: number;
    while ((nl = buf.indexOf("\n")) >= 0) {
      const line = buf.slice(0, nl).replace(/\r$/, "");
      buf = buf.slice(nl + 1);
      if (line === "") {
        if (event) onFrame(event, data.join("\n"));
        event = "";
        data = [];
      } else if (line.startsWith(":")) {
        // 心跳注释
      } else {
        const colon = line.indexOf(":");
        const field = colon < 0 ? line : line.slice(0, colon);
        const val = colon < 0 ? "" : line.slice(colon + 1).replace(/^ /, "");
        if (field === "event") event = val;
        else if (field === "data") data.push(val);
      }
    }
  }
}

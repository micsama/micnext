import { authHeaders, ensureOk } from "./client";
import { parseStreamEvent, type StreamEvent } from "./types";

/**
 * 打开会话的 SSE 流，逐个回调事件，流结束时返回。
 * `EventSource` 不能带 Bearer 头，故用 fetch 读流并按 SSE 分帧（只用到 `event`/`id`/`data`）。
 */
export async function readStream(
  sessionId: number,
  after: number | null,
  signal: AbortSignal,
  onEvent: (e: StreamEvent) => void,
): Promise<void> {
  const q = after === null ? "" : `?after=${after}`;
  const res = await ensureOk(
    await fetch(`/api/sessions/${sessionId}/stream${q}`, { headers: authHeaders(), signal }),
  );
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
        if (event) onEvent(parseStreamEvent(event, data.join("\n")));
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

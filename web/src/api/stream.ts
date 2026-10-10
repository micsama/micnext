import { parseLogEvent, type LogEvent } from "./developer";
import { readSse } from "./sse";
import { parseStreamEvent, type StreamEvent } from "./types";

/** 打开会话的 SSE 流，逐个回调事件，流结束时返回。 */
export async function readStream(
  sessionId: number,
  after: number | null,
  signal: AbortSignal,
  onEvent: (e: StreamEvent) => void,
): Promise<void> {
  const q = after === null ? "" : `?after=${after}`;
  await readSse(`/sessions/${sessionId}/stream${q}`, signal, (event, data) => onEvent(parseStreamEvent(event, data)));
}

/** 打开开发者日志流：先回放保留窗口再跟随，流结束时返回。 */
export async function readLogStream(signal: AbortSignal, onEvent: (e: LogEvent) => void): Promise<void> {
  await readSse("/developer/logs/stream", signal, (event, data) => onEvent(parseLogEvent(event, data)));
}

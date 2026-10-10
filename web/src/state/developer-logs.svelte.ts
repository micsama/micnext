import { ApiError } from "../api/client";
import type { LogRecord } from "../api/developer";
import { readLogStream } from "../api/stream";
import { ProtocolError } from "../api/types";
import { sleep } from "../lib/sleep";

const RETRY_MIN_MS = 1000;
const RETRY_MAX_MS = 10_000;
/** 与服务端保留窗口同量级。 */
const MAX_RECORDS = 10_000;
const MAX_BYTES = 16 * 1024 * 1024;

export type LogStatus = "connecting" | "live" | "reconnecting";

/** `n` 为本页面内递增编号，跨重连不复用，供暂停定位。 */
export type LogEntry = LogRecord & { n: number };

/** 开发者日志连接：收到 ready 清空并接收重放，断线按 1/2/4/8/10s 退避重连。 */
export class LogFeed {
  /** 记录量大，不做深层代理；`version` 递增表示内容变化。 */
  records: LogEntry[] = [];
  version = $state(0);
  status = $state<LogStatus>("connecting");
  historyTrimmed = $state(false);
  /** 无法自动恢复的错误（请求被拒、版本不一致），需手动重连。 */
  fatal = $state<string | null>(null);

  #next = 0;
  #sizes: number[] = [];
  #bytes = 0;
  #frame = 0;
  #abort = new AbortController();

  constructor() {
    void this.#run();
  }

  /** 下一条记录将得到的编号。 */
  get next(): number {
    return this.#next;
  }

  close(): void {
    this.#abort.abort();
    cancelAnimationFrame(this.#frame);
    this.#frame = 0;
  }

  reconnect(): void {
    this.close();
    this.#abort = new AbortController();
    this.fatal = null;
    this.status = "connecting";
    void this.#run();
  }

  async #run(): Promise<void> {
    const signal = this.#abort.signal;
    let delay = RETRY_MIN_MS;
    while (!signal.aborted) {
      try {
        await readLogStream(signal, (e) => {
          if (e.event === "ready") {
            this.#clear();
            this.historyTrimmed = e.data.history_trimmed;
            this.status = "live";
            delay = RETRY_MIN_MS;
          } else {
            this.#push(e.data);
          }
        });
      } catch (e) {
        if (signal.aborted) return;
        if (e instanceof ApiError && e.status === 401) return;
        if ((e instanceof ApiError && e.status === 400) || e instanceof ProtocolError) {
          this.fatal = e.message;
          return;
        }
      }
      if (signal.aborted) return;
      this.status = "reconnecting";
      await sleep(delay, signal);
      delay = Math.min(delay * 2, RETRY_MAX_MS);
    }
  }

  #clear(): void {
    this.records = [];
    this.#sizes = [];
    this.#bytes = 0;
    this.#touch();
  }

  #push(record: LogRecord): void {
    const size = sizeOf(record);
    this.records.push({ ...record, n: this.#next++ });
    this.#sizes.push(size);
    this.#bytes += size;
    let drop = 0;
    while (this.records.length - drop > MAX_RECORDS || this.#bytes > MAX_BYTES) {
      this.#bytes -= this.#sizes[drop]!;
      drop += 1;
    }
    if (drop > 0) {
      this.records.splice(0, drop);
      this.#sizes.splice(0, drop);
      this.historyTrimmed = true;
    }
    this.#touch();
  }

  /** 每帧最多通知一次，避免高频日志逐条重算筛选。 */
  #touch(): void {
    if (this.#frame) return;
    this.#frame = requestAnimationFrame(() => {
      this.#frame = 0;
      this.version += 1;
    });
  }
}

/** 文本字节的近似值（UTF-16 码元数 × 2）。 */
function sizeOf(r: LogRecord): number {
  let n = r.target.length + r.message.length + (r.file?.length ?? 0);
  for (const f of r.fields) n += f.name.length + f.value.length;
  return n * 2;
}

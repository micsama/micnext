import { ApiError } from "../api/client";
import { obj, num, ProtocolError } from "../api/decode";
import { readSse } from "../api/sse";
import { sleep } from "../lib/sleep";
import { pushCenter } from "./pushes.svelte";
import { sessionsSync } from "./sessions.svelte";

/** 通知中心、任务页与侧栏共用一条全局流。 */
class GlobalStream {
  schedules = $state(0);
  deliveries = $state(0);
  connected = $state(0);
  error = $state<string | null>(null);

  start(): () => void {
    const abort = new AbortController();
    void this.run(abort.signal);
    return () => abort.abort();
  }

  private async run(signal: AbortSignal): Promise<void> {
    let delay = 1000;
    while (!signal.aborted) {
      try {
        await readSse("/stream", new URLSearchParams(), ["push_created", "push_delivery_changed", "schedule_changed", "session_activity"], signal, (event, data) => {
          obj({ id: num })(JSON.parse(data), "全局事件");
          if (event === "schedule_changed") this.schedules++;
          if (event === "session_activity") sessionsSync.request();
          if (event === "push_created") void pushCenter.refresh();
          if (event === "push_delivery_changed") {
            this.deliveries++;
            void pushCenter.refresh();
          }
        }, () => {
          delay = 1000;
          this.error = null;
          this.connected++;
          void pushCenter.refresh();
          sessionsSync.request();
        });
      } catch (e) {
        if (signal.aborted || (e instanceof ApiError && e.status === 401)) return;
        if (e instanceof ProtocolError) { this.error = e.message; pushCenter.error = e.message; return; }
        this.error = (e as Error).message;
      }
      if (signal.aborted) return;
      await sleep(delay, signal);
      delay = Math.min(delay * 2, 10_000);
    }
  }
}

export const globalStream = new GlobalStream();

import { getTraceSessions } from "../api/client";
import type { TraceSession } from "../api/trace";

/** trace 模式下侧栏与追踪页共用的会话列表。 */
class TraceSessions {
  items = $state.raw<TraceSession[]>([]);
  loaded = $state(false);
  error = $state<string | null>(null);

  async refresh(): Promise<void> {
    try {
      this.items = await getTraceSessions();
      this.loaded = true;
      this.error = null;
    } catch (e) {
      this.error = (e as Error).message;
    }
  }

  title(id: number): string {
    return this.items.find((s) => s.session === id)?.title ?? `会话 ${id}`;
  }
}

export const traceSessions = new TraceSessions();

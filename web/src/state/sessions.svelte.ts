import { listSessions } from "../api/client";
import type { Cursor, SessionItem } from "../api/types";

/** Gateway 之外、Web 只读查看的渠道。新渠道由接入它的 B2 追加（web-ui §五）。 */
export const OTHER_CHANNELS = ["cli"] as const;

const older = (s: SessionItem, c: Cursor) =>
  s.last_activity_at < c.before_at || (s.last_activity_at === c.before_at && s.id < c.before_id);

/** 某渠道的会话列表，最近活跃在前，按页加载。 */
export class SessionList {
  items = $state<SessionItem[]>([]);
  next = $state<Cursor | null>(null);
  loaded = $state(false);
  loading = $state(false);
  error = $state<string | null>(null);

  constructor(readonly channel: string) {}

  /** 重拉第一页，已加载的更早部分保留。 */
  async refresh(): Promise<void> {
    try {
      const page = await listSessions(this.channel, null);
      const ids = new Set(page.items.map((s) => s.id));
      const tail = page.next ? this.items.filter((s) => !ids.has(s.id) && older(s, page.next!)) : [];
      this.items = [...page.items, ...tail];
      if (tail.length === 0) this.next = page.next;
      this.loaded = true;
      this.error = null;
    } catch (e) {
      this.error = (e as Error).message;
    }
  }

  async loadMore(): Promise<void> {
    if (!this.next || this.loading) return;
    this.loading = true;
    try {
      const page = await listSessions(this.channel, this.next);
      const ids = new Set(this.items.map((s) => s.id));
      this.items = [...this.items, ...page.items.filter((s) => !ids.has(s.id))];
      this.next = page.next;
      this.error = null;
    } catch (e) {
      this.error = (e as Error).message;
    } finally {
      this.loading = false;
    }
  }
}

export const webSessions = new SessionList("web");
export const otherSessions = OTHER_CHANNELS.map((c) => new SessionList(c));

export function findSession(id: number): SessionItem | undefined {
  for (const list of [webSessions, ...otherSessions]) {
    const s = list.items.find((i) => i.id === id);
    if (s) return s;
  }
  return undefined;
}

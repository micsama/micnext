import { listPinnedSessions, listSessions } from "../api/client";
import type { Cursor, SessionItem } from "../api/types";

/** Gateway 之外、Web 只读查看的渠道。新渠道由接入它的 B2 追加（web-ui §五）。 */
export const OTHER_CHANNELS = ["cli", "wechat"] as const;

/** 某渠道的未置顶会话，最近活跃在前，按页加载。 */
export class SessionList {
  items = $state<SessionItem[]>([]);
  next = $state<Cursor | null>(null);
  loaded = $state(false);
  loading = $state(false);
  error = $state<string | null>(null);
  #pages = 1;

  constructor(readonly channel: string) {}

  /** 从首页起重拉已加载的页数，整体替换。 */
  async refresh(): Promise<void> {
    try {
      let page = await listSessions(this.channel, null);
      const items = [...page.items];
      for (let n = 1; n < this.#pages && page.next; n++) {
        page = await listSessions(this.channel, page.next);
        items.push(...page.items);
      }
      this.items = items;
      this.next = page.next;
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
      this.#pages++;
      this.error = null;
    } catch (e) {
      this.error = (e as Error).message;
    } finally {
      this.loading = false;
    }
  }
}

/** 跨渠道的置顶会话，一次全部列出。 */
class PinnedList {
  items = $state<SessionItem[]>([]);
  error = $state<string | null>(null);

  async refresh(): Promise<void> {
    try {
      this.items = await listPinnedSessions();
      this.error = null;
    } catch (e) {
      this.error = (e as Error).message;
    }
  }
}

export const pinnedSessions = new PinnedList();
export const webSessions = new SessionList("web");
export const otherSessions = OTHER_CHANNELS.map((c) => new SessionList(c));
const allLists = () => [pinnedSessions, webSessions, ...otherSessions];

const THROTTLE_MS = 500;

/** 侧栏刷新只此一处：事件合并节流，同时只跑一轮，期间的新请求在结束后补一轮。 */
class SessionsSync {
  #dirty = false;
  #running: Promise<void> | null = null;

  request(): void {
    this.#dirty = true;
    this.#running ??= this.#loop().finally(() => (this.#running = null));
  }

  async #loop(): Promise<void> {
    while (this.#dirty) {
      this.#dirty = false;
      await Promise.all(allLists().map((list) => list.refresh()));
      if (this.#dirty) await new Promise((r) => setTimeout(r, THROTTLE_MS));
    }
  }

  /** 已读上报成功后先在本地消点，不等刷新。 */
  markRead(id: number): void {
    for (const list of allLists()) for (const s of list.items) if (s.id === id) s.unread = false;
  }

  get unread(): boolean {
    return pinnedSessions.items.some((s) => s.unread) || webSessions.items.some((s) => s.unread);
  }
}

export const sessionsSync = new SessionsSync();

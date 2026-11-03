import { listPinnedSessions, listSessions, listTags } from "../api/client";
import type { Cursor, SessionItem, Tag } from "../api/types";

/** 标签词表与侧栏的单选筛选；筛选只在本页有效。 */
class Tags {
  items = $state<Tag[]>([]);
  error = $state<string | null>(null);
  filter = $state<number | null>(null);

  byId(id: number): Tag | undefined {
    return this.items.find((t) => t.id === id);
  }

  async refresh(): Promise<void> {
    try {
      this.items = await listTags();
      this.error = null;
      if (this.filter !== null && !this.byId(this.filter)) this.select(null);
    } catch (e) {
      this.error = (e as Error).message;
    }
  }

  select(id: number | null): void {
    if (this.filter === id) return;
    this.filter = id;
    // 交互不进节流队列，立即按新标签拉取；新一轮使旧请求失效，结果回来前保留旧列表。
    for (const list of [webSessions, ...otherSessions]) list.reset();
    for (const list of allLists()) void list.refresh();
  }
}

export const tags = new Tags();

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
  #round = 0;

  constructor(readonly channel: string) {}

  /** 换筛选后只拉首页。 */
  reset(): void {
    this.#pages = 1;
  }

  /** 从首页起重拉已加载的页数，整体替换；只有最新一轮能提交。 */
  async refresh(): Promise<void> {
    const round = ++this.#round;
    const tag = tags.filter;
    const pages = this.#pages;
    try {
      let page = await listSessions(this.channel, tag, null);
      const items = [...page.items];
      for (let n = 1; n < pages && page.next; n++) {
        page = await listSessions(this.channel, tag, page.next);
        items.push(...page.items);
      }
      if (round !== this.#round) return;
      this.items = items;
      this.next = page.next;
      this.loaded = true;
      this.error = null;
    } catch (e) {
      if (round === this.#round) this.error = (e as Error).message;
    } finally {
      if (round === this.#round) this.loading = false;
    }
  }

  /** 多加载一页：页数加一后整体重拉，与刷新共用一条提交路径。 */
  loadMore(): void {
    if (!this.next || this.loading) return;
    this.loading = true;
    this.#pages++;
    void this.refresh();
  }
}

/** 跨渠道的置顶会话，一次全部列出。 */
class PinnedList {
  items = $state<SessionItem[]>([]);
  error = $state<string | null>(null);
  #round = 0;

  async refresh(): Promise<void> {
    const round = ++this.#round;
    try {
      const items = await listPinnedSessions(tags.filter);
      if (round !== this.#round) return;
      this.items = items;
      this.error = null;
    } catch (e) {
      if (round === this.#round) this.error = (e as Error).message;
    }
  }
}

export const pinnedSessions = new PinnedList();
export const webSessions = new SessionList("web");
export const otherSessions = OTHER_CHANNELS.map((c) => new SessionList(c));
const allLists = () => [pinnedSessions, webSessions, ...otherSessions];

const THROTTLE_MS = 500;

/** 后台事件触发的侧栏刷新：合并节流，同时只跑一轮，期间的新请求在结束后补一轮。 */
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
      await Promise.all([tags.refresh(), ...allLists().map((list) => list.refresh())]);
      if (this.#dirty) await new Promise((r) => setTimeout(r, THROTTLE_MS));
    }
  }

  /** 已读上报成功后先在本地消点，不等刷新。 */
  markRead(id: number): void {
    for (const list of allLists()) for (const s of list.items) if (s.id === id) s.unread = false;
  }

  get unread(): boolean {
    return pinnedSessions.items.some((s) => s.unread && !s.running) || webSessions.items.some((s) => s.unread && !s.running);
  }
}

export const sessionsSync = new SessionsSync();

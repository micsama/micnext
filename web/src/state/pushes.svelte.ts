import { listPushes, markPushesRead } from "../api/client";
import { type PushView } from "../api/pushes";
import type { ComposerReply } from "./session.svelte";

/** 通知中心：登录后常驻，新推送到达即重拉首页。 */
class PushCenter {
  items = $state<PushView[]>([]);
  hasMore = $state(false);
  /** 未读条数。 */
  unread = $state(0);
  error = $state<string | null>(null);
  loadingMore = $state(false);
  /** 通知中心是否展开；横幅「打开」也经此展开。 */
  open = $state(false);
  /** 横幅里待查看的新推送，新的在前；空即不显示。 */
  fresh = $state<PushView[]>([]);
  /** 每到一批新推送加一，铃铛据此重放动效。 */
  arrivals = $state(0);
  /** 已见过的最大推送 id；首次加载前为 null，首屏历史不提醒。 */
  #seen: number | null = null;
  /** 【去会话里问】带过去的引用，目标会话打开时取走。 */
  #reply: { session: number; reply: ComposerReply } | null = null;

  #refresh: Promise<void> | null = null;
  #dirty = false;

  refresh(): Promise<void> {
    this.#dirty = true;
    if (this.loadingMore) return this.#refresh ?? Promise.resolve();
    if (this.#refresh) return this.#refresh;
    this.#refresh = (async () => {
      while (this.#dirty && !this.loadingMore) {
        this.#dirty = false;
        const count = this.items.length;
        try {
          const items: PushView[] = [];
          let more = false;
          do {
            const page = await listPushes(items.at(-1)?.id ?? null);
            items.push(...page.pushes);
            more = page.has_more;
            this.unread = page.unread;
          } while (more && items.length < count);
          this.items = items;
          this.hasMore = more;
          this.error = null;
          this.#notice(items);
        } catch (e) { this.error = (e as Error).message; }
      }
    })().finally(() => { this.#refresh = null; });
    return this.#refresh;
  }

  async loadMore(): Promise<void> {
    if (this.loadingMore) return;
    this.loadingMore = true;
    await this.#refresh;
    const last = this.items.at(-1);
    if (!last) { this.loadingMore = false; return; }
    try {
      const page = await listPushes(last.id);
      this.items = [...this.items, ...page.pushes];
      this.hasMore = page.has_more;
      this.unread = page.unread;
    } catch (e) {
      this.error = (e as Error).message;
    } finally {
      this.loadingMore = false;
      if (this.#dirty) void this.refresh();
    }
  }

  /** 展开通知中心，横幅随之清除。 */
  show(): void {
    this.open = true;
    this.fresh = [];
  }

  /** 只隐藏横幅，不标已读。 */
  dismiss(): void {
    this.fresh = [];
  }

  #notice(page: PushView[]): void {
    const seen = this.#seen;
    this.#seen = Math.max(seen ?? 0, page[0]?.id ?? 0);
    if (seen === null) return;
    const added = page.filter((p) => p.id > seen && p.read_at === null);
    if (added.length === 0) return;
    this.arrivals++;
    if (!this.open) this.fresh = [...added, ...this.fresh];
  }

  /** 只标这些实际展示过的。 */
  async markRead(ids: number[]): Promise<void> {
    if (ids.length === 0) return;
    try {
      await markPushesRead(ids);
      await this.refresh();
    } catch (e) {
      this.error = (e as Error).message;
    }
  }

  askIn(push: PushView): void {
    const firstLine = push.text.split("\n", 1)[0]!;
    this.#reply = {
      session: push.source_session_id,
      reply: { label: `关于推送：${firstLine}`, quote: `关于你推送的消息：\n${push.text}` },
    };
  }

  takeReply(session: number): ComposerReply | null {
    if (this.#reply?.session !== session) return null;
    const { reply } = this.#reply;
    this.#reply = null;
    return reply;
  }

}

export const pushCenter = new PushCenter();

import { ApiError, listPushes, markPushesRead } from "../api/client";
import { parsePushCreated, type PushView } from "../api/pushes";
import { readSse } from "../api/sse";
import { ProtocolError } from "../api/types";
import { sleep } from "../lib/sleep";
import type { ComposerReply } from "./session.svelte";

const RETRY_MIN_MS = 1000;
const RETRY_MAX_MS = 10_000;

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

  /** 返回停止函数。 */
  start(): () => void {
    const abort = new AbortController();
    void this.#run(abort.signal);
    return () => abort.abort();
  }

  async refresh(): Promise<void> {
    try {
      const page = await listPushes(null);
      // 已加载的更早条目保留，首页覆盖其中重叠部分。
      const oldest = page.pushes.at(-1)?.id ?? Infinity;
      const older = page.has_more ? this.items.filter((p) => p.id < oldest) : [];
      this.items = [...page.pushes, ...older];
      if (older.length === 0) this.hasMore = page.has_more;
      this.unread = page.unread;
      this.error = null;
      this.#notice(page.pushes);
    } catch (e) {
      this.error = (e as Error).message;
    }
  }

  async loadMore(): Promise<void> {
    const last = this.items.at(-1);
    if (!last || this.loadingMore) return;
    this.loadingMore = true;
    try {
      const page = await listPushes(last.id);
      this.items = [...this.items, ...page.pushes];
      this.hasMore = page.has_more;
      this.unread = page.unread;
    } catch (e) {
      this.error = (e as Error).message;
    } finally {
      this.loadingMore = false;
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

  async #run(signal: AbortSignal): Promise<void> {
    let delay = RETRY_MIN_MS;
    while (!signal.aborted) {
      // 每次接上先补拉，断开期间的推送不丢。
      await this.refresh();
      try {
        await readSse("/pushes/stream", new URLSearchParams(), ["push_created"], signal, (_, data) => {
          parsePushCreated(data);
          delay = RETRY_MIN_MS;
          void this.refresh();
        });
      } catch (e) {
        if (signal.aborted) return;
        if (e instanceof ApiError && e.status === 401) return;
        if (e instanceof ProtocolError) {
          this.error = e.message;
          return;
        }
      }
      if (signal.aborted) return;
      await sleep(delay, signal);
      delay = Math.min(delay * 2, RETRY_MAX_MS);
    }
  }
}

export const pushCenter = new PushCenter();

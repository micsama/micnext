import { searchSessions } from "../api/client";
import type { Cursor, SessionHit } from "../api/types";

const DEBOUNCE_MS = 200;

/** 搜索面板：开关、输入与按页累积的结果；只保留最新一次输入的结果。 */
class SearchPalette {
  open = $state(false);
  query = $state("");
  items = $state<SessionHit[]>([]);
  next = $state<Cursor | null>(null);
  /** 本次输入已有结果返回（含空结果）。 */
  done = $state(false);
  loading = $state(false);
  error = $state<string | null>(null);
  #seq = 0;
  #timer: ReturnType<typeof setTimeout> | undefined;

  toggle(): void {
    this.open = !this.open;
  }

  /** 输入变化：停顿后从第一页重搜。 */
  input(query: string): void {
    this.query = query;
    clearTimeout(this.#timer);
    const seq = ++this.#seq;
    if (!query.trim()) {
      this.items = [];
      this.next = null;
      this.done = false;
      this.loading = false;
      this.error = null;
      return;
    }
    this.loading = true;
    this.#timer = setTimeout(() => void this.#fetch(seq, null), DEBOUNCE_MS);
  }

  loadMore(): void {
    if (!this.next || this.loading) return;
    this.loading = true;
    void this.#fetch(this.#seq, this.next);
  }

  async #fetch(seq: number, before: Cursor | null): Promise<void> {
    try {
      const page = await searchSessions(this.query, before);
      if (seq !== this.#seq) return;
      this.items = before ? [...this.items, ...page.items] : page.items;
      this.next = page.next;
      this.error = null;
      this.done = true;
    } catch (e) {
      if (seq === this.#seq) this.error = (e as Error).message;
    } finally {
      if (seq === this.#seq) this.loading = false;
    }
  }
}

export const searchPalette = new SearchPalette();

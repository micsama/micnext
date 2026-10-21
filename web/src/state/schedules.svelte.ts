import { listSchedules, listFires, updateSchedule, deleteSchedule, type Schedule, type ScheduleFire, type ScheduleUpdate } from "../api/schedules";
import { SvelteMap } from "svelte/reactivity";

class FireList {
  items = $state<ScheduleFire[]>([]);
  more = $state(false);
  loading = $state(false);
  error = $state<string | null>(null);
  #dirty = false;
  constructor(readonly id: number) {}

  async refresh(): Promise<void> {
    if (this.loading) { this.#dirty = true; return; }
    this.loading = true;
    // 保留已加载的深度，同时更新旧分页中的投递事实。
    const count = this.items.length;
    try {
      const items: ScheduleFire[] = [];
      let more = false;
      do {
        const page = await listFires(this.id, items.at(-1)?.id ?? null);
        items.push(...page.fires);
        more = page.has_more;
      } while (more && items.length < count);
      this.items = items;
      this.more = more;
      this.error = null;
    } catch (e) { this.error = (e as Error).message; }
    finally {
      this.loading = false;
      if (this.#dirty) { this.#dirty = false; await this.refresh(); }
    }
  }

  async loadMore(): Promise<void> {
    if (this.loading) return;
    this.loading = true;
    try {
      const page = await listFires(this.id, this.items.at(-1)?.id ?? null);
      this.items = [...this.items, ...page.fires];
      this.more = page.has_more;
      this.error = null;
    } catch (e) { this.error = (e as Error).message; }
    finally {
      this.loading = false;
      if (this.#dirty) { this.#dirty = false; await this.refresh(); }
    }
  }
}

export class ScheduleList {
  items = $state<Schedule[]>([]);
  loaded = $state(false);
  error = $state<string | null>(null);
  expanded = new SvelteMap<number, FireList>();
  #refresh: Promise<void> | null = null;
  #dirty = false;

  refresh(): Promise<void> {
    this.#dirty = true;
    if (this.#refresh) return this.#refresh;
    this.#refresh = (async () => {
      while (this.#dirty) {
        this.#dirty = false;
        try {
          this.items = await listSchedules();
          this.loaded = true;
          this.error = null;
          for (const id of this.expanded.keys()) if (!this.items.some(s => s.id === id)) this.expanded.delete(id);
          await Promise.all([...this.expanded.values()].map(list => list.refresh()));
        } catch (e) { this.error = (e as Error).message; }
      }
    })().finally(() => { this.#refresh = null; });
    return this.#refresh;
  }

  toggle(id: number): void {
    if (this.expanded.has(id)) { this.expanded.delete(id); return; }
    const list = new FireList(id);
    this.expanded.set(id, list);
    void list.loadMore();
  }

  async update(id: number, edit: ScheduleUpdate): Promise<void> {
    await updateSchedule(id, edit);
    await this.refresh();
  }

  async delete(id: number): Promise<void> {
    await deleteSchedule(id);
    this.expanded.delete(id);
    await this.refresh();
  }
}

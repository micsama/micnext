import { getTraceSessions } from "../api/client";
import type { TraceSession } from "../api/trace";

/** trace 模式下侧栏与 Trace 页共用的会话树；接口扁平返回，这里按 `parent` 建索引。 */
class TraceSessions {
  items = $state.raw<TraceSession[]>([]);
  loaded = $state(false);
  error = $state<string | null>(null);
  readonly byId = $derived(new Map(this.items.map((s) => [s.session, s])));
  /** 子会话按 job id 升序。 */
  readonly kids = $derived.by(() => {
    const by = new Map<number, TraceSession[]>();
    for (const s of this.items) if (s.parent !== null) by.set(s.parent, [...(by.get(s.parent) ?? []), s]);
    for (const list of by.values()) list.sort((a, b) => (a.job ?? 0) - (b.job ?? 0));
    return by;
  });

  async refresh(): Promise<void> {
    try {
      this.items = await getTraceSessions();
      this.loaded = true;
      this.error = null;
    } catch (e) {
      this.error = (e as Error).message;
    }
  }

  children(id: number): TraceSession[] {
    return this.kids.get(id) ?? [];
  }

  /** 全部后代，深度优先。 */
  descendants(id: number): TraceSession[] {
    return this.children(id).flatMap((c) => [c, ...this.descendants(c.session)]);
  }

  /** 从根到父，不含自身。 */
  ancestors(id: number): TraceSession[] {
    const parent = this.byId.get(id)?.parent;
    const p = parent == null ? undefined : this.byId.get(parent);
    return p ? [...this.ancestors(p.session), p] : [];
  }

  title(id: number): string {
    const s = this.byId.get(id);
    return s?.label ?? (s?.kind === "task" ? `子任务 #${s.job}` : `会话 ${id}`);
  }
}

/** 会话自身异常：失败轮次加工具报错，中断单列。 */
export const issues = (s: TraceSession) => ({ bad: s.failed_runs + s.tool_errors, interrupted: s.interrupted_runs });

export const traceSessions = new TraceSessions();

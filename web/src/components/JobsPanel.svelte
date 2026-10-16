<script lang="ts">
  import { ApiError, jobOutput, stopJob } from "../api/client";
  import type { JobOutput, JobState, JobView } from "../api/types";
  import type { SessionView } from "../state/session.svelte";
  import { confirm } from "../state/confirm.svelte";
  import { formatDuration } from "../lib/format";
  import Icon from "../lib/Icon.svelte";

  let { view, writable, onclose }: { view: SessionView; writable: boolean; onclose: () => void } = $props();

  const STATE: Record<JobState, { label: string; icon: "loader" | "check" | "error" | "clock" | "stop" | "alert"; tone: string }> = {
    running: { label: "运行中", icon: "loader", tone: "text-primary" },
    completed: { label: "已完成", icon: "check", tone: "text-muted-foreground" },
    failed: { label: "失败", icon: "error", tone: "text-destructive" },
    timed_out: { label: "超时", icon: "clock", tone: "text-warning" },
    cancelled: { label: "已终止", icon: "stop", tone: "text-muted-foreground" },
    interrupted: { label: "已中断", icon: "alert", tone: "text-warning" },
  };
  /** 展开时一次向前多读的字节数。 */
  const PAGE = 16_384;

  let now = $state(Date.now());
  const running = $derived(view.jobs.some((j) => j.state === "running"));
  $effect(() => {
    if (!running) return;
    const timer = setInterval(() => (now = Date.now()), 1000);
    return () => clearInterval(timer);
  });

  function elapsed(j: JobView): string {
    const ms = (j.finished_at ?? now) - j.created_at;
    return j.state === "running" && ms < 60_000 ? `${Math.max(0, Math.floor(ms / 1000))} s` : formatDuration(Math.max(0, ms));
  }

  /** 展开的任务：已读窗口；`pinned` 表示读过更早部分，不再随输出增长换成尾部。 */
  type Opened = { out: JobOutput | null; pinned: boolean; loading: boolean; error: string | null; seen: number };
  let opened = $state<Record<number, Opened>>({});
  let stopping = $state<Record<number, boolean>>({});
  let actionError = $state<string | null>(null);

  async function readTail(id: number) {
    const o = opened[id];
    if (!o) return;
    o.loading = true;
    try {
      o.out = await jobOutput(view.id, id);
      o.error = null;
    } catch (e) { o.error = (e as Error).message; }
    finally { o.loading = false; }
  }

  function toggle(j: JobView) {
    if (opened[j.id]) { delete opened[j.id]; return; }
    opened[j.id] = { out: null, pinned: false, loading: false, error: null, seen: j.output_bytes };
    void readTail(j.id);
  }

  $effect(() => {
    for (const j of view.jobs) {
      const o = opened[j.id];
      if (o && !o.pinned && !o.loading && j.output_bytes !== o.seen) {
        o.seen = j.output_bytes;
        void readTail(j.id);
      }
    }
  });

  async function earlier(id: number) {
    const o = opened[id];
    if (!o?.out) return;
    const from = Math.max(o.out.available_from, o.out.offset - PAGE);
    o.loading = true;
    try {
      const page = await jobOutput(view.id, id, from, o.out.offset - from);
      o.out = { ...o.out, text: page.text + o.out.text, offset: page.offset, available_from: page.available_from };
      o.pinned = true;
      o.error = null;
    } catch (e) { o.error = (e as Error).message; }
    finally { o.loading = false; }
  }

  async function stop(j: JobView) {
    const ok = await confirm.ask({ title: "终止这个任务？", description: `${j.tool_name}：${j.description}`, action: "终止" });
    if (!ok) return;
    stopping[j.id] = true;
    actionError = null;
    try { await stopJob(view.id, j.id); }
    catch (e) { if (!(e instanceof ApiError && e.status === 404)) actionError = (e as Error).message; }
    finally { delete stopping[j.id]; view.refreshJobs(); }
  }
</script>

<aside class="absolute inset-y-0 right-0 z-10 flex w-80 max-w-full flex-col border-l border-border bg-background shadow-lg lg:static lg:shadow-none">
  <div class="flex h-10 shrink-0 items-center justify-between border-b border-border px-3 text-sm font-medium">
    后台任务
    <button type="button" aria-label="收起任务列表" title="收起" class="flex size-6 items-center justify-center rounded text-muted-foreground hover:bg-accent hover:text-foreground" onclick={onclose}><Icon name="x" class="size-3.5" /></button>
  </div>
  {#if view.jobsError ?? actionError}<p role="alert" class="border-b border-border px-3 py-2 text-xs text-destructive">{actionError ?? view.jobsError}</p>{/if}
  <ul class="min-h-0 flex-1 divide-y divide-border overflow-y-auto">
    {#each view.jobs as j (j.id)}
      {@const st = STATE[j.state]}
      {@const o = opened[j.id]}
      <li class="space-y-1.5 px-3 py-2 text-xs">
        <div class="flex items-start gap-2">
          <Icon name={st.icon} class="mt-0.5 size-3.5 shrink-0 {st.tone} {j.state === 'running' ? 'animate-spin' : ''}" />
          <div class="min-w-0 flex-1">
            <p class="truncate text-sm" title={j.description}>{j.description}</p>
            <p class="text-muted-foreground"><span class="font-mono">{j.tool_name}</span> · <span class={st.tone}>{st.label}</span> · {elapsed(j)}</p>
          </div>
          {#if j.state === "running" && writable}
            <button type="button" aria-label="终止任务" title="终止" disabled={stopping[j.id]} class="flex size-6 shrink-0 items-center justify-center rounded text-muted-foreground hover:bg-accent hover:text-destructive disabled:opacity-40" onclick={() => stop(j)}><Icon name="stop" class="size-3.5" /></button>
          {/if}
        </div>
        {#if o}
          <div class="space-y-1">
            {#if o.out && o.out.offset > o.out.available_from}
              <button type="button" class="text-primary hover:underline disabled:opacity-40" disabled={o.loading} onclick={() => earlier(j.id)}>加载更早输出</button>
            {:else if o.out && o.out.available_from > 0}
              <p class="text-muted-foreground">更早的 {o.out.available_from} 字节已丢弃</p>
            {/if}
            {#if o.error}<p class="text-destructive">{o.error}</p>{/if}
            <pre class="max-h-80 overflow-auto rounded bg-muted p-2 font-mono text-[11px] leading-snug whitespace-pre-wrap break-all">{o.out ? o.out.text || "（无输出）" : "读取中…"}</pre>
          </div>
        {:else if j.tail}
          <pre class="rounded bg-muted px-2 py-1 font-mono text-[11px] leading-snug whitespace-pre-wrap break-all text-muted-foreground">{j.tail.trimEnd().split("\n").slice(-3).join("\n")}</pre>
        {/if}
        {#if j.output_bytes > 0}
          <button type="button" class="inline-flex items-center gap-0.5 text-muted-foreground hover:text-foreground" onclick={() => toggle(j)}>
            <Icon name="chevron" class="size-3 transition-transform {o ? 'rotate-90' : ''}" />{o ? "收起输出" : `查看输出（${j.output_bytes} 字节）`}
          </button>
        {/if}
      </li>
    {/each}
  </ul>
</aside>

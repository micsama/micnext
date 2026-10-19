<script lang="ts">
  import { tick, untrack } from "svelte";
  import { slide } from "svelte/transition";
  import { ApiError, jobOutput, stopJob } from "../api/client";
  import { jobActive, type AgentJobView, type JobOutput, type JobView } from "../api/types";
  import type { SessionView } from "../state/session.svelte";
  import { confirm } from "../state/confirm.svelte";
  import { formatDuration } from "../lib/format";
  import { router } from "../state/route.svelte";
  import Icon from "../lib/Icon.svelte";
  import * as Dialog from "$lib/components/ui/dialog";
  import { jobLook } from "./job-look";

  /** 贴在输入框上沿的后台任务条：收起为胶囊，展开按「正在执行 / 可追问 / 已结束」分区。 */
  let { view, writable }: { view: SessionView; writable: boolean } = $props();

  /** 展开时一次向前多读的字节数。 */
  const PAGE = 16_384;

  /** 不限时的后台命令（如 dev server）：常驻运行，不算「在做事」。 */
  const resident = (j: JobView) => j.kind === "tool" && j.timeout_ms === null && j.state === "running";
  const running = $derived(view.jobs.filter((j) => jobActive(j) && !resident(j)));
  const residents = $derived(view.jobs.filter(resident));
  const idle = $derived(view.jobs.filter((j) => j.kind === "agent" && !j.running));
  const ended = $derived(view.jobs.filter((j) => j.kind === "tool" && !jobActive(j)));
  const sections = $derived([
    { title: "正在执行", jobs: running },
    { title: "常驻", jobs: residents },
    { title: "可追问", jobs: idle },
    { title: "已结束", jobs: ended },
  ].filter((s) => s.jobs.length > 0));
  /** 收起时值得提示的任务：运行中（含常驻）或结束未看。 */
  const lead = $derived(view.jobs.filter((j) => jobActive(j) || view.unseenJobs.has(j.id)));
  const freshBad = $derived(lead.some((j) => !jobActive(j) && jobLook(j).bad));
  const succeeded = (j: JobView) => j.kind === "tool" ? j.state === "completed" : j.last_run === "completed";
  /** 胶囊颜色取最需要留意的一项：新失败 > 运行中 > 新完成；常驻不着色。 */
  const dockState = $derived(freshBad ? "failed" : running.length ? "running" : lead.some(succeeded) ? "success" : "idle");
  let panel: HTMLDivElement;

  function dismissOutside(event: PointerEvent) {
    const path = event.composedPath();
    const inComposer = path.some((node) => node instanceof HTMLElement && node.hasAttribute("data-composer-input"));
    if (view.jobsOpen && full === null && !confirm.pending && !path.includes(panel) && !inComposer) view.jobsOpen = false;
  }


  let now = $state(Date.now());
  $effect(() => {
    if (running.length + residents.length === 0) return;
    const timer = setInterval(() => (now = Date.now()), 1000);
    return () => clearInterval(timer);
  });

  function elapsed(j: JobView): string {
    if (j.kind === "agent") return j.running ? "" : `${formatDuration(Math.max(0, now - j.last_active_at))}前`;
    const ms = (j.finished_at ?? now) - j.created_at;
    if (resident(j)) return `已运行 ${formatDuration(Math.max(0, ms))}`;
    return j.state === "running" && ms < 60_000 ? `${Math.max(0, Math.floor(ms / 1000))} s` : formatDuration(Math.max(0, ms));
  }

  /** 展开的任务：已读窗口；`pinned` 表示读过更早部分，不再随输出增长换成尾部。 */
  type Opened = { out: JobOutput | null; pinned: boolean; loading: boolean; error: string | null; seen: number };
  let opened = $state<Record<number, Opened>>({});
  let stopping = $state<Record<number, boolean>>({});
  let actionError = $state<string | null>(null);
  let full = $state<number | null>(null);
  let flash = $state<number | null>(null);
  const rows: Record<number, HTMLElement> = {};

  /** agent 的交付随每个子 run 结束更新；以最近活动时间判断。 */
  const version = (j: JobView) => (j.kind === "tool" ? j.output_bytes : j.last_active_at);
  const hasOutput = (j: JobView) => (j.kind === "tool" ? j.output_bytes > 0 : j.last_run !== null);

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

  function expand(j: JobView) {
    view.unseenJobs.delete(j.id);
    if (opened[j.id]) return;
    opened[j.id] = { out: null, pinned: false, loading: false, error: null, seen: version(j) };
    if (hasOutput(j)) void readTail(j.id);
  }

  function toggle(j: JobView) {
    if (opened[j.id]) delete opened[j.id];
    else expand(j);
  }

  $effect(() => {
    for (const j of view.jobs) {
      const o = opened[j.id];
      if (o && !o.pinned && !o.loading && version(j) !== o.seen) {
        o.seen = version(j);
        if (hasOutput(j)) void readTail(j.id);
      }
    }
  });

  $effect(() => {
    const focus = view.jobFocus;
    if (!focus) return;
    untrack(() => {
      const j = view.jobs.find((x) => x.id === focus.id);
      if (!j) return;
      expand(j);
      flash = null;
      void tick().then(() => {
        rows[j.id]?.scrollIntoView({ block: "nearest" });
        requestAnimationFrame(() => (flash = j.id));
      });
    });
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
    const ok = await confirm.ask(j.kind === "agent"
      ? { title: "终止这个子 agent？", description: `${j.description}（之后仍可由会话追问）`, action: "终止" }
      : { title: "终止这个任务？", description: `${j.tool_name}：${j.description}`, action: "终止" });
    if (!ok) return;
    stopping[j.id] = true;
    actionError = null;
    try { await stopJob(view.id, j.id); }
    catch (e) { if (!(e instanceof ApiError && e.status === 404)) actionError = (e as Error).message; }
    finally { delete stopping[j.id]; view.refreshJobs(); }
  }

  function replyTo(j: AgentJobView) {
    view.reply = { label: `回复 agent #${j.id} · ${j.description}`, quote: `回复子 agent #${j.id}（${j.description}）` };
  }

  const fullJob = $derived(full === null ? undefined : view.jobs.find((j) => j.id === full));
</script>

<svelte:window onpointerdown={dismissOutside} />

{#snippet output(j: JobView, o: Opened, pre: string)}
  {#if o.out && o.out.offset > o.out.available_from}
    <button type="button" class="text-primary hover:underline disabled:opacity-40" disabled={o.loading} onclick={() => earlier(j.id)}>加载更早输出</button>
  {:else if o.out && o.out.available_from > 0}
    <p class="text-muted-foreground">更早的 {o.out.available_from} 字节已丢弃</p>
  {/if}
  {#if o.error}<p class="text-destructive">{o.error}</p>{/if}
  <pre class="{pre} overflow-auto rounded-lg bg-code px-2.5 py-2 font-mono text-[11.5px] leading-snug whitespace-pre-wrap break-all">{!hasOutput(j)
      ? j.kind === "agent" ? "（尚无交付）" : "（无输出）"
      : o.out ? o.out.text || "（无输出）" : "读取中…"}</pre>
{/snippet}

{#snippet row(j: JobView)}
  {@const look = jobLook(j)}
  {@const active = jobActive(j)}
  {@const steady = resident(j)}
  {@const done = j.kind === "tool" && !active}
  {@const o = opened[j.id]}
  {@const status = active ? null : j.kind === "tool" || look.bad ? look.label : null}
  <li bind:this={rows[j.id]} class="group rounded-lg {flash === j.id ? 'flash' : ''}">
    <div class="flex items-center gap-1 rounded-lg pr-1.5 transition-colors hover:bg-muted {o ? 'bg-muted/60' : ''}">
      <button type="button" class="grid min-w-0 flex-1 grid-cols-[24px_minmax(0,1fr)] items-center gap-2.5 rounded-lg py-2 pl-2 text-left outline-none focus-visible:ring-2 focus-visible:ring-ring" aria-expanded={!!o} onclick={() => toggle(j)}>
        <span class="type-chip relative grid size-6 place-items-center rounded-md {steady ? 'text-success' : active ? 'text-primary' : look.bad ? look.tone : done ? 'text-muted-foreground' : 'text-foreground/70'}" title={j.kind === "agent" ? "子 agent" : "后台任务"}>
          <Icon name={j.kind === "agent" ? "bot" : "terminal"} class="size-3.5" />
          {#if steady}<span class="absolute -top-0.5 -right-0.5 size-2 rounded-full bg-success ring-2 ring-card" title="常驻运行中"></span>
          {:else if active}<span class="absolute -top-0.5 -right-0.5 size-2 animate-pulse rounded-full bg-primary ring-2 ring-card motion-reduce:animate-none"></span>
          {:else if view.unseenJobs.has(j.id)}<span class="absolute -top-0.5 -right-0.5 size-2 rounded-full bg-primary ring-2 ring-card" title="结束后未查看"></span>{/if}
        </span>
        <span class="min-w-0">
          <span class="block truncate text-[13px] leading-5 {done && !look.bad ? 'text-muted-foreground' : ''}" title={j.description}>{j.description}</span>
          <span class="block text-[11px] leading-4 text-muted-foreground"><span class="font-mono">{j.kind === "agent" ? "agent" : j.tool_name} #{j.id}</span>{#if status} · <span class={look.tone}>{status}</span>{/if}{#if elapsed(j)} · <span class="tabular-nums">{elapsed(j)}</span>{/if}</span>
        </span>
      </button>
      <span class="flex shrink-0 items-center gap-0.5 opacity-0 transition-opacity group-hover:opacity-100 focus-within:opacity-100 [@media(hover:none)]:opacity-100 {o ? 'opacity-100' : ''}">
        {#if j.kind === "agent"}
          {#if writable}
            <button type="button" aria-label="回复" title={active ? "回复（本轮结束后处理）" : "回复"} class="grid size-6 place-items-center rounded-md text-muted-foreground hover:bg-accent hover:text-foreground" onclick={() => replyTo(j)}><Icon name="reply" class="size-3.5" /></button>
          {/if}
          <button type="button" aria-label="在 Trace 中查看" title="在 Trace 中查看" class="grid size-6 place-items-center rounded-md text-muted-foreground hover:bg-accent hover:text-foreground" onclick={() => router.go(`/s/${j.child_session_id}/trace`)}><Icon name="external" class="size-3.5" /></button>
        {/if}
        {#if active && writable}
          <button type="button" aria-label="终止任务" title="终止" disabled={stopping[j.id]} class="grid size-6 place-items-center rounded-md text-muted-foreground hover:bg-accent hover:text-destructive disabled:opacity-40" onclick={() => stop(j)}><Icon name="stop" class="size-3.5" /></button>
        {/if}
      </span>
      <Icon name="chevron" class="size-3 shrink-0 text-muted-foreground/60 transition-transform {o ? 'rotate-90' : ''}" />
    </div>
    {#if o}
      <div class="space-y-2 pr-3 pb-3 pl-11 text-xs">
        {@render output(j, o, "max-h-48")}
        {#if hasOutput(j)}<button type="button" class="text-muted-foreground hover:text-foreground" onclick={() => (full = j.id)}>全屏查看</button>{/if}
      </div>
    {/if}
  </li>
{/snippet}

<div class="relative z-10 mx-3 h-10">
  <div bind:this={panel} data-state={dockState} data-open={view.jobsOpen || undefined} class="jobs-panel absolute inset-x-0 bottom-1 overflow-hidden border {view.jobsOpen
      ? 'rounded-2xl bg-card shadow-lg'
      : 'right-auto max-w-full rounded-full bg-card'}">
    <button
      type="button"
      title={view.jobsOpen ? "收起后台任务" : "展开后台任务"}
      aria-expanded={view.jobsOpen}
      class="flex h-8 w-full items-center gap-2 rounded-[inherit] px-3 text-left text-xs text-muted-foreground outline-none transition-colors hover:bg-muted hover:text-foreground focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-ring"
      onclick={() => (view.jobsOpen = !view.jobsOpen)}>
      <Icon name={running.length ? "loader" : freshBad ? "alert" : "layers"} class="size-3.5 shrink-0 {running.length ? 'animate-spin motion-reduce:animate-none' : ''} {freshBad ? 'text-destructive' : running.length ? 'text-primary' : ''}" />
      <span class="shrink-0 font-medium {view.jobsOpen ? 'text-foreground' : ''}">后台任务</span>
      <span class="rounded-md bg-muted px-1.5 py-0.5 text-[10px] leading-none tabular-nums">{view.jobs.length}</span>
      {#if !view.jobsOpen && lead.length}
        <span class="flex shrink-0 items-center gap-1" aria-label="任务状态">
          {#each lead.slice(0, 5) as j (j.id)}
            {@const look = jobLook(j)}
            <span title={`${j.description} · ${resident(j) ? "常驻运行中" : look.label}`} class="size-1.5 rounded-full {resident(j) ? 'bg-success' : jobActive(j) ? 'bg-primary animate-pulse motion-reduce:animate-none' : look.bad ? 'bg-destructive' : succeeded(j) ? 'bg-success' : 'bg-muted-foreground/50'}"></span>
          {/each}
          {#if lead.length > 5}<span class="text-[10px] tabular-nums">+{lead.length - 5}</span>{/if}
        </span>
      {/if}
      <span class="flex-1"></span>
      <Icon name="chevron" class="size-3 shrink-0 text-muted-foreground/70 transition-transform motion-reduce:transition-none {view.jobsOpen ? 'rotate-90' : '-rotate-90'}" />
    </button>
    {#if view.jobsOpen}
      <div transition:slide={{ duration: window.matchMedia("(prefers-reduced-motion: reduce)").matches ? 0 : 180 }}>
      {#if view.jobsError ?? actionError}<p role="alert" class="border-t border-border px-3 py-1.5 text-xs text-destructive">{actionError ?? view.jobsError}</p>{/if}
      <ul class="max-h-[40vh] overscroll-contain overflow-y-auto border-t border-border/70 p-1.5">
        {#each sections as s (s.title)}
          {#if sections.length > 1}<li class="px-2.5 pt-2 pb-1 text-[11px] font-medium text-muted-foreground first:pt-1">{s.title}<span class="ml-1.5 font-normal tabular-nums opacity-70">{s.jobs.length}</span></li>{/if}
          {#each s.jobs as j (j.id)}{@render row(j)}{/each}
        {/each}
      </ul>
      </div>
    {/if}
  </div>
</div>

<Dialog.Root bind:open={() => fullJob !== undefined, (o) => !o && (full = null)}>
  {@const o = fullJob && opened[fullJob.id]}
  {#if fullJob && o}
    <Dialog.Content class="flex h-[80vh] flex-col gap-2 sm:max-w-4xl">
      <Dialog.Header>
        <Dialog.Title class="truncate pr-8">{fullJob.description} <span class="font-mono text-muted-foreground">#{fullJob.id}</span></Dialog.Title>
      </Dialog.Header>
      <div class="flex min-h-0 flex-1 flex-col gap-1 text-xs">{@render output(fullJob, o, "min-h-0 flex-1")}</div>
    </Dialog.Content>
  {/if}
</Dialog.Root>

<style>
  /* 收起与展开同一套状态色；呼吸只在收起时。 */
  .jobs-panel {
    --job-color: var(--border);
    border-color: color-mix(in srgb, var(--job-color) 35%, var(--border));
    background-image: linear-gradient(color-mix(in srgb, var(--job-color) 5%, transparent), color-mix(in srgb, var(--job-color) 5%, transparent));
  }
  .jobs-panel[data-state="running"] { --job-color: var(--primary); }
  .jobs-panel[data-state="success"] { --job-color: var(--success); }
  .jobs-panel[data-state="failed"] { --job-color: var(--destructive); }
  .jobs-panel[data-state="running"]:not([data-open]) { animation: breathe 2.8s ease-in-out infinite; }
  .type-chip { background: color-mix(in srgb, currentColor 10%, transparent); }
  @keyframes breathe {
    0%, 100% { border-color: color-mix(in srgb, var(--primary) 35%, var(--border)); box-shadow: 0 0 0 0 transparent; }
    50% { border-color: color-mix(in srgb, var(--primary) 75%, var(--border)); box-shadow: 0 0 12px color-mix(in srgb, var(--primary) 16%, transparent); }
  }
  @media (prefers-reduced-motion: reduce) {
    .jobs-panel[data-state="running"] { animation: none; }
  }

  .flash {
    animation: flash 1.2s ease-out;
  }
  @keyframes flash {
    from {
      background: color-mix(in srgb, var(--primary) 18%, transparent);
    }
  }
</style>

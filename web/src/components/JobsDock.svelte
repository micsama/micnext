<script lang="ts">
  import { tick, untrack } from "svelte";
  import { ApiError, jobOutput, stopJob } from "../api/client";
  import { jobActive, type JobOutput, type JobView } from "../api/types";
  import type { SessionView } from "../state/session.svelte";
  import { confirm } from "../state/confirm.svelte";
  import { formatDuration } from "../lib/format";
  import { router } from "../state/route.svelte";
  import Icon from "../lib/Icon.svelte";
  import * as Dialog from "$lib/components/ui/dialog";
  import { jobLook } from "./job-look";

  /** 贴在输入框上沿的后台任务条：有运行中或结束未看的任务时整条显示，否则收成小标签。 */
  let { view, writable }: { view: SessionView; writable: boolean } = $props();

  /** 展开时一次向前多读的字节数。 */
  const PAGE = 16_384;

  const prominent = (j: JobView) => jobActive(j) || view.unseenJobs.has(j.id);
  const lead = $derived(view.jobs.filter(prominent));
  const ended = $derived(view.jobs.filter((j) => !prominent(j)));
  const runningCount = $derived(view.jobs.filter(jobActive).length);
  const fresh = $derived(lead.filter((j) => !jobActive(j)));
  const freshBad = $derived(fresh.filter((j) => jobLook(j).bad).length);
  const quiet = $derived(!view.jobsOpen && lead.length === 0);

  let now = $state(Date.now());
  $effect(() => {
    if (runningCount === 0) return;
    const timer = setInterval(() => (now = Date.now()), 1000);
    return () => clearInterval(timer);
  });

  function elapsed(j: JobView): string {
    if (j.kind === "agent") return j.running ? "" : `${formatDuration(Math.max(0, now - j.last_active_at))}前`;
    const ms = (j.finished_at ?? now) - j.created_at;
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
      ? { title: "终止这个子任务？", description: `${j.description}（之后仍可由会话追问）`, action: "终止" }
      : { title: "终止这个任务？", description: `${j.tool_name}：${j.description}`, action: "终止" });
    if (!ok) return;
    stopping[j.id] = true;
    actionError = null;
    try { await stopJob(view.id, j.id); }
    catch (e) { if (!(e instanceof ApiError && e.status === 404)) actionError = (e as Error).message; }
    finally { delete stopping[j.id]; view.refreshJobs(); }
  }

  const fullJob = $derived(full === null ? undefined : view.jobs.find((j) => j.id === full));
</script>

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

{#snippet row(j: JobView, low: boolean)}
  {@const look = jobLook(j)}
  {@const active = jobActive(j)}
  {@const o = opened[j.id]}
  <li bind:this={rows[j.id]} class="group rounded-xl {flash === j.id ? 'flash' : ''}">
    <div class="flex items-center gap-1 rounded-xl pr-1 hover:bg-muted">
      <button type="button" class="grid min-w-0 flex-1 grid-cols-[14px_minmax(0,1fr)] items-center gap-2 py-1.5 pl-2 text-left" aria-expanded={!!o} onclick={() => toggle(j)}>
        <Icon name={look.icon} class="size-3.5 {look.tone} {active ? 'animate-spin' : ''} {low ? 'opacity-60' : ''}" />
        <span class="min-w-0">
          <span class="block truncate text-[13px] {low ? 'text-muted-foreground' : ''}" title={j.description}>{j.description}</span>
          <span class="block text-xs {low ? 'text-muted-foreground/70' : 'text-muted-foreground'}"><span class="font-mono">{j.kind === "agent" ? `子任务 #${j.id}` : `${j.tool_name} #${j.id}`}</span> · <span class={low ? "" : look.tone}>{look.label}</span>{#if elapsed(j)} · {elapsed(j)}{/if}</span>
        </span>
      </button>
      <span class="flex shrink-0 items-center gap-0.5 opacity-0 transition-opacity group-hover:opacity-100 focus-within:opacity-100 [@media(hover:none)]:opacity-100 {o ? 'opacity-100' : ''}">
        {#if j.kind === "agent"}
          <button type="button" aria-label="在 Trace 中查看" title="在 Trace 中查看" class="grid size-6 place-items-center rounded-md text-muted-foreground hover:bg-accent hover:text-foreground" onclick={() => router.go(`/s/${j.child_session_id}/trace`)}><Icon name="external" class="size-3.5" /></button>
        {/if}
        {#if active && writable}
          <button type="button" aria-label="终止任务" title="终止" disabled={stopping[j.id]} class="grid size-6 place-items-center rounded-md text-muted-foreground hover:bg-accent hover:text-destructive disabled:opacity-40" onclick={() => stop(j)}><Icon name="stop" class="size-3.5" /></button>
        {/if}
      </span>
      {#if view.unseenJobs.has(j.id)}<span class="mr-1.5 size-1.5 shrink-0 rounded-full bg-primary" title="结束后未查看"></span>{/if}
    </div>
    {#if o}
      <div class="space-y-1 pr-2 pb-2 pl-8 text-xs">
        {@render output(j, o, "max-h-48")}
        {#if hasOutput(j)}<button type="button" class="text-muted-foreground hover:text-foreground" onclick={() => (full = j.id)}>全屏查看</button>{/if}
      </div>
    {/if}
  </li>
{/snippet}

<div class="relative z-10 mx-[18px] -mb-px h-8">
  <div class="absolute inset-x-0 bottom-0 overflow-hidden rounded-t-2xl border border-b-0 {view.jobsOpen
      ? 'border-border bg-card shadow-[0_-6px_24px_#0000000f] dark:shadow-[0_-6px_24px_#00000059]'
      : quiet ? 'right-auto border-transparent' : 'border-border bg-muted'}">
    <button
      type="button"
      title={view.jobsOpen ? "收起" : "展开后台任务"}
      aria-expanded={view.jobsOpen}
      class="flex w-full items-center gap-2 text-left text-xs hover:text-foreground {quiet ? 'mt-1.5 h-[26px] px-2 text-muted-foreground/70' : 'h-8 px-3 text-muted-foreground'}"
      onclick={() => (view.jobsOpen = !view.jobsOpen)}>
      {#if view.jobsOpen}
        <span class="min-w-0 flex-1 truncate"><b class="font-medium text-foreground">后台任务</b> · {view.jobs.length}</span>
      {:else if quiet}
        <Icon name="terminal" class="size-3.5" /><span>{view.jobs.length} 个后台任务</span>
      {:else}
        <span class="inline-flex gap-[3px]">
          {#each lead as j (j.id)}
            {@const look = jobLook(j)}
            <i class="size-1.5 rounded-full {jobActive(j) ? 'animate-pulse bg-primary' : look.bad ? 'bg-destructive' : 'bg-success'}"></i>
          {/each}
        </span>
        <span class="min-w-0 flex-1 truncate">
          {#if runningCount}<b class="font-medium text-foreground">{runningCount}</b> 个运行中{/if}{#if runningCount && fresh.length} · {/if}{#if fresh.length}{fresh.length} 个刚结束{#if freshBad}，<span class="text-destructive">{freshBad} 个异常</span>{/if}{/if}
        </span>
      {/if}
      <Icon name="chevron" class="size-3.5 transition-transform {view.jobsOpen ? 'rotate-90' : '-rotate-90'}" />
    </button>
    {#if view.jobsOpen}
      {#if view.jobsError ?? actionError}<p role="alert" class="border-t border-border px-3 py-1.5 text-xs text-destructive">{actionError ?? view.jobsError}</p>{/if}
      <ul class="max-h-[40vh] overflow-y-auto border-t border-border p-1">
        {#each lead as j (j.id)}{@render row(j, false)}{/each}
        {#if ended.length}
          {#if lead.length}<li class="flex items-center gap-2 px-2 pt-2 pb-1 text-[11px] text-muted-foreground/70"><span class="h-px flex-1 bg-border"></span>已结束<span class="h-px flex-1 bg-border"></span></li>{/if}
          {#each ended as j (j.id)}{@render row(j, true)}{/each}
        {/if}
      </ul>
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
  .flash {
    animation: flash 1.2s ease-out;
  }
  @keyframes flash {
    from {
      background: color-mix(in srgb, var(--primary) 18%, transparent);
    }
  }
</style>

<script lang="ts">
  import PencilIcon from "@lucide/svelte/icons/pencil";
  import type { Schedule, FireOutcome } from "../api/schedules";
  import { outcomeLabel, ruleLabel } from "../api/schedules";
  import type { ScheduleList } from "../state/schedules.svelte";
  import { router } from "../state/route.svelte";
  import { formatCost, formatTokens } from "../lib/format";
  import Icon from "../lib/Icon.svelte";
  import DeliveryChips from "./DeliveryChips.svelte";

  let { task, tasks, onedit }: { task: Schedule; tasks: ScheduleList; onedit: () => void } = $props();
  let saving = $state(false);
  let error = $state<string | null>(null);
  /** 展开过全文的回复。 */
  let opened = $state<number[]>([]);
  const fires = $derived(tasks.expanded.get(task.id));

  const DAY = 86_400_000;
  const clock = (d: Date) => d.toLocaleTimeString("zh-CN", { hour: "2-digit", minute: "2-digit" });
  function when(at: number): string {
    const d = new Date(at);
    const today = new Date().setHours(0, 0, 0, 0);
    if (at >= today && at < today + DAY) return `今天 ${clock(d)}`;
    if (at >= today + DAY && at < today + 2 * DAY) return `明天 ${clock(d)}`;
    if (at >= today - DAY && at < today) return `昨天 ${clock(d)}`;
    return `${d.getMonth() + 1} 月 ${d.getDate()} 日 ${clock(d)}`;
  }

  type Tone = "ok" | "bad" | "run" | "idle";
  const tone = (o: FireOutcome): Tone =>
    o.kind === "completed" ? "ok" : ["failed", "timed_out", "interrupted", "missed"].includes(o.kind) ? "bad" : o.kind === "pending" ? "run" : "idle";
  const DOT: Record<Tone, string> = { ok: "bg-emerald-500", bad: "bg-destructive", run: "bg-blue-500", idle: "bg-muted-foreground/40" };
  const TEXT: Record<Tone, string> = { ok: "text-emerald-600 dark:text-emerald-400", bad: "text-destructive", run: "text-blue-600 dark:text-blue-400", idle: "text-muted-foreground" };

  const badge = $derived(
    task.paused_reason ? { label: "已暂停", cls: "bg-amber-500/10 text-amber-700 dark:text-amber-400" }
    : !task.enabled ? { label: "已停用", cls: "bg-muted text-muted-foreground" }
    : task.next_run_at === null ? { label: "已结束", cls: "bg-muted text-muted-foreground" }
    : null,
  );
  const tokens = $derived(task.usage.input_tokens + task.usage.output_tokens);

  async function toggle() {
    saving = true;
    error = null;
    try { await tasks.update(task.id, { enabled: !task.enabled }); }
    catch (e) { error = (e as Error).message; }
    finally { saving = false; }
  }
</script>

<section class={["overflow-hidden rounded-xl border border-border bg-card shadow-xs transition-colors", !task.enabled && "bg-muted/30"]}>
  <div class="flex items-start gap-3 p-4">
    <div class={["grid size-9 shrink-0 place-items-center rounded-lg", task.enabled && !task.paused_reason ? "bg-primary/10 text-primary" : "bg-muted text-muted-foreground"]}>
      <Icon name="clock" class="size-4.5" />
    </div>
    <div class="min-w-0 flex-1">
      <div class="flex items-center gap-2">
        <h3 class={["truncate font-medium", !task.enabled && "text-muted-foreground"]}>{task.name}</h3>
        {#if badge}<span class="shrink-0 rounded-full px-2 py-0.5 text-xs {badge.cls}">{badge.label}</span>{/if}
      </div>
      <p class="mt-0.5 text-sm text-muted-foreground">
        <span title={task.rule}>{ruleLabel(task.rule)}</span>
        {#if task.next_run_at !== null}<span class="mx-1.5 text-border">|</span>下次 <span class="text-foreground/80 tabular-nums">{when(task.next_run_at)}</span>{/if}
      </p>
      <p class="mt-2 line-clamp-2 text-sm text-foreground/70" title={task.prompt}>{task.prompt}</p>
      {#if task.paused_reason}<p class="mt-2 flex items-start gap-1.5 text-xs text-amber-700 dark:text-amber-400"><Icon name="alert" class="mt-px size-3.5 shrink-0" />{task.paused_reason}</p>{/if}
      {#if error}<p role="alert" class="mt-2 text-xs text-destructive">{error}</p>{/if}
    </div>
    <div class="flex shrink-0 items-center gap-1">
      <button type="button" role="switch" aria-checked={task.enabled} title={task.enabled ? "停用" : "启用"} aria-label={`${task.enabled ? "停用" : "启用"} ${task.name}`} disabled={saving} onclick={toggle} class="relative mr-1 h-5 w-9 rounded-full transition-colors focus-visible:outline-2 focus-visible:outline-ring disabled:opacity-50 {task.enabled ? 'bg-primary' : 'bg-muted-foreground/30'}"><span class="absolute top-0.5 left-0.5 size-4 rounded-full bg-background shadow transition-transform {task.enabled ? 'translate-x-4' : ''}"></span></button>
      <button type="button" title="编辑" aria-label={`编辑 ${task.name}`} class="grid size-8 place-items-center rounded-md text-muted-foreground hover:bg-accent hover:text-foreground" onclick={onedit}><PencilIcon class="size-4" /></button>
    </div>
  </div>

  <div class="flex flex-wrap items-center gap-x-4 gap-y-2 border-t border-border px-4 py-2 text-xs text-muted-foreground">
    <button type="button" class="-ml-1.5 inline-flex items-center gap-2 rounded-md px-1.5 py-1 hover:bg-accent hover:text-foreground" aria-expanded={!!fires} onclick={() => tasks.toggle(task.id)}>
      <Icon name="chevron" class="size-3.5 transition-transform {fires ? 'rotate-90' : ''}" />运行记录
      <span class="flex items-center gap-1" aria-label="最近 10 次触发，新的在前">
        {#each task.recent as fire (fire.id)}
          <span class="size-2 rounded-full {DOT[tone(fire.outcome)]}" title={`${when(fire.due_at)} · ${outcomeLabel(fire.outcome)}`}></span>
        {:else}<span>尚未触发</span>{/each}
      </span>
    </button>
    <span class="ml-auto tabular-nums" title="全部历史触发的已记录调用；不含人工追问与独立压缩{task.usage.unknown_cost > 0 ? `\n${task.usage.unknown_cost} 次未计价` : ''}{task.usage.unknown_usage > 0 ? `\n${task.usage.unknown_usage} 次用量未知` : ''}">
      {task.usage.calls} 次调用 · {formatTokens(tokens)} token · {formatCost(task.usage.cost)}{#if task.usage.unknown_cost > 0 || task.usage.unknown_usage > 0}<Icon name="info" class="ml-1 inline size-3 align-[-2px]" />{/if}
    </span>
    <button type="button" class="inline-flex items-center gap-1 rounded-md px-1.5 py-1 -mr-1.5 hover:bg-accent hover:text-foreground" onclick={() => router.go(`/s/${task.session_id}`)}><Icon name="message" class="size-3.5" />打开会话</button>
  </div>

  {#if fires}
    <div class="border-t border-border bg-muted/20 px-4 py-3">
      <ol>
        {#each fires.items as fire (fire.id)}
          {@const t = tone(fire.outcome)}
          {@const full = opened.includes(fire.id)}
          <li class="relative flex gap-3 pb-4 last:pb-0 [&:not(:last-child)]:before:absolute [&:not(:last-child)]:before:top-4 [&:not(:last-child)]:before:bottom-0 [&:not(:last-child)]:before:left-[3.5px] [&:not(:last-child)]:before:w-px [&:not(:last-child)]:before:bg-border">
            <span class="mt-1.5 size-2 shrink-0 rounded-full {DOT[t]}"></span>
            <div class="min-w-0 flex-1">
              <div class="flex flex-wrap items-center gap-x-3 gap-y-1 text-xs">
                <time class="text-muted-foreground tabular-nums">{when(fire.due_at)}</time>
                <span class="font-medium {TEXT[t]}">{fire.outcome.kind === "completed" ? "完成" : outcomeLabel(fire.outcome)}</span>
                <span class="inline-flex items-center gap-2 text-muted-foreground">
                  {#each fire.pushes as push (push.id)}<DeliveryChips deliveries={push.deliveries} />{/each}
                </span>
                {#if fire.run_id !== null}<button type="button" class="ml-auto inline-flex items-center gap-1 text-muted-foreground hover:text-foreground" onclick={() => router.go(`/s/${task.session_id}/trace?run=${fire.run_id}`)}><Icon name="sparkles" class="size-3" />Trace</button>{/if}
              </div>
              {#if fire.reply}
                <button type="button" class="mt-1.5 block w-full rounded-lg border border-border bg-background px-3 py-2 text-left text-sm break-words whitespace-pre-wrap text-foreground/80 hover:border-foreground/20" title={full ? "收起" : "展开全文"} onclick={() => (opened = full ? opened.filter((id) => id !== fire.id) : [...opened, fire.id])}>
                  <span class={full ? "" : "line-clamp-3"}>{fire.reply}</span>
                </button>
              {/if}
            </div>
          </li>
        {:else}<li class="py-4 text-center text-xs text-muted-foreground">{fires.loading ? "加载中…" : "尚未触发"}</li>{/each}
      </ol>
      {#if fires.error}<p role="alert" class="mt-2 text-xs text-destructive">{fires.error}<button type="button" class="ml-2 underline" onclick={() => fires.refresh()}>重试</button></p>{/if}
      {#if fires.more}<button type="button" disabled={fires.loading} class="mt-3 w-full rounded-md py-1.5 text-xs text-muted-foreground hover:bg-accent hover:text-foreground disabled:opacity-50" onclick={() => fires.loadMore()}>{fires.loading ? "加载中…" : "更早的记录"}</button>{/if}
    </div>
  {/if}
</section>

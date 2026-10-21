<script lang="ts">
  import type { Schedule, FireOutcome, ScheduleUpdate } from "../api/schedules";
  import { outcomeLabel, ruleLabel } from "../api/schedules";
  import { channelLabel } from "../api/pushes";
  import type { ScheduleList } from "../state/schedules.svelte";
  import { router } from "../state/route.svelte";
  import { confirm } from "../state/confirm.svelte";
  import Icon from "../lib/Icon.svelte";
  let { task, tasks }: { task: Schedule; tasks: ScheduleList } = $props();
  let editing = $state(false);
  let name = $state("");
  let rule = $state("");
  let prompt = $state("");
  let saving = $state(false);
  let error = $state<string | null>(null);
  const fires = $derived(tasks.expanded.get(task.id));
  const time = (at: number) => new Date(at).toLocaleString(undefined, { year: "numeric", month: "short", day: "numeric", hour: "2-digit", minute: "2-digit" });
  const tone = (o: FireOutcome) => o.kind === "completed" ? "bg-emerald-500" : ["failed", "timed_out", "interrupted", "missed"].includes(o.kind) ? "bg-destructive" : o.kind === "pending" ? "bg-blue-500" : "bg-muted-foreground/40";

  function edit() {
    name = task.name;
    rule = task.rule;
    prompt = task.prompt;
    error = null;
    editing = true;
  }
  async function update(change: ScheduleUpdate) {
    saving = true;
    error = null;
    try { await tasks.update(task.id, change); editing = false; }
    catch (e) { error = (e as Error).message; }
    finally { saving = false; }
  }
  async function remove() {
    if (!await confirm.ask({ title: `删除「${task.name}」？`, description: "任务会话与执行记录会保留。", action: "删除" })) return;
    saving = true;
    error = null;
    try { await tasks.delete(task.id); }
    catch (e) { error = (e as Error).message; }
    finally { saving = false; }
  }
</script>

<section class="overflow-hidden rounded-xl border border-border bg-background">
  <div class="flex flex-wrap items-start gap-3 p-4">
    <button type="button" class="mt-1 rounded p-1 text-muted-foreground hover:bg-accent" aria-label={fires ? "收起运行记录" : "展开运行记录"} aria-expanded={!!fires} onclick={() => tasks.toggle(task.id)}>
      <Icon name="chevron" class="size-4 {fires ? 'rotate-90' : ''}" />
    </button>
    <div class="min-w-0 flex-1 basis-48">
      <button type="button" class="max-w-full truncate text-left font-medium hover:text-primary" onclick={() => tasks.toggle(task.id)}>{task.name}</button>
      <p class="mt-1 text-sm text-muted-foreground" title={task.rule}>{ruleLabel(task.rule)}</p>
      <p class="mt-1 text-xs text-muted-foreground">下次运行：{task.next_run_at === null ? "已停用" : time(task.next_run_at)}</p>
      {#if task.paused_reason}<p class="mt-2 text-xs text-amber-600 dark:text-amber-400">已暂停：{task.paused_reason}</p>{/if}
    </div>
    <div class="space-y-2 text-xs text-muted-foreground">
      <div class="flex min-h-4 items-center gap-1.5" aria-label="最近 10 次触发，新的在前">
        {#each task.recent as fire (fire.id)}
          <span class="size-2.5 rounded-full {tone(fire.outcome)}" title={`${time(fire.due_at)} · ${outcomeLabel(fire.outcome)}`}></span>
        {:else}<span>尚未触发</span>{/each}
      </div>
      <p title="全部历史触发 run 的已记录调用；不含人工追问与独立压缩">累计 {task.usage.calls} 次调用 · {(task.usage.input_tokens + task.usage.output_tokens).toLocaleString()} token · ${task.usage.cost.toFixed(4)}</p>
      {#if task.usage.unknown_cost > 0 || task.usage.unknown_usage > 0}
        <p>{#if task.usage.unknown_cost > 0}{task.usage.unknown_cost} 次未计价 {/if}{#if task.usage.unknown_usage > 0}{task.usage.unknown_usage} 次用量未知{/if}</p>
      {/if}
    </div>
    <div class="flex items-center gap-2">
      <button type="button" role="switch" aria-checked={task.enabled} aria-label={`${task.enabled ? '停用' : '启用'} ${task.name}`} disabled={saving} onclick={() => update({ enabled: !task.enabled })} class="relative h-5 w-9 rounded-full transition-colors disabled:opacity-50 {task.enabled ? 'bg-primary' : 'bg-muted-foreground/30'}"><span class="absolute top-0.5 left-0.5 size-4 rounded-full bg-background shadow transition-transform {task.enabled ? 'translate-x-4' : ''}"></span></button>
      <button type="button" class="rounded px-2 py-1 text-xs hover:bg-accent disabled:opacity-50" disabled={saving} onclick={edit}>编辑</button>
      <button type="button" class="rounded px-2 py-1 text-xs text-destructive hover:bg-accent disabled:opacity-50" disabled={saving} onclick={remove}>删除</button>
    </div>
  </div>
  {#if error}<p role="alert" class="px-4 pb-3 text-sm text-destructive">{error}</p>{/if}
  {#if editing}
    <form class="space-y-3 border-t border-border bg-muted/30 p-4" onsubmit={(e) => { e.preventDefault(); void update({ name, rule, prompt }); }}>
      <label class="block space-y-1 text-xs text-muted-foreground"><span>名字</span><input bind:value={name} class="w-full rounded-md border border-border bg-background px-3 py-2 text-sm text-foreground" /></label>
      <label class="block space-y-1 text-xs text-muted-foreground"><span>时间规则（服务器本地时区）</span><input bind:value={rule} placeholder="0 9 * * * 或 2026-10-22 09:00" class="w-full rounded-md border border-border bg-background px-3 py-2 font-mono text-sm text-foreground" /></label>
      <label class="block space-y-1 text-xs text-muted-foreground"><span>每次运行的指令</span><textarea bind:value={prompt} rows="5" class="w-full rounded-md border border-border bg-background px-3 py-2 text-sm text-foreground"></textarea></label>
      <div class="flex gap-2"><button type="submit" disabled={saving} class="rounded-md bg-primary px-3 py-1.5 text-sm text-primary-foreground disabled:opacity-50">{saving ? "保存中…" : "保存"}</button><button type="button" disabled={saving} onclick={() => editing = false} class="rounded-md px-3 py-1.5 text-sm hover:bg-accent">取消</button></div>
    </form>
  {/if}
  {#if fires}
    <div class="border-t border-border">
      <div class="flex items-center justify-between bg-muted/30 px-4 py-2 text-xs text-muted-foreground"><span>触发记录</span><button type="button" class="text-primary hover:underline" onclick={() => router.go(`/s/${task.session_id}`)}>打开任务会话 →</button></div>
      {#each fires.items as fire (fire.id)}
        <div class="space-y-2 border-t border-border px-4 py-3 text-sm">
          <div class="flex flex-wrap items-center gap-2"><span class="size-2 rounded-full {tone(fire.outcome)}"></span><time class="text-xs text-muted-foreground">{time(fire.due_at)}</time><span class="text-xs">{outcomeLabel(fire.outcome)}</span>
            {#if fire.run_id !== null}<button type="button" class="ml-auto text-xs text-primary hover:underline" onclick={() => router.go(`/s/${task.session_id}/trace?run=${fire.run_id}`)}>Trace</button>{/if}
          </div>
          {#if fire.reply}<p class="whitespace-pre-wrap break-words text-muted-foreground">{fire.reply}</p>{/if}
          <div class="text-xs text-muted-foreground">
            {#if fire.pushes.length === 0}未推送{:else}
              {#each fire.pushes as push (push.id)}
                <p>已推送{#each push.deliveries as delivery (delivery.channel)} · {channelLabel(delivery.channel)}：{#if delivery.state.kind === "delivered"}已送达{:else if delivery.state.kind === "failed"}<span class="text-destructive">推送未送达（{delivery.state.reason}）</span>{:else}等待送达{/if}{/each}</p>
              {/each}
            {/if}
          </div>
        </div>
      {:else}<p class="px-4 py-6 text-center text-xs text-muted-foreground">{fires.loading ? "加载中…" : "尚未触发"}</p>{/each}
      {#if fires.error}<p role="alert" class="px-4 py-2 text-xs text-destructive">{fires.error}<button type="button" class="ml-2 underline" onclick={() => fires.refresh()}>重试</button></p>{/if}
      {#if fires.more}<button type="button" disabled={fires.loading} class="w-full border-t border-border py-3 text-xs text-primary hover:bg-accent disabled:opacity-50" onclick={() => fires.loadMore()}>{fires.loading ? "加载中…" : "加载更多"}</button>{/if}
    </div>
  {/if}
</section>

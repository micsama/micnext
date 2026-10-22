<script lang="ts">
  import { untrack } from "svelte";
  import Header from "./Header.svelte";
  import Icon from "../lib/Icon.svelte";
  import ScheduleRow from "./ScheduleRow.svelte";
  import ScheduleDialog, { type ScheduleForm } from "./ScheduleDialog.svelte";
  import type { Schedule } from "../api/schedules";
  import { ScheduleList } from "../state/schedules.svelte";
  import { globalStream } from "../state/global-stream.svelte";
  const tasks = new ScheduleList();
  let editing = $state<{ task: Schedule; form: ScheduleForm } | null>(null);
  $effect(() => {
    globalStream.schedules;
    globalStream.deliveries;
    globalStream.connected;
    untrack(() => { void tasks.refresh(); });
  });
</script>

<Header title="定时任务" detail="任务由对话创建，在这里查看运行和调整安排" />
<div class="min-h-0 flex-1 overflow-y-auto p-4 md:p-6">
  <div class="mx-auto max-w-3xl space-y-3">
    {#if globalStream.error}<p role="alert" class="text-sm text-destructive">{globalStream.error}</p>{/if}
    {#if tasks.error}
      <div role="alert" class="flex items-center gap-3 rounded-lg border border-destructive/30 p-4 text-sm text-destructive">
        <span>{tasks.error}</span><button type="button" class="underline" onclick={() => tasks.refresh()}>重试</button>
      </div>
    {/if}
    {#if !tasks.loaded && !tasks.error}
      <p class="py-12 text-center text-sm text-muted-foreground">加载中…</p>
    {:else if tasks.loaded && tasks.items.length === 0}
      <div class="flex flex-col items-center rounded-xl border border-dashed border-border p-12 text-center">
        <div class="mb-3 grid size-10 place-items-center rounded-full bg-muted text-muted-foreground"><Icon name="clock" /></div>
        <p class="font-medium">还没有定时任务</p>
        <p class="mt-2 text-sm text-muted-foreground">在对话里说「每天 9 点…」即可创建</p>
      </div>
    {/if}
    {#each tasks.items as task (task.id)}
      <ScheduleRow {task} {tasks} onedit={() => (editing = { task, form: { name: task.name, rule: task.rule, prompt: task.prompt } })} />
    {/each}
  </div>
</div>

<ScheduleDialog bind:editing {tasks} />

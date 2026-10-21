<script lang="ts">
  import { untrack } from "svelte";
  import Header from "./Header.svelte";
  import ScheduleRow from "./ScheduleRow.svelte";
  import { ScheduleList } from "../state/schedules.svelte";
  import { globalStream } from "../state/global-stream.svelte";
  const tasks = new ScheduleList();
  $effect(() => {
    globalStream.schedules;
    globalStream.deliveries;
    globalStream.connected;
    untrack(() => { void tasks.refresh(); });
  });
</script>

<Header title="定时任务" detail="任务由对话创建，在这里查看运行和调整安排" />
<div class="min-h-0 flex-1 overflow-y-auto p-4 md:p-6">
  <div class="mx-auto max-w-5xl space-y-3">
    {#if globalStream.error}<p role="alert" class="text-sm text-destructive">{globalStream.error}</p>{/if}
    {#if tasks.error}
      <div role="alert" class="flex items-center gap-3 rounded-lg border border-destructive/30 p-4 text-sm text-destructive">
        <span>{tasks.error}</span><button type="button" class="underline" onclick={() => tasks.refresh()}>重试</button>
      </div>
    {/if}
    {#if !tasks.loaded && !tasks.error}
      <p class="py-12 text-center text-sm text-muted-foreground">加载中…</p>
    {:else if tasks.loaded && tasks.items.length === 0}
      <div class="rounded-xl border border-dashed border-border p-12 text-center">
        <p class="font-medium">还没有定时任务</p>
        <p class="mt-2 text-sm text-muted-foreground">在对话里说「每天 9 点…」即可创建</p>
      </div>
    {/if}
    {#each tasks.items as task (task.id)}
      <ScheduleRow {task} {tasks} />
    {/each}
  </div>
</div>

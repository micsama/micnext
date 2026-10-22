<script lang="ts" module>
  export type ScheduleForm = { name: string; rule: string; prompt: string };
</script>

<script lang="ts">
  import { Button } from "$lib/components/ui/button";
  import { Input } from "$lib/components/ui/input";
  import { Textarea } from "$lib/components/ui/textarea";
  import * as Dialog from "$lib/components/ui/dialog";
  import { ruleLabel, type Schedule } from "../api/schedules";
  import type { ScheduleList } from "../state/schedules.svelte";
  import { confirm } from "../state/confirm.svelte";

  let { editing = $bindable(), tasks }: { editing: { task: Schedule; form: ScheduleForm } | null; tasks: ScheduleList } = $props();

  let busy = $state(false);
  let error = $state<string | null>(null);

  const dirty = $derived(!!editing && (["name", "rule", "prompt"] as const).some((k) => editing!.form[k] !== editing!.task[k]));

  function reset() {
    editing = null;
    error = null;
  }

  async function close() {
    if (dirty && !(await confirm.ask({ title: "放弃未保存的修改？", action: "放弃" }))) return;
    reset();
  }

  async function run(action: () => Promise<void>) {
    busy = true;
    error = null;
    try { await action(); reset(); }
    catch (e) { error = (e as Error).message; }
    finally { busy = false; }
  }

  async function remove(t: Schedule) {
    if (!(await confirm.ask({ title: `删除「${t.name}」？`, description: "任务会话与执行记录会保留。", action: "删除" }))) return;
    await run(() => tasks.delete(t.id));
  }
</script>

<Dialog.Root bind:open={() => editing !== null, (o) => !o && void close()}>
  <Dialog.Content class="flex max-h-[85vh] flex-col gap-0 p-0 sm:max-w-lg">
    {#if editing}
      {@const t = editing.task}
      {@const f = editing.form}
      {@const hint = ruleLabel(f.rule.trim())}
      <form class="flex min-h-0 flex-col" onsubmit={(e) => { e.preventDefault(); void run(() => tasks.update(t.id, { ...f })); }}>
        <Dialog.Header class="px-6 pt-6 pb-4">
          <Dialog.Title>编辑定时任务</Dialog.Title>
        </Dialog.Header>
        <div class="min-h-0 flex-1 space-y-4 overflow-y-auto px-6 pb-6">
          <label class="block space-y-1.5 text-sm font-medium">名字<Input bind:value={f.name} /></label>
          <label class="block space-y-1.5 text-sm font-medium">
            时间
            <Input bind:value={f.rule} class="font-mono" placeholder="0 9 * * * 或 2026-10-22 09:00" />
            <span class="block text-xs font-normal text-muted-foreground">{hint !== f.rule.trim() ? `${hint} · ` : ""}cron 五段或单次时刻，按服务器本地时区</span>
          </label>
          <label class="block space-y-1.5 text-sm font-medium">每次运行的指令<Textarea bind:value={f.prompt} rows={6} class="font-normal" /></label>
          {#if error}<p role="alert" class="text-sm text-destructive">{error}</p>{/if}
        </div>
        <Dialog.Footer class="flex-row justify-between border-t px-6 py-4">
          <Button variant="ghost" class="text-destructive" disabled={busy} onclick={() => remove(t)}>删除任务</Button>
          <div class="flex gap-2">
            <Button variant="outline" disabled={busy} onclick={() => close()}>取消</Button>
            <Button type="submit" disabled={busy || !dirty}>{busy ? "保存中…" : "保存"}</Button>
          </div>
        </Dialog.Footer>
      </form>
    {/if}
  </Dialog.Content>
</Dialog.Root>

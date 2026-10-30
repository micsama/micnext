<script lang="ts">
  import { onMount } from "svelte";
  import PencilIcon from "@lucide/svelte/icons/pencil";
  import Trash2Icon from "@lucide/svelte/icons/trash-2";
  import { Button } from "$lib/components/ui/button";
  import { Input } from "$lib/components/ui/input";
  import { deleteEnvVar, listEnvVars, setEnvVar, type EnvVar } from "../../api/client";
  import { confirm } from "../../state/confirm.svelte";
  import Group from "./Group.svelte";

  const MIN_CHARS = 8;

  let vars = $state<EnvVar[]>([]);
  let draft = $state({ name: "", value: "" });
  let editing = $state<{ name: string; value: string } | null>(null);
  let busy = $state(false);
  let error = $state<string | null>(null);

  const refresh = async () => (vars = await listEnvVars());
  onMount(() => void refresh().catch((e: Error) => (error = e.message)));

  async function run(action: () => Promise<unknown>): Promise<boolean> {
    busy = true;
    error = null;
    try {
      await action();
      return true;
    } catch (e) {
      error = (e as Error).message;
      return false;
    } finally {
      busy = false;
      await refresh().catch((e: Error) => (error ??= e.message));
    }
  }

  async function add(ev: SubmitEvent) {
    ev.preventDefault();
    if (await run(() => setEnvVar(draft.name.trim(), draft.value))) draft = { name: "", value: "" };
  }

  async function save(ev: SubmitEvent, e: { name: string; value: string }) {
    ev.preventDefault();
    if (await run(() => setEnvVar(e.name, e.value))) editing = null;
  }

  async function remove(name: string) {
    const ok = await confirm.ask({ title: `删除环境变量 ${name}？`, description: "之后的命令不再带上它。", action: "删除" });
    if (ok) void run(() => deleteEnvVar(name));
  }
</script>

<div class="space-y-8">
  <Group title="命令环境变量" footer={`bash 命令和子 agent 都会带上这些变量，用 $NAME 引用。页面和发给模型的内容都只露出值的两头，模型那边整段换成 <NAME>。值至少 ${MIN_CHARS} 个字符，普通配置写进 skill 或命令里。`}>
    <form class="flex items-center gap-2 px-4 py-3" onsubmit={add}>
      <Input bind:value={draft.name} placeholder="名字，例如：DINGTALK_TOKEN" class="h-8 flex-1 font-mono" autocomplete="off" />
      <Input bind:value={draft.value} type="password" placeholder="值" class="h-8 flex-1" autocomplete="new-password" />
      <Button type="submit" size="sm" disabled={busy || !draft.name.trim() || !draft.value}>添加</Button>
    </form>
    {#each vars as { name, masked } (name)}
      <div class="flex items-center gap-3 px-4 py-2.5">
        <span class="font-mono text-sm">{name}</span>
        {#if editing?.name === name}
          {@const e = editing}
          <form class="flex flex-1 items-center gap-2" onsubmit={(ev) => save(ev, e)}>
            <Input bind:value={e.value} type="password" placeholder="新值" class="h-8 flex-1" autocomplete="new-password" autofocus />
            <Button type="submit" size="sm" disabled={busy || !e.value}>保存</Button>
            <Button variant="ghost" size="sm" disabled={busy} onclick={() => ((editing = null), (error = null))}>取消</Button>
          </form>
        {:else}
          <span class="flex-1 font-mono text-xs text-muted-foreground">{masked}</span>
          <Button variant="ghost" size="icon-sm" aria-label="改值" title="改值" disabled={busy} onclick={() => ((editing = { name, value: "" }), (error = null))}><PencilIcon /></Button>
          <Button variant="ghost" size="icon-sm" aria-label="删除" title="删除" class="text-destructive" disabled={busy} onclick={() => remove(name)}><Trash2Icon /></Button>
        {/if}
      </div>
    {/each}
    {#if error}<p role="alert" class="px-4 py-2 text-sm text-destructive">{error}</p>{/if}
  </Group>
</div>

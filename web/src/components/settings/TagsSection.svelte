<script lang="ts">
  import { onMount } from "svelte";
  import PencilIcon from "@lucide/svelte/icons/pencil";
  import Trash2Icon from "@lucide/svelte/icons/trash-2";
  import { Button } from "$lib/components/ui/button";
  import { Input } from "$lib/components/ui/input";
  import { createTag, deleteTag, renameTag } from "../../api/client";
  import type { Tag } from "../../api/types";
  import { confirm } from "../../state/confirm.svelte";
  import { tags } from "../../state/sessions.svelte";
  import TagPill from "../TagPill.svelte";
  import Group from "./Group.svelte";

  const TAG_CHARS = 4;

  let draft = $state("");
  let editing = $state<{ id: number; name: string } | null>(null);
  let busy = $state(false);
  let error = $state<string | null>(null);

  onMount(() => void tags.refresh());

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
      await tags.refresh();
    }
  }

  async function add(ev: SubmitEvent) {
    ev.preventDefault();
    if (await run(() => createTag(draft))) draft = "";
  }

  async function rename(ev: SubmitEvent, e: { id: number; name: string }) {
    ev.preventDefault();
    if (await run(() => renameTag(e.id, e.name))) editing = null;
  }

  async function remove(t: Tag) {
    const ok = await confirm.ask({
      title: `删除标签「${t.name}」？`,
      description: t.sessions > 0 ? `${t.sessions} 个会话正在用它，删除后会一并去掉。` : "还没有会话用它。",
      action: "删除",
    });
    if (ok) void run(() => deleteTag(t.id));
  }
</script>

<div class="space-y-8">
  <Group title="词表" footer={`新会话生成标题时，会从这里挑最多 2 个标签；在会话页手动改过的不再自动改。每个标签最多 ${TAG_CHARS} 字。`}>
    <form class="flex items-center gap-2 px-4 py-3" onsubmit={add}>
      <Input bind:value={draft} placeholder="新标签，例如：工作" maxlength={TAG_CHARS} class="h-8 flex-1" />
      <Button type="submit" size="sm" disabled={busy || !draft.trim()}>添加</Button>
    </form>
    {#each tags.items as t (t.id)}
      <div class="flex items-center gap-3 px-4 py-2.5">
        {#if editing?.id === t.id}
          {@const e = editing}
          <form class="flex flex-1 items-center gap-2" onsubmit={(ev) => rename(ev, e)}>
            <Input bind:value={e.name} maxlength={TAG_CHARS} class="h-8 flex-1" autofocus />
            <Button type="submit" size="sm" disabled={busy || !e.name.trim() || e.name.trim() === t.name}>保存</Button>
            <Button variant="ghost" size="sm" disabled={busy} onclick={() => ((editing = null), (error = null))}>取消</Button>
          </form>
        {:else}
          <TagPill id={t.id} name={t.name} />
          <span class="flex-1 text-xs text-muted-foreground tabular-nums">{t.sessions} 个会话</span>
          <Button variant="ghost" size="icon-sm" aria-label="改名" title="改名" disabled={busy} onclick={() => ((editing = { id: t.id, name: t.name }), (error = null))}><PencilIcon /></Button>
          <Button variant="ghost" size="icon-sm" aria-label="删除" title="删除" class="text-destructive" disabled={busy} onclick={() => remove(t)}><Trash2Icon /></Button>
        {/if}
      </div>
    {/each}
    {#if error}<p role="alert" class="px-4 py-2 text-sm text-destructive">{error}</p>{/if}
  </Group>
</div>

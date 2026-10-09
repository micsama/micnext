<script lang="ts">
  import ChevronRightIcon from "@lucide/svelte/icons/chevron-right";
  import PlusIcon from "@lucide/svelte/icons/plus";
  import { Badge } from "$lib/components/ui/badge";
  import { Button } from "$lib/components/ui/button";
  import * as Dialog from "$lib/components/ui/dialog";
  import { Input } from "$lib/components/ui/input";
  import { Textarea } from "$lib/components/ui/textarea";
  import { createPersona, deletePersona, updatePersona } from "../../api/client";
  import type { Persona } from "../../api/types";
  import { confirm } from "../../state/confirm.svelte";
  import { settingsStore } from "../../state/settings.svelte";
  import Group from "./Group.svelte";

  let { ondirty }: { ondirty: (dirty: boolean) => void } = $props();

  /** 内置人设只读；自定义人设与新建共用编辑表单，`id` 为 null 即新建。 */
  type Editor =
    | { kind: "view"; persona: Persona }
    | { kind: "edit"; id: number | null; name: string; prompt: string; base: { name: string; prompt: string } };

  let editor = $state<Editor | null>(null);
  let busy = $state(false);
  let error = $state<string | null>(null);

  const custom = $derived(settingsStore.personas.filter((p) => !p.builtin));
  const builtin = $derived(settingsStore.personas.filter((p) => p.builtin));
  const dirty = $derived(
    editor?.kind === "edit" && (editor.name !== editor.base.name || editor.prompt !== editor.base.prompt),
  );
  $effect(() => ondirty(dirty));

  function edit(id: number | null, name: string, prompt: string, base = { name, prompt }) {
    error = null;
    editor = { kind: "edit", id, name, prompt, base };
  }

  function open(p: Persona) {
    error = null;
    if (p.builtin) editor = { kind: "view", persona: p };
    else edit(p.id, p.name, p.prompt);
  }

  function copy(p: Persona) {
    const names = new Set(settingsStore.personas.map((x) => x.name));
    let name = `${p.name} 副本`;
    for (let i = 2; names.has(name); i++) name = `${p.name} 副本 ${i}`;
    edit(null, name, p.prompt, { name: "", prompt: "" });
  }

  async function close() {
    if (dirty && !(await confirm.ask({ title: "放弃未保存的修改？", action: "放弃" }))) return;
    editor = null;
  }

  async function run(action: () => Promise<unknown>) {
    busy = true;
    error = null;
    try {
      await action();
      await settingsStore.refresh();
      editor = null;
    } catch (e) {
      error = (e as Error).message;
    } finally {
      busy = false;
    }
  }

  function save(e: Extract<Editor, { kind: "edit" }>) {
    const { id, name, prompt } = e;
    void run(() => (id === null ? createPersona(name, prompt) : updatePersona(id, name, prompt)));
  }

  async function remove(id: number, name: string) {
    const ok = await confirm.ask({
      title: `删除人设「${name}」？`,
      description: "使用它的会话需要重新选择人设，历史记录不受影响。",
      action: "删除",
    });
    if (ok) void run(() => deletePersona(id));
  }
</script>

{#snippet row(p: Persona)}
  <button
    type="button"
    class="flex w-full items-center gap-3 px-4 py-3 text-left transition-colors hover:bg-accent/60"
    onclick={() => open(p)}
  >
    <div class="min-w-0 flex-1">
      <div class="flex items-center gap-2">
        <span class="truncate text-sm font-medium">{p.name}</span>
        {#if p.id === settingsStore.settings?.default_persona_id}
          <Badge variant="secondary">默认</Badge>
        {/if}
      </div>
      <p class="mt-0.5 truncate text-xs text-muted-foreground">{p.prompt}</p>
    </div>
    <ChevronRightIcon class="size-4 shrink-0 text-muted-foreground" />
  </button>
{/snippet}

<div class="space-y-8">
  <Group title="我的">
    {#snippet actions()}
      <Button variant="ghost" size="xs" onclick={() => edit(null, "", "")}><PlusIcon />新建</Button>
    {/snippet}
    {#each custom as p (p.id)}
      {@render row(p)}
    {:else}
      <p class="px-4 py-6 text-center text-sm text-muted-foreground">还没有自定义人设</p>
    {/each}
  </Group>

  <Group title="内置">
    {#each builtin as p (p.id)}
      {@render row(p)}
    {/each}
  </Group>
</div>

<Dialog.Root bind:open={() => editor !== null, (o) => !o && void close()}>
  <Dialog.Content class="sm:max-w-lg">
    {#if editor?.kind === "view"}
      {@const p = editor.persona}
      <Dialog.Header>
        <Dialog.Title>{p.name}</Dialog.Title>
        <Dialog.Description>内置人设不可修改，可复制后编辑。</Dialog.Description>
      </Dialog.Header>
      <pre class="max-h-80 overflow-auto rounded-lg bg-muted px-3 py-2 font-sans text-sm whitespace-pre-wrap">{p.prompt}</pre>
      <Dialog.Footer>
        <Button onclick={() => copy(p)}>复制为我的人设</Button>
      </Dialog.Footer>
    {:else if editor?.kind === "edit"}
      {@const e = editor}
      <form
        class="contents"
        onsubmit={(ev) => {
          ev.preventDefault();
          save(e);
        }}
      >
        <Dialog.Header>
          <Dialog.Title>{e.id === null ? "新建人设" : "编辑人设"}</Dialog.Title>
        </Dialog.Header>
        <div class="space-y-3">
          <Input bind:value={e.name} placeholder="名称" maxlength={40} required />
          <Textarea bind:value={e.prompt} rows={10} placeholder="描述角色、做事方式与语气" required />
          {#if error}<p class="text-sm text-destructive">{error}</p>{/if}
        </div>
        <Dialog.Footer class="sm:justify-between">
          {#if e.id !== null}
            {@const id = e.id}
            <Button variant="ghost" class="text-destructive" disabled={busy} onclick={() => remove(id, e.base.name)}>
              删除
            </Button>
          {:else}
            <span></span>
          {/if}
          <div class="flex gap-2">
            <Button variant="outline" disabled={busy} onclick={() => close()}>取消</Button>
            <Button type="submit" disabled={busy || !dirty}>保存</Button>
          </div>
        </Dialog.Footer>
      </form>
    {/if}
  </Dialog.Content>
</Dialog.Root>

<script lang="ts">
  import PlusIcon from "@lucide/svelte/icons/plus";
  import XIcon from "@lucide/svelte/icons/x";
  import { Button } from "$lib/components/ui/button";
  import { Input } from "$lib/components/ui/input";
  import * as Select from "$lib/components/ui/select";
  import * as Dialog from "$lib/components/ui/dialog";
  import { deleteEndpoint } from "../../api/client";
  import type { Preset } from "../../api/types";
  import { confirm } from "../../state/confirm.svelte";
  import { settingsStore } from "../../state/settings.svelte";
  import {
    blankRow,
    effortLabel,
    fixedUrl,
    keyHint,
    limitable,
    OLLAMA_URL,
    PRESETS,
    presetLabel,
    probe,
    REASONING,
    saveForm,
    type EndpointForm,
  } from "./endpoint-form";

  let {
    editing = $bindable(),
    ondirty,
  }: {
    editing: { form: EndpointForm; base: EndpointForm } | null;
    ondirty: (dirty: boolean) => void;
  } = $props();

  let busy = $state(false);
  let error = $state<string | null>(null);
  let testing = $state(false);
  let test = $state<{ ok: boolean; text: string } | null>(null);

  const dirty = $derived(!!editing && JSON.stringify(editing.form) !== JSON.stringify(editing.base));
  $effect(() => ondirty(dirty));

  function reset() {
    editing = null;
    error = null;
    test = null;
  }

  async function close() {
    if (dirty && !(await confirm.ask({ title: "放弃未保存的修改？", action: "放弃" }))) return;
    reset();
  }

  async function runTest(f: EndpointForm) {
    testing = true;
    test = null;
    try {
      const names = await probe(f);
      const have = new Set(f.models.map((m) => m.name));
      const added = names.filter((n) => !have.has(n));
      f.models.push(...added.map((n) => blankRow(n)));
      test = { ok: true, text: `连接成功 · 发现 ${names.length} 个模型，新增 ${added.length} 个` };
    } catch (e) {
      test = { ok: false, text: (e as Error).message };
    } finally {
      testing = false;
    }
  }

  async function run(action: () => Promise<unknown>) {
    busy = true;
    error = null;
    try {
      await action();
      reset();
    } catch (e) {
      error = (e as Error).message;
    } finally {
      await settingsStore.refresh();
      busy = false;
    }
  }

  async function remove(id: number, name: string) {
    const ok = await confirm.ask({
      title: `删除服务商「${name}」？`,
      description: "其下模型和已保存的 API key 一并删除，使用这些模型的会话需要重新选择。",
      action: "删除",
    });
    if (ok) void run(() => deleteEndpoint(id));
  }

  const keyPlaceholder = (f: EndpointForm) => (f.key_set ? "留空保留现有 key" : keyHint(f.preset));
</script>

<Dialog.Root bind:open={() => editing !== null, (o) => !o && void close()}>
  <Dialog.Content class="flex max-h-[85vh] flex-col gap-0 p-0 sm:max-w-2xl">
    {#if editing}
      {@const f = editing.form}
      {@const base = editing.base}
      {@const efforts = REASONING[f.preset]}
      <form
        class="flex min-h-0 flex-1 flex-col"
        onsubmit={(ev) => {
          ev.preventDefault();
          void run(() => saveForm($state.snapshot(f), base));
        }}
      >
        <Dialog.Header class="px-6 pt-6 pb-4">
          <Dialog.Title>{f.id === null ? "添加服务商" : f.name || "服务商"}</Dialog.Title>
        </Dialog.Header>

        <div class="min-h-0 flex-1 space-y-6 overflow-y-auto px-6 pb-4">
          <section class="space-y-3">
            <div class="grid grid-cols-[1fr_auto] gap-3">
              <Input bind:value={f.name} placeholder="名称" maxlength={40} required />
              <Select.Root type="single" bind:value={() => f.preset, (v) => (f.preset = v as Preset)}>
                <Select.Trigger class="w-40">{presetLabel(f.preset)}</Select.Trigger>
                <Select.Content>
                  {#each PRESETS as p (p.value)}
                    <Select.Item value={p.value} label={p.label} />
                  {/each}
                </Select.Content>
              </Select.Root>
            </div>
            {#if !fixedUrl(f.preset)}
              <Input
                bind:value={f.base_url}
                class="font-mono text-xs"
                placeholder={f.preset === "ollama" ? OLLAMA_URL : "http://host:8000/v1"}
                required={f.preset === "generic"}
              />
            {/if}
            <div class="flex items-center gap-2">
              <Input
                type="password"
                autocomplete="off"
                bind:value={f.key}
                disabled={f.clear_key}
                class="font-mono text-xs"
                placeholder={keyPlaceholder(f)}
              />
              {#if f.key_set}
                <Button
                  variant={f.clear_key ? "secondary" : "ghost"}
                  size="sm"
                  onclick={() => {
                    f.clear_key = !f.clear_key;
                    f.key = "";
                  }}>{f.clear_key ? "撤销清除" : "清除 key"}</Button
                >
              {/if}
            </div>
            <div class="flex items-center gap-3">
              <Button variant="outline" size="sm" disabled={testing} onclick={() => runTest(f)}>
                {testing ? "连接中…" : "测试连接"}
              </Button>
              {#if test}
                <p class="min-w-0 flex-1 truncate text-xs {test.ok ? 'text-success' : 'text-destructive'}" title={test.text}>
                  {test.text}
                </p>
              {/if}
            </div>
          </section>

          <section class="space-y-2">
            <div
              class="grid items-center gap-2 px-1 text-xs text-muted-foreground {efforts
                ? 'grid-cols-[1fr_6rem_7rem_2rem]'
                : 'grid-cols-[1fr_6rem_2rem]'}"
            >
              <span>模型 ID</span>
              <span>最大输出</span>
              {#if efforts}<span>推理</span>{/if}
              <span></span>
            </div>
            {#each f.models as r, i (i)}
              <div
                class="grid items-center gap-2 {efforts
                  ? 'grid-cols-[1fr_6rem_7rem_2rem]'
                  : 'grid-cols-[1fr_6rem_2rem]'}"
              >
                <Input bind:value={r.name} class="font-mono text-xs" placeholder="模型 ID" aria-label="模型 ID" />
                <Input
                  type="number"
                  min="1"
                  value={limitable(f.preset) ? r.max_tokens : ""}
                  oninput={(e) => (r.max_tokens = e.currentTarget.value)}
                  disabled={!limitable(f.preset)}
                  placeholder={limitable(f.preset) ? "默认" : "不支持"}
                  aria-label="最大输出"
                />
                {#if efforts}
                  <Select.Root type="single" bind:value={r.reasoning_effort}>
                    <Select.Trigger class="w-full" aria-label="推理">
                      {effortLabel(f.preset, r.reasoning_effort)}
                    </Select.Trigger>
                    <Select.Content>
                      {#each efforts as o (o.value)}
                        <Select.Item value={o.value} label={o.label} />
                      {/each}
                    </Select.Content>
                  </Select.Root>
                {/if}
                <Button variant="ghost" size="icon-sm" aria-label="移除" onclick={() => f.models.splice(i, 1)}>
                  <XIcon />
                </Button>
              </div>
            {/each}
            <Button variant="ghost" size="sm" onclick={() => f.models.push(blankRow())}><PlusIcon />添加模型</Button>
          </section>
          {#if error}<p class="text-sm text-destructive">{error}</p>{/if}
        </div>

        <Dialog.Footer class="flex-row justify-between border-t px-6 py-4">
          {#if f.id !== null}
            {@const id = f.id}
            <Button variant="ghost" class="text-destructive" disabled={busy} onclick={() => remove(id, base.name)}>
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

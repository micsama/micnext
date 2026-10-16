<script lang="ts">
  import ChevronRightIcon from "@lucide/svelte/icons/chevron-right";
  import ImageIcon from "@lucide/svelte/icons/image";
  import ImageOffIcon from "@lucide/svelte/icons/image-off";
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
    fixedUrl,
    keyHint,
    limitable,
    OLLAMA_URL,
    PRESETS,
    presetLabel,
    probe,
    saveForm,
    type EndpointForm,
  } from "./endpoint-form";

  let {
    editing = $bindable(),
    focus = null,
    ondirty,
  }: {
    editing: { form: EndpointForm; base: EndpointForm } | null;
    /** 打开时展开并滚到该模型的计费设置。 */
    focus?: number | null;
    ondirty: (dirty: boolean) => void;
  } = $props();
  const reveal = (el: HTMLElement) => void requestAnimationFrame(() => el.scrollIntoView({ block: "center" }));

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
  <Dialog.Content class="flex max-h-[85vh] flex-col gap-0 p-0 sm:max-w-xl">
    {#if editing}
      {@const f = editing.form}
      {@const base = editing.base}
      <form
        class="flex min-h-0 flex-1 flex-col"
        onsubmit={(ev) => {
          ev.preventDefault();
          void run(() => saveForm($state.snapshot(f), base));
        }}
      >
        <Dialog.Header class="px-6 pt-6 pb-4">
          <Dialog.Title>{f.id === null ? "添加服务商" : f.name.trim() || presetLabel(f.preset)}</Dialog.Title>
        </Dialog.Header>

        <div class="min-h-0 flex-1 space-y-4 overflow-y-auto px-6 pb-4">
          <section class="space-y-3">
            <h3 class="text-xs font-medium text-muted-foreground">服务连接</h3>
            <div class="grid grid-cols-[1fr_auto] gap-3">
              <Input bind:value={f.name} placeholder={`名称（默认 ${presetLabel(f.preset)}）`} aria-label="服务商名称" maxlength={40} />
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

          <section class="space-y-1 border-t pt-4">
            <div class="flex items-center justify-between"><h3 class="text-xs font-medium text-muted-foreground">模型 <span class="ml-1 tabular-nums">{f.models.length}</span></h3><Button variant="ghost" size="xs" onclick={() => f.models.push(blankRow())}><PlusIcon />添加模型</Button></div>
            {#if f.models.length === 0}<div class="rounded-lg border border-dashed px-3 py-3 text-center text-xs leading-6 text-muted-foreground">还没有模型<br />测试连接可以发现模型，也可以手动添加。</div>{/if}
            {#each f.models as r, i (r)}
              <div class="border-b last:border-b-0 py-3 space-y-2">
                <div class="model-fields">
                  <label class="min-w-0 space-y-1 text-xs text-muted-foreground">模型 ID<Input bind:value={r.name} class="h-8 font-mono text-xs text-foreground" placeholder="例如 deepseek-flash" /></label>
                  <label class="space-y-1 text-xs text-muted-foreground">上下文 · token<Input class="h-8 text-xs text-foreground" type="number" min="1" value={r.context_window} oninput={(e) => (r.context_window = e.currentTarget.value)} placeholder="100000" /></label>
                  <label class="space-y-1 text-xs text-muted-foreground">最大输出<Input class="h-8 text-xs text-foreground" type="number" min="1" value={limitable(f.preset) ? r.max_tokens : ""} oninput={(e) => (r.max_tokens = e.currentTarget.value)} disabled={!limitable(f.preset)} placeholder={limitable(f.preset) ? "默认" : "不支持"} /></label>
                  <div class="space-y-1 text-xs text-muted-foreground">
                    <span class="block">多模态</span>
                    <Button
                      variant="outline"
                      size="icon-sm"
                      class={r.images ? "border-success/50 bg-success/10 text-success hover:bg-success/15 hover:text-success" : "text-muted-foreground/60"}
                      aria-pressed={r.images}
                      aria-label="多模态"
                      title={r.images ? "多模态：可接收图片（点击关闭）" : "非多模态：图片换成文字占位（点击开启）"}
                      onclick={() => (r.images = !r.images)}
                    >
                      {#if r.images}<ImageIcon />{:else}<ImageOffIcon />{/if}
                    </Button>
                  </div>
                  <Button variant="ghost" size="icon-sm" class="text-muted-foreground" aria-label="移除模型 {r.name}" onclick={() => f.models.splice(i, 1)}><XIcon /></Button>
                </div>
                <details class="group" open={r.id !== null && r.id === focus} {@attach (el) => { if (r.id !== null && r.id === focus) reveal(el); }}>
                  <summary class="flex cursor-pointer list-none items-center gap-1 text-xs text-muted-foreground hover:text-foreground [&::-webkit-details-marker]:hidden">
                    <ChevronRightIcon class="size-3.5 transition-transform group-open:rotate-90" />计费
                    <span class="ml-1 opacity-70">{[r.price_input, r.price_cache_read, r.price_cache_write, r.price_output].filter((v) => v !== "").length ? "已填写单价" : "未设置"}</span>
                    <span class="ml-auto opacity-70">$ / 百万 token</span>
                  </summary>
                  <div class="mt-2 grid grid-cols-2 sm:grid-cols-4 gap-2">
                    {#each [["price_input", "输入"], ["price_output", "输出"], ["price_cache_read", "缓存读取"], ["price_cache_write", "缓存写入"]] as const as [key, label]}
                      <label class="space-y-1 text-xs text-muted-foreground">{label}<Input class="h-8 text-xs text-foreground" type="number" min="0" step="any" value={r[key]} oninput={(e) => (r[key] = e.currentTarget.value)} placeholder={key === "price_cache_write" ? "同输入" : "未设置"} /></label>
                    {/each}
                  </div>
                </details>
              </div>
            {/each}
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

<style>
  .model-fields { display: grid; grid-template-columns: minmax(0, 1fr) 96px 88px 44px 32px; align-items: end; gap: 8px; }
  @media (max-width: 480px) {
    .model-fields { grid-template-columns: minmax(0, 1fr) minmax(0, 1fr) 44px 32px; }
    .model-fields>label:first-child { grid-column: 1 / -1; }
  }
</style>

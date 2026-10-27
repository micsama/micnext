<script lang="ts">
  import ImageOffIcon from "@lucide/svelte/icons/image-off";
  import PlusIcon from "@lucide/svelte/icons/plus";
  import ServerIcon from "@lucide/svelte/icons/server";
  import StarIcon from "@lucide/svelte/icons/star";
  import { Badge } from "$lib/components/ui/badge";
  import { Button } from "$lib/components/ui/button";
  import { setDefaultModel } from "../../api/client";
  import type { Endpoint, Model } from "../../api/types";
  import { settingsStore } from "../../state/settings.svelte";
  import { router } from "../../state/route.svelte";
  import { blankEndpoint, presetLabel, toForm, type EndpointForm } from "./endpoint-form";
  import EndpointDialog from "./EndpointDialog.svelte";

  let { focus = null, ondirty }: { focus?: number | null; ondirty: (dirty: boolean) => void } = $props();

  let editing = $state<{ form: EndpointForm; base: EndpointForm } | null>(null);
  let error = $state<string | null>(null);
  let priceOf = $state<number | null>(null);

  // 从 trace 跳来设置单价：打开所属服务商并展开该模型计费，随即清掉地址里的模型。
  $effect(() => {
    const m = focus === null ? undefined : settingsStore.models.find((m) => m.id === focus);
    const e = m && settingsStore.endpoints.find((e) => e.id === m.endpoint_id);
    if (!m || !e) return;
    priceOf = m.id;
    open(toForm(e, settingsStore.models));
    router.replace("/settings");
  });

  function open(f: EndpointForm) {
    editing = { form: structuredClone(f), base: f };
  }

  async function makeDefault(m: Model) {
    error = null;
    try {
      await setDefaultModel(m.id);
      await settingsStore.refresh();
    } catch (e) {
      error = (e as Error).message;
    }
  }

  const address = (e: Endpoint) => e.config.base_url ?? "官方地址";
  const credential = (e: Endpoint) =>
    e.key_set ? "已保存 key" : e.key_env ? `读取 ${e.key_env}` : "未配置 key";
  const tokens = (n: number) => (n >= 1000 ? `${+(n / 1000).toFixed(1)}K` : String(n));
</script>

<div class="space-y-6">
  {#if error}<p class="text-sm text-destructive">{error}</p>{/if}

  {#each settingsStore.endpoints as e (e.id)}
    {@const models = settingsStore.models.filter((m) => m.endpoint_id === e.id)}
    <section class="overflow-hidden rounded-xl border bg-card">
      <header class="flex items-center gap-3 px-4 py-3.5">
        <div class="flex size-9 shrink-0 items-center justify-center rounded-lg bg-primary/10 text-primary">
          <ServerIcon class="size-4" />
        </div>
        <div class="min-w-0 flex-1">
          <div class="flex items-center gap-2">
            <span class="truncate text-sm font-medium">{e.name}</span>
            <Badge variant="outline" class="font-normal text-muted-foreground">{presetLabel(e.config.preset)}</Badge>
          </div>
          <p class="mt-0.5 flex min-w-0 items-center gap-1.5 text-xs text-muted-foreground">
            <span class="truncate font-mono">{address(e)}</span>
            <span aria-hidden="true">·</span>
            <span class="flex shrink-0 items-center gap-1">
              <span class="size-1.5 rounded-full {e.key_set || e.key_env ? 'bg-success' : 'bg-muted-foreground/40'}"></span>
              {credential(e)}
            </span>
          </p>
        </div>
        <Button variant="ghost" size="sm" onclick={() => open(toForm(e, settingsStore.models))}>编辑</Button>
      </header>
      <ul class="divide-y divide-border border-t bg-muted/20">
        {#each models as m (m.id)}
          {@const isDefault = m.id === settingsStore.defaultModelId}
          <li class="group flex min-h-10 items-center gap-2 px-4 py-1.5">
            {#if m.starred}<span title="已收藏" class="text-warning"><StarIcon class="size-3.5 fill-current" /></span>{/if}
            <span class="truncate font-mono text-xs">{m.name}</span>
            {#if isDefault}<Badge variant="secondary">默认</Badge>{/if}
            {#if !m.images}
              <span title="非多模态：图片换成文字占位" class="text-muted-foreground/70"><ImageOffIcon class="size-3.5" /></span>
            {/if}
            <span class="ml-auto flex shrink-0 items-center gap-2">
              {#if !isDefault}
                <Button
                  variant="ghost"
                  size="xs"
                  class="opacity-0 group-hover:opacity-100 focus-visible:opacity-100 max-md:opacity-100"
                  onclick={() => makeDefault(m)}>设为默认</Button
                >
              {/if}
              {#if m.context_window !== null}
                <span class="text-xs tabular-nums text-muted-foreground" title="上下文窗口">{tokens(m.context_window)}</span>
              {/if}
            </span>
          </li>
        {:else}
          <li class="px-4 py-3 text-xs text-muted-foreground">还没有模型，点「编辑」测试连接即可发现</li>
        {/each}
      </ul>
    </section>
  {:else}
    <div class="flex flex-col items-center gap-3 rounded-xl border bg-card px-4 py-10 text-center">
      <div class="flex size-10 items-center justify-center rounded-lg bg-primary/10 text-primary"><ServerIcon class="size-5" /></div>
      <div class="space-y-1">
        <p class="text-sm font-medium">还没有服务商</p>
        <p class="text-xs text-muted-foreground">添加 DeepSeek、OpenAI 或任意兼容接口</p>
      </div>
      <Button onclick={() => open(blankEndpoint())}><PlusIcon />添加服务商</Button>
    </div>
  {/each}

  {#if settingsStore.endpoints.length}
    <Button variant="outline" class="w-full border-dashed" onclick={() => open(blankEndpoint())}><PlusIcon />添加服务商</Button>
  {/if}
</div>

<EndpointDialog bind:editing focus={priceOf} {ondirty} />

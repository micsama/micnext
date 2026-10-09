<script lang="ts">
  import PlusIcon from "@lucide/svelte/icons/plus";
  import { Badge } from "$lib/components/ui/badge";
  import { Button } from "$lib/components/ui/button";
  import { setDefaultModel } from "../../api/client";
  import type { Endpoint, Model } from "../../api/types";
  import { settingsStore } from "../../state/settings.svelte";
  import { blankEndpoint, presetLabel, toForm, type EndpointForm } from "./endpoint-form";
  import EndpointDialog from "./EndpointDialog.svelte";

  let { ondirty }: { ondirty: (dirty: boolean) => void } = $props();

  let editing = $state<{ form: EndpointForm; base: EndpointForm } | null>(null);
  let error = $state<string | null>(null);

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
    e.key_set ? "已保存 key" : e.key_env ? `读取 ${e.key_env}` : "无 key";
  const meta = (m: Model) =>
    [m.config.max_tokens !== null && `${m.config.max_tokens} tokens`, m.config.reasoning_effort]
      .filter(Boolean)
      .join(" · ");
</script>

<div class="space-y-6">
  {#if error}<p class="text-sm text-destructive">{error}</p>{/if}

  {#each settingsStore.endpoints as e (e.id)}
    {@const models = settingsStore.models.filter((m) => m.endpoint_id === e.id)}
    <section class="overflow-hidden rounded-xl border bg-card">
      <header class="flex items-center gap-3 px-4 py-3">
        <div class="min-w-0 flex-1">
          <div class="flex items-center gap-2">
            <span class="truncate text-sm font-medium">{e.name}</span>
            <Badge variant="outline">{presetLabel(e.config.preset)}</Badge>
          </div>
          <p class="mt-0.5 truncate text-xs text-muted-foreground">
            <span class="font-mono">{address(e)}</span> · {credential(e)}
          </p>
        </div>
        <Button variant="outline" size="sm" onclick={() => open(toForm(e, settingsStore.models))}>编辑</Button>
      </header>
      <ul class="divide-y divide-border border-t">
        {#each models as m (m.id)}
          {@const isDefault = m.id === settingsStore.defaultModelId}
          <li class="group flex min-h-11 items-center gap-2 px-4 py-2">
            <span class="truncate font-mono text-xs">{m.name}</span>
            {#if isDefault}<Badge variant="secondary">默认</Badge>{/if}
            <span class="ml-auto truncate text-xs text-muted-foreground">{meta(m)}</span>
            {#if !isDefault}
              <Button
                variant="ghost"
                size="xs"
                class="opacity-0 group-hover:opacity-100 focus-visible:opacity-100 max-md:opacity-100"
                onclick={() => makeDefault(m)}>设为默认</Button
              >
            {/if}
          </li>
        {:else}
          <li class="px-4 py-3 text-xs text-muted-foreground">还没有模型</li>
        {/each}
      </ul>
    </section>
  {:else}
    <div class="flex flex-col items-center gap-3 rounded-xl border bg-card px-4 py-10">
      <p class="text-sm text-muted-foreground">还没有服务商</p>
      <Button onclick={() => open(blankEndpoint())}><PlusIcon />添加服务商</Button>
    </div>
  {/each}

  {#if settingsStore.endpoints.length}
    <Button variant="outline" onclick={() => open(blankEndpoint())}><PlusIcon />添加服务商</Button>
  {/if}
</div>

<EndpointDialog bind:editing {ondirty} />

<script lang="ts">
  import { Popover } from "bits-ui";
  import { settingsStore } from "../state/settings.svelte";
  import type { ReasoningEffort } from "../api/types";
  import Icon from "../lib/Icon.svelte";
  import ReasoningSlider, { reasoningLabel } from "./ReasoningSlider.svelte";

  let { value, onpick, reasoningEffort, onreasoning, ondefault, disabled = false }: {
    value: number | null;
    onpick: (id: number) => void;
    reasoningEffort: ReasoningEffort | null;
    onreasoning: (effort: ReasoningEffort | null) => void;
    disabled?: boolean;
    ondefault?: () => void;
  } = $props();

  let open = $state(false);
  const followingDefault = $derived(value === null && !!ondefault);
  const model = $derived(followingDefault ? (settingsStore.defaultModelId === null ? null : settingsStore.model(settingsStore.defaultModelId)) : value === null ? null : settingsStore.model(value));
  const effort = $derived(reasoningEffort ?? model?.reasoning?.default ?? null);
  const strength = $derived(effort === null ? null : reasoningLabel(effort));
  const showReasoning = $derived(!!model && (model.reasoning !== null || reasoningEffort !== null));
</script>

<Popover.Root bind:open>
  <Popover.Trigger
    aria-label="选择模型和思考强度"
    title={model ? `${model.name}${strength ? ` · ${strength}` : ""}` : "请选择模型"}
    class="inline-flex h-9 min-w-0 max-w-52 items-center gap-2 rounded-full bg-muted px-3.5 text-sm text-foreground transition-colors hover:bg-accent focus-visible:outline-2 focus-visible:outline-ring data-[state=open]:bg-accent sm:max-w-72">
    <span class="truncate">{followingDefault ? "默认模型" : (model?.name ?? "请选择模型")}</span>
    {#if strength}
      <span class="shrink-0 text-muted-foreground/50">·</span>
      <span class="shrink-0 text-muted-foreground">{strength}</span>
    {/if}
    <Icon name="chevron" class="size-3.5 shrink-0 rotate-90 text-muted-foreground transition-transform {open ? 'rotate-270' : ''}" />
  </Popover.Trigger>
  <Popover.Portal>
    <Popover.Content
      side="top"
      align="end"
      sideOffset={8}
      collisionPadding={16}
      aria-label="模型和思考强度"
      class="picker-panel z-50 w-72 max-w-[calc(100vw-2rem)] overflow-hidden rounded-[20px] border border-border/70 bg-popover/95 p-2 text-popover-foreground outline-none backdrop-blur-xl">
      <div class="max-h-56 overflow-y-auto overscroll-contain py-0.5">
        {#if ondefault}
          <button type="button" {disabled} aria-pressed={followingDefault} onclick={ondefault} class="flex min-h-9 w-full items-center justify-between gap-3 rounded-lg px-2.5 py-2 text-left text-sm hover:bg-muted {followingDefault ? 'bg-muted font-medium' : ''}">
            <span>使用默认模型</span>{#if followingDefault}<Icon name="check" class="size-4" />{/if}
          </button>
        {/if}
        {#each settingsStore.endpoints as endpoint (endpoint.id)}
          {@const models = settingsStore.models.filter((m) => m.endpoint_id === endpoint.id)}
          {#if models.length > 0}
            <p class="px-2.5 pb-1 pt-2 text-[10px] font-medium tracking-wide text-muted-foreground">{endpoint.name}</p>
            {#each models as choice (choice.id)}
              <button
                type="button"
                {disabled}
                aria-pressed={choice.id === value}
                title={choice.name}
                class="flex min-h-9 w-full items-center justify-between gap-3 rounded-lg px-2.5 py-2 text-left text-sm transition-colors hover:bg-muted focus-visible:bg-muted focus-visible:outline-none {choice.id === value ? 'bg-muted font-medium' : ''}"
                onclick={() => { if (choice.id !== value) onpick(choice.id); }}>
                <span class="truncate">{choice.name}</span>
                {#if choice.id === value}<Icon name="check" class="size-4 shrink-0 text-foreground" />{/if}
              </button>
            {/each}
          {/if}
        {/each}
        {#if settingsStore.models.length === 0}
          <p class="px-3 py-4 text-sm text-muted-foreground">先到设置里添加模型</p>
        {/if}
      </div>
      {#if followingDefault}
        <p class="mx-2.5 mt-2 border-t border-border/70 pb-1 pt-3 text-xs text-muted-foreground">{model ? `跟随 ${model.name} 的默认档位` : "尚未设置默认模型"}</p>
      {:else if model && showReasoning}
        <div class="mx-2.5 mt-1.5 border-t border-border/70 pb-1.5 pt-3">
          <ReasoningSlider options={model.reasoning} value={reasoningEffort} onpick={onreasoning} {disabled} />
        </div>
      {/if}
    </Popover.Content>
  </Popover.Portal>
</Popover.Root>

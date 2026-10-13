<script lang="ts">
  import { Popover } from "bits-ui";
  import Icon from "../lib/Icon.svelte";
  import { settingsStore } from "../state/settings.svelte";

  let { value, onpick, disabled = false }: { value: number; onpick: (id: number) => void; disabled?: boolean } = $props();

  let open = $state(false);
  const persona = $derived(settingsStore.persona(value));

  function choose(id: number) {
    if (id !== value) onpick(id);
    open = false;
  }
</script>

<Popover.Root bind:open>
  <Popover.Trigger
    {disabled}
    aria-label="选择人设"
    title={persona?.name ?? "选择人设"}
    class="inline-flex h-9 min-w-0 max-w-36 items-center gap-2 rounded-full bg-muted px-3.5 text-sm text-foreground transition-colors hover:bg-accent focus-visible:outline-2 focus-visible:outline-ring disabled:opacity-50 data-[state=open]:bg-accent sm:max-w-44">
    <Icon name="sparkles" class="size-3.5 shrink-0 text-muted-foreground" />
    <span class="truncate">{persona?.name ?? "请选择人设"}</span>
    <Icon name="chevron" class="size-3.5 shrink-0 rotate-90 text-muted-foreground transition-transform {open ? 'rotate-270' : ''}" />
  </Popover.Trigger>
  <Popover.Portal>
    <Popover.Content
      side="top"
      align="start"
      sideOffset={8}
      collisionPadding={16}
      aria-label="人设"
      class="picker-panel z-50 w-64 max-w-[calc(100vw-2rem)] overflow-hidden rounded-[20px] border border-border/70 bg-popover/95 p-2 text-popover-foreground outline-none backdrop-blur-xl">
      <p class="px-2.5 pb-1 pt-2 text-[10px] font-medium tracking-wide text-muted-foreground">人设</p>
      <div class="max-h-64 overflow-y-auto overscroll-contain py-0.5">
        {#each settingsStore.personas as choice (choice.id)}
          <button
            type="button"
            {disabled}
            aria-pressed={choice.id === value}
            title={choice.name}
            class="flex min-h-10 w-full items-center gap-2.5 rounded-lg px-2.5 py-2 text-left text-sm transition-colors hover:bg-muted focus-visible:bg-muted focus-visible:outline-none disabled:opacity-50 {choice.id === value ? 'bg-muted font-medium' : ''}"
            onclick={() => choose(choice.id)}>
            <span aria-hidden="true" class="grid size-6 shrink-0 place-items-center rounded-lg text-[11px] font-medium {choice.id === value ? 'bg-primary/10 text-primary' : 'bg-muted text-muted-foreground'}">{Array.from(choice.name)[0]}</span>
            <span class="min-w-0 flex-1 truncate">{choice.name}</span>
            {#if choice.id === value}<Icon name="check" class="size-4 shrink-0 text-foreground" />{/if}
          </button>
        {/each}
        {#if settingsStore.personas.length === 0}
          <p class="px-2.5 py-3 text-sm text-muted-foreground">暂无人设</p>
        {/if}
      </div>
    </Popover.Content>
  </Popover.Portal>
</Popover.Root>

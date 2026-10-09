<script lang="ts">
  import * as Select from "$lib/components/ui/select";
  import Icon from "../lib/Icon.svelte";
  import { settingsStore } from "../state/settings.svelte";

  let { value, onpick, disabled = false }: { value: number; onpick: (id: number) => void; disabled?: boolean } =
    $props();
</script>

<Select.Root type="single" value={String(value)} onValueChange={(v) => onpick(Number(v))} {disabled}>
  <Select.Trigger
    aria-label="人设"
    title="人设"
    class="h-9 max-w-44 rounded-full border-0 bg-transparent px-3 text-sm text-muted-foreground shadow-none hover:bg-accent hover:text-foreground dark:bg-transparent dark:hover:bg-accent">
    <Icon name="sparkles" class="size-4" />
    <span class="truncate">{settingsStore.persona(value)?.name ?? "请选择"}</span>
  </Select.Trigger>
  <Select.Content align="start">
    {#each settingsStore.personas as p (p.id)}
      <Select.Item value={String(p.id)} label={p.name} />
    {/each}
  </Select.Content>
</Select.Root>

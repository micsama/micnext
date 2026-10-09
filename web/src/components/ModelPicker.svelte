<script lang="ts">
  import * as Select from "$lib/components/ui/select";
  import { settingsStore } from "../state/settings.svelte";

  let { value, onpick, disabled = false }: { value: number | null; onpick: (id: number) => void; disabled?: boolean } =
    $props();
</script>

<Select.Root type="single" value={value === null ? "" : String(value)} onValueChange={(v) => onpick(Number(v))} {disabled}>
  <Select.Trigger
    aria-label="模型"
    title="模型"
    class="h-9 max-w-56 rounded-full border-0 bg-transparent px-3 text-sm text-muted-foreground shadow-none hover:bg-accent hover:text-foreground dark:bg-transparent dark:hover:bg-accent">
    <span class="truncate">{value === null ? "请选择模型" : (settingsStore.model(value)?.name ?? "请选择模型")}</span>
  </Select.Trigger>
  <Select.Content align="end">
    {#each settingsStore.endpoints as e (e.id)}
      <Select.Group>
        <Select.GroupHeading>{e.name}</Select.GroupHeading>
        {#each settingsStore.models.filter((m) => m.endpoint_id === e.id) as m (m.id)}
          <Select.Item value={String(m.id)} label={m.name} />
        {/each}
      </Select.Group>
    {/each}
  </Select.Content>
</Select.Root>

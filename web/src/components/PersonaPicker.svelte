<script lang="ts">
  import { settingsStore } from "../state/settings.svelte";

  let { value, onpick, disabled = false }: { value: number; onpick: (id: number) => void; disabled?: boolean } =
    $props();
</script>

<select
  aria-label="人设"
  title="人设"
  {value}
  {disabled}
  onchange={(e) => onpick(Number(e.currentTarget.value))}
  class="h-8 max-w-32 shrink-0 truncate rounded-full border border-border bg-muted px-2 text-xs text-muted-foreground outline-none hover:text-foreground focus:border-primary disabled:opacity-60">
  {#each settingsStore.personas as p (p.id)}
    <option value={p.id}>{p.name}</option>
  {/each}
</select>

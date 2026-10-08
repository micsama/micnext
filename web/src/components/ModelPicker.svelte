<script lang="ts">
  import { settingsStore } from "../state/settings.svelte";

  let { value, onpick, disabled = false }: { value: number | null; onpick: (id: number) => void; disabled?: boolean } =
    $props();
</script>

<select
  aria-label="模型"
  title="模型"
  {value}
  {disabled}
  onchange={(e) => onpick(Number(e.currentTarget.value))}
  class="h-8 max-w-40 shrink-0 truncate rounded-full border border-line bg-panel px-2 text-xs text-muted outline-none hover:text-fg focus:border-accent disabled:opacity-60">
  {#if value === null}
    <option value="" selected disabled>请选择</option>
  {/if}
  {#each settingsStore.endpoints as e (e.id)}
    <optgroup label={e.name}>
      {#each settingsStore.models.filter((m) => m.endpoint_id === e.id) as m (m.id)}
        <option value={m.id}>{m.name}</option>
      {/each}
    </optgroup>
  {/each}
</select>

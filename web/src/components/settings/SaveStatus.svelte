<script lang="ts" module>
  export type SaveState = { kind: "saving" } | { kind: "saved" } | { kind: "error"; message: string };
</script>

<script lang="ts">
  import CheckIcon from "@lucide/svelte/icons/check";
  import LoaderIcon from "@lucide/svelte/icons/loader-circle";
  import { fade } from "svelte/transition";

  let { state }: { state: SaveState | undefined } = $props();
</script>

{#if state?.kind === "saving"}
  <LoaderIcon class="size-3.5 animate-spin text-muted-foreground" aria-label="保存中" />
{:else if state?.kind === "saved"}
  <span class="inline-flex items-center gap-1 text-xs text-success" role="status" out:fade={{ duration: 300 }}>
    <CheckIcon class="size-3.5" />已保存
  </span>
{/if}

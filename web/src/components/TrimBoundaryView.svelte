<script lang="ts">
  import type { Trim } from "../api/types";
  import type { SessionView } from "../state/session.svelte";
  import Undo2Icon from "@lucide/svelte/icons/undo-2";
  let { id, trim, view }: { id: number; trim: Trim; view: SessionView } = $props();
  const revocation = $derived(view.context?.revocable.find((c) => c.boundary_id === id)?.state);
  const revoked = $derived(revocation?.kind === "revoked");
  const canRevoke = $derived(view.info?.writable && revocation?.kind === "applied" && view.context?.active_boundary === id);
  let error = $state<string | null>(null);
  async function revoke() {
    error = null;
    try { await view.revoke(id); }
    catch (e) { error = (e as Error).message; }
  }
</script>

<div class="flex items-center gap-3 text-xs text-muted-foreground">
  <div class="h-px flex-1 bg-border"></div>
  <span class="inline-flex items-center gap-1">
    {#if revoked}<Undo2Icon class="size-3.5" />已撤回整理{:else}长时间未活动，已整理历史 · 省略 {trim.elided.length} 条旧内容{/if}
    {#if canRevoke}
      <button type="button" class="ml-1 inline-flex items-center gap-1 rounded-full px-2 py-0.5 text-destructive hover:bg-destructive/10 disabled:opacity-50" disabled={view.revoking !== null} onclick={revoke}><Undo2Icon class="size-3.5" />{view.revoking === id ? "撤回排队中…" : "撤回"}</button>
    {/if}
  </span>
  <div class="h-px flex-1 bg-border"></div>
</div>
{#if error}<p class="text-center text-xs text-destructive">{error}</p>{/if}

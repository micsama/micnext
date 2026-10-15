<script lang="ts">
  import type { Compaction } from "../api/types";
  import type { SessionView } from "../state/session.svelte";
  import Markdown from "./Markdown.svelte";
  let { id, result, view }: { id: number; result: Compaction; view: SessionView } = $props();
  const compactionState = $derived(view.context?.compactions.find((c) => c.boundary_id === id)?.state);
  const revoked = $derived(compactionState?.kind === "revoked");
  const canRevoke = $derived(view.info?.writable && compactionState?.kind === "applied" && view.context?.active_boundary === id);
  let error = $state<string | null>(null);
  async function revoke() {
    error = null;
    try { await view.revoke(id); }
    catch (e) { error = (e as Error).message; }
  }
</script>

<details class="group/compact text-sm">
  <summary class="flex cursor-pointer list-none items-center gap-3 text-xs text-muted-foreground select-none hover:text-foreground [&::-webkit-details-marker]:hidden">
    <div class="h-px flex-1 bg-border"></div>
    <span class={revoked ? "" : "text-emerald-600 dark:text-emerald-400"}>{revoked ? "↶" : "✓"}</span>{revoked ? "已撤回压缩" : "已压缩"} ·
    <span class="group-open/compact:hidden">查看摘要</span><span class="hidden group-open/compact:inline">收起</span>
    <div class="h-px flex-1 bg-border"></div>
  </summary>
  <div class="mt-3 space-y-4 rounded-xl border border-border px-4 py-3">
    <Markdown source={result.summary} />
    {#each result.files as file (file.path)}
      <details class="rounded-lg border border-border">
        <summary class="cursor-pointer px-3 py-2 font-mono text-xs">{file.path}</summary>
        {#if "Read" in file.outcome}
          <pre class="max-h-72 overflow-auto border-t border-border px-3 py-2 text-xs whitespace-pre-wrap">{file.outcome.Read.text}</pre>
          {#if file.outcome.Read.extent === "Truncated"}<p class="px-3 pb-2 text-xs text-muted-foreground">快照已截断</p>{/if}
        {:else}
          <p class="px-3 pb-2 text-xs text-muted-foreground">{file.outcome.Unavailable.reason}</p>
        {/if}
      </details>
    {/each}
    {#if canRevoke}
      <button type="button" class="rounded-lg border border-border px-3 py-1.5 text-xs hover:bg-muted disabled:opacity-50" disabled={view.revoking !== null} onclick={revoke}>{view.revoking === id ? "撤回排队中…" : "撤回压缩"}</button>
    {/if}
    {#if error}<p class="text-xs text-destructive">{error}</p>{/if}
  </div>
</details>

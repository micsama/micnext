<script lang="ts">
  import type { Compaction } from "../api/types";
  import type { SessionView } from "../state/session.svelte";
  import Markdown from "./Markdown.svelte";
  let { id, result, view }: { id: number; result: Compaction; view: SessionView } = $props();
  const compactionState = $derived(view.context?.compactions.find((c) => c.boundary_id === id)?.state);
  const revoked = $derived(compactionState?.kind === "revoked");
  const canRevoke = $derived(view.info?.writable && compactionState?.kind === "applied" && view.context?.active_boundary === id);
  const kept = $derived(result.keep_from === null ? 0 : view.messages.filter((m) => m.body.kind === "Reply" && m.id >= result.keep_from! && m.id < id).length);
  let error = $state<string | null>(null);
  async function revoke() {
    error = null;
    try { await view.revoke(id); }
    catch (e) { error = (e as Error).message; }
  }
</script>

<details class="rounded-xl border border-border bg-muted/30 text-sm">
  <summary class="flex cursor-pointer items-center gap-2 px-4 py-3 select-none">
    <span class={revoked ? "text-muted-foreground" : "text-emerald-600 dark:text-emerald-400"}>{revoked ? "↶" : "✓"}</span>
    <span class="font-medium">{revoked ? "已撤回" : "已压缩"}</span>
    <span class="text-xs text-muted-foreground">保留 {kept} 步 · 查看摘要</span>
  </summary>
  <div class="space-y-4 border-t border-border px-4 py-3">
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

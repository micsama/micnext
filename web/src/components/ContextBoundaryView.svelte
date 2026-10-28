<script lang="ts">
  import type { Compaction } from "../api/types";
  import type { SessionView } from "../state/session.svelte";
  import Markdown from "./Markdown.svelte";
  import CopyButton from "./CopyButton.svelte";
  import CheckIcon from "@lucide/svelte/icons/check";
  import Undo2Icon from "@lucide/svelte/icons/undo-2";
  import ChevronUpIcon from "@lucide/svelte/icons/chevron-up";
  let { id, result, view }: { id: number; result: Compaction; view: SessionView } = $props();
  const compactionState = $derived(view.context?.revocable.find((c) => c.boundary_id === id)?.state);
  const revoked = $derived(compactionState?.kind === "revoked");
  const canRevoke = $derived(view.info?.writable && compactionState?.kind === "applied" && view.context?.active_boundary === id);
  const retained = $derived([
    result.elided.length ? `省略 ${result.elided.length} 条工具输出` : "",
    result.quoted.length ? `带回 ${result.quoted.length} 条用户原话` : "",
  ].filter(Boolean).join(" · "));
  let error = $state<string | null>(null);
  let el: HTMLDetailsElement;
  let flash = $state(false);
  // 展开时把摘要开头滚进视野；收起后拉回分隔条并闪一下，视线有落点。
  function ontoggle() {
    const behavior = matchMedia("(prefers-reduced-motion: reduce)").matches ? "instant" : "smooth";
    if (el.open) {
      flash = false;
      el.scrollIntoView({ behavior, block: "start" });
    } else {
      el.scrollIntoView({ behavior, block: "nearest" });
      flash = true;
    }
  }
  async function revoke() {
    error = null;
    try { await view.revoke(id); }
    catch (e) { error = (e as Error).message; }
  }
</script>

<details class="group/compact text-sm" bind:this={el} {ontoggle}>
  <summary class="sticky top-2 z-10 flex cursor-pointer list-none items-center justify-center gap-3 text-xs text-muted-foreground select-none hover:text-foreground group-open/compact:hover:text-muted-foreground [&::-webkit-details-marker]:hidden">
    <div class="h-px flex-1 bg-border group-open/compact:hidden"></div>
    <span class:summary-flash={flash} onanimationend={() => (flash = false)} class="inline-flex items-center gap-1 rounded-full border border-border bg-background px-2.5 py-0.5 group-open/compact:py-1 group-open/compact:pr-1 group-open/compact:pl-3 group-open/compact:shadow-lg">
      {#if revoked}<Undo2Icon class="size-3.5" />已撤回压缩{:else}<CheckIcon class="size-3.5 text-emerald-600 dark:text-emerald-400" />已压缩{/if}{#if retained} · {retained}{/if}<span class="group-open/compact:hidden"> · 查看摘要</span>
      <span class="ml-2 hidden items-center gap-1 group-open/compact:inline-flex">
        <span role="presentation" onclick={(e) => e.preventDefault()}><CopyButton text={result.summary} prominent /></span>
        <span class="inline-flex items-center gap-1 rounded-full bg-muted px-2.5 py-1 font-medium text-foreground hover:bg-accent"><ChevronUpIcon class="size-3.5" />收起</span>
      </span>
    </span>
    <div class="h-px flex-1 bg-border group-open/compact:hidden"></div>
  </summary>
  <div class="summary-panel mt-2 space-y-4 rounded-xl border border-border px-4 py-3">
    <Markdown source={result.summary} />
    {#if result.files.length}
      <div class="space-y-1 text-xs text-muted-foreground">
        <p>继续工作时按需读取</p>
        {#each result.files as path (path)}<p class="font-mono break-all">{path}</p>{/each}
      </div>
    {/if}
    {#if error}<p class="text-xs text-destructive">{error}</p>{/if}
    {#if canRevoke}
      <button type="button" class="inline-flex items-center gap-1 rounded-lg border border-destructive/40 px-3 py-1.5 text-xs font-medium text-destructive hover:bg-destructive/10 disabled:opacity-50" disabled={view.revoking !== null} onclick={revoke}><Undo2Icon class="size-3.5" />{view.revoking === id ? "撤回排队中…" : "撤回压缩"}</button>
    {/if}
  </div>
</details>

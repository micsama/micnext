<script lang="ts">
  import type { Draft } from "../state/session.svelte";
  import Markdown from "./Markdown.svelte";
  import ReasoningFold from "./ReasoningFold.svelte";

  let { draft }: { draft: Draft } = $props();

  // 正文每帧至多重渲染一次。
  let shown = $state("");
  let frame = 0;
  $effect(() => {
    void draft.text;
    if (frame) return;
    frame = requestAnimationFrame(() => {
      frame = 0;
      shown = draft.text;
    });
  });
  $effect(() => () => cancelAnimationFrame(frame));

  let box: HTMLDivElement | undefined = $state();
  $effect(() => {
    void draft.reasoning;
    if (box) box.scrollTop = box.scrollHeight;
  });
</script>

{#if draft.reasoning.trim()}
  {#if draft.text}
    <ReasoningFold text={draft.reasoning.trim()} />
  {:else}
    <div class="my-1 text-sm">
      <div class="animate-pulse text-muted">思考中…</div>
      <div bind:this={box} class="mt-1 max-h-32 overflow-y-auto border-l-2 border-line pl-3 whitespace-pre-wrap text-muted">
        {draft.reasoning.trim()}
      </div>
    </div>
  {/if}
{/if}
{#if shown}
  <Markdown source={shown} />
{/if}

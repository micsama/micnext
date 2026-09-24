<script lang="ts">
  import type { ReplyBlock, ToolResultOutcome } from "../api/types";
  import CopyButton from "./CopyButton.svelte";
  import Markdown from "./Markdown.svelte";
  import ReasoningFold from "./ReasoningFold.svelte";
  import ToolRow from "./ToolRow.svelte";

  let {
    blocks,
    results,
    running,
  }: { blocks: ReplyBlock[]; results: Map<string, ToolResultOutcome>; running: boolean } = $props();

  const text = $derived(
    blocks
      .flatMap((b) => ("Text" in b ? [b.Text.text] : []))
      .join("\n\n")
      .trim(),
  );
</script>

<div class="group/reply">
  {#each blocks as block, i (i)}
    {#if "Reasoning" in block}
      {#if "Visible" in block.Reasoning}
        {#if block.Reasoning.Visible.text.trim()}
          <ReasoningFold text={block.Reasoning.Visible.text.trim()} />
        {/if}
      {:else}
        <p class="my-1 text-sm text-muted">（推理内容已加密）</p>
      {/if}
    {:else if "Text" in block}
      <Markdown source={block.Text.text} />
    {:else}
      <ToolRow
        name={block.ToolCall.name}
        args={block.ToolCall.args}
        result={results.get(block.ToolCall.id)}
        {running} />
    {/if}
  {/each}
  {#if text}
    <div class="mt-1 opacity-0 transition-opacity group-hover/reply:opacity-100 focus-within:opacity-100">
      <CopyButton {text} />
    </div>
  {/if}
</div>

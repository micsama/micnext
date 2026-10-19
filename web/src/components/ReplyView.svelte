<script lang="ts">
  import type { ReplyBlock, ReplyStats, ReplyUsage, ToolResultOutcome } from "../api/types";
  import { formatDuration, formatTokens } from "../lib/format";
  import CopyButton from "./CopyButton.svelte";
  import Markdown from "./Markdown.svelte";
  import ReasoningFold from "./ReasoningFold.svelte";
  import ToolRow from "./ToolRow.svelte";
  import Icon from "../lib/Icon.svelte";

  let {
    blocks,
    results,
    running,
    meta,
    stats,
  }: { blocks: ReplyBlock[]; results: Map<string, ToolResultOutcome>; running: boolean; meta: string; stats: ReplyStats } = $props();

  const usage = (u: ReplyUsage | null) =>
    u === null
      ? "用量未知"
      : `输入 ${formatTokens(u.input_tokens)} · 输出 ${formatTokens(u.output_tokens)} · ${
          u.input_tokens > 0 && u.cache_read_tokens !== null ? `缓存 ${Math.round((u.cache_read_tokens / u.input_tokens) * 100)}%` : "缓存未知"
        }`;
  const metrics = $derived(`模型 ${formatDuration(stats.model_ms)} · ${usage(stats.usage)}`);

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
        <p class="my-1 flex items-center gap-1.5 text-xs text-muted-foreground" title="模型未提供思考摘要"><Icon name="bulb" class="size-3.5" />已思考</p>
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
  <div class="mt-1 flex h-5 items-center gap-2 overflow-x-auto whitespace-nowrap [scrollbar-width:none] text-xs text-muted-foreground opacity-0 transition-opacity group-hover/reply:opacity-100 focus-within:opacity-100">
    {#if text}<CopyButton {text} />{/if}<span>{meta}</span>
    <span>{metrics}</span>
  </div>
</div>

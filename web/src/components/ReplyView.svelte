<script lang="ts">
  import { toolKey } from "./tool-results";
  import ArrowDownIcon from "@lucide/svelte/icons/arrow-down";
  import ArrowUpIcon from "@lucide/svelte/icons/arrow-up";
  import GaugeIcon from "@lucide/svelte/icons/gauge";
  import TimerIcon from "@lucide/svelte/icons/timer";
  import type { ReplyBlock, ReplyStats, ToolResultOutcome } from "../api/types";
  import { formatDuration, formatTokens } from "../lib/format";
  import CopyButton from "./CopyButton.svelte";
  import Markdown from "./Markdown.svelte";
  import ReasoningFold from "./ReasoningFold.svelte";
  import ToolRow from "./ToolRow.svelte";
  import Icon from "../lib/Icon.svelte";

  let {
    replyId,
    blocks,
    results,
    running,
    time,
    model,
    stats,
  }: { replyId: number; blocks: ReplyBlock[]; results: Map<string, ToolResultOutcome>; running: boolean; time: string; model: string; stats: ReplyStats } = $props();

  const u = $derived(stats.usage);
  /** 出字速度按首字之后的生成时长算，排除排队与首字等待。 */
  const tps = $derived.by(() => {
    if (!u || stats.first_output_ms === null) return null;
    const ms = stats.model_ms - stats.first_output_ms;
    return ms > 0 && u.output_tokens > 0 ? (u.output_tokens * 1000) / ms : null;
  });
  const hit = $derived(u && u.input_tokens > 0 && u.cache_read_tokens !== null ? u.cache_read_tokens / u.input_tokens : null);
  const RING = 2 * Math.PI * 5;
  const ringColor = (r: number) => (r >= 0.6 ? "text-emerald-500" : r >= 0.2 ? "text-amber-500" : "text-muted-foreground/60");

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
        result={results.get(toolKey({ reply: replyId, index: i }))}
        {running} />
    {/if}
  {/each}
  <div class="mt-1 flex h-6 items-center gap-3 overflow-x-auto whitespace-nowrap [scrollbar-width:none] text-xs text-muted-foreground tabular-nums opacity-0 transition-opacity group-hover/reply:opacity-100 focus-within:opacity-100">
    {#if text}<CopyButton {text} />{/if}
    <span class="inline-flex items-center gap-1"><Icon name="clock" class="size-3" />{time}</span>
    <span class="inline-flex items-center gap-1"><Icon name="bot" class="size-3 text-violet-600 dark:text-violet-400" />{model}</span>
    <span class="h-3 w-px bg-border"></span>
    <span class="inline-flex items-center gap-1" title="模型耗时"><TimerIcon class="size-3 text-sky-600 dark:text-sky-400" />{formatDuration(stats.model_ms)}</span>
    {#if stats.first_output_ms !== null}<span class="inline-flex items-center gap-1" title="首字延迟（TTFT）"><Icon name="zap" class="size-3 text-sky-600 dark:text-sky-400" />{formatDuration(stats.first_output_ms)}</span>{/if}
    {#if tps !== null}<span class="inline-flex items-center gap-1" title="出字速度（TPS，首字之后）"><GaugeIcon class="size-3 text-emerald-600 dark:text-emerald-400" />{tps.toFixed(tps < 10 ? 1 : 0)} tok/s</span>{/if}
    <span class="h-3 w-px bg-border"></span>
    {#if u}
      <span class="inline-flex items-center gap-0.5" title="输入 token"><ArrowUpIcon class="size-3 text-amber-600 dark:text-amber-400" />{formatTokens(u.input_tokens)}</span>
      <span class="inline-flex items-center gap-0.5" title="输出 token"><ArrowDownIcon class="size-3 text-amber-600 dark:text-amber-400" />{formatTokens(u.output_tokens)}</span>
      {#if hit !== null}
        <span class="inline-flex items-center gap-1" title={hit > 0 ? `缓存命中 ${formatTokens(u.cache_read_tokens ?? 0)} / ${formatTokens(u.input_tokens)}` : "未命中缓存（首轮或缓存过期时正常）"}>
          <svg viewBox="0 0 14 14" class="size-3.5 -rotate-90 {ringColor(hit)}" aria-hidden="true">
            <circle cx="7" cy="7" r="5" fill="none" stroke-width="2" class="stroke-muted-foreground/20" />
            {#if hit > 0}<circle cx="7" cy="7" r="5" fill="none" stroke-width="2" stroke="currentColor" stroke-linecap="round" stroke-dasharray="{hit * RING} {RING}" />{/if}
          </svg>缓存 {Math.round(hit * 100)}%
        </span>
      {:else}<span>缓存未知</span>{/if}
    {:else}<span>用量未知</span>{/if}
  </div>
</div>

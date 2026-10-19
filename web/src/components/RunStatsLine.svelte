<script lang="ts">
  import type { ChatRunStats, RunState } from "../api/types";
  import { formatCost, formatDuration, formatTokens } from "../lib/format";

  /** `inline` 时嵌在回复的悬停栏里，否则独占一行。 */
  let { stats: s, inline = false }: { stats: ChatRunStats; inline?: boolean } = $props();

  const ENDING: Partial<Record<RunState, string>> = { stopped: "已停止", provider_failed: "调用失败", max_turns: "达到上限", interrupted: "已中断", timed_out: "超时" };
  const ge = $derived(s.usage_complete ? "" : "≥");
  const cache = $derived(
    s.usage_complete && s.cache_complete && s.input_tokens > 0 ? `缓存 ${Math.round((s.cache_read_tokens / s.input_tokens) * 100)}%` : "缓存未知",
  );
  const parts = $derived(
    [
      ENDING[s.state],
      s.total_ms === null ? null : formatDuration(s.total_ms),
      `${s.steps} 步${s.tools > 0 ? ` · ${s.tools} 工具` : ""}`,
      `输出 ${ge}${formatTokens(s.output_tokens)}`,
      cache,
      s.cost === null ? null : formatCost(s.cost),
    ].filter((p) => p),
  );
  const detail = $derived(
    [
      `模型 ${formatDuration(s.model_ms)} · 工具 ${formatDuration(s.tool_ms)}`,
      `${s.requests} 次请求${s.first_output_ms === null ? "" : ` · 首字 ${formatDuration(s.first_output_ms)}`}${s.output_tps === null ? "" : ` · 输出 ${s.output_tps.toFixed(1)} tok/s`}`,
      `累计输入 ${ge}${formatTokens(s.input_tokens)}（每步重发上下文）· 输出 ${ge}${formatTokens(s.output_tokens)} · 缓存读 ${s.cache_complete ? "" : "≥"}${formatTokens(s.cache_read_tokens)}`,
      s.cost === null ? "费用未知：有模型未定价" : null,
    ]
      .filter((p) => p)
      .join("\n"),
  );
</script>

<p class="{inline ? '' : '-mt-2'} w-fit cursor-default text-xs text-muted-foreground/70 tabular-nums" title={detail}>{parts.join(" · ")}</p>

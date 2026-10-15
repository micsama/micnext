import type { TraceCall, TraceMessage, TraceRun } from "../../api/trace";
import { formatDuration } from "../../lib/format";
import hljs from "highlight.js/lib/common";

const escape = (s: string) => s.replace(/[&<>]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;" })[c]!);
// 超过这么长的内容卡片默认折叠。
export const isLong = (text: string) => text.length > 600 || text.split("\n").length > 14;

/** 能解析为 JSON 的文本格式化并高亮，其余原样转义。 */
export function jsonHtml(text: string) {
  const t = text.trim();
  if (!/^[[{]/.test(t)) return escape(text);
  try {
    return hljs.highlight(JSON.stringify(JSON.parse(t), null, 2), { language: "json", ignoreIllegals: true }).value;
  } catch { return escape(text); }
}

// 耗时热度 0–1：同类样本 ≥5 时按中位数倍数，否则按固定阈值。
const SOFT = { model: 15_000, tool: 5_000, run: 30_000 };
const HARD = 60_000;
const clamp = (x: number) => Math.max(0, Math.min(1, x));
export function heat(ms: number, kind: keyof typeof SOFT, peer?: { median_ms: number; samples: number }) {
  const soft = SOFT[kind];
  const fixed = clamp(Math.log(ms / soft) / Math.log(HARD / soft));
  if (!peer || peer.samples < 5 || peer.median_ms <= 0) return { lv: fixed < .12 ? 0 : fixed, why: `固定阈值：${formatDuration(soft)} 起变色，${formatDuration(HARD)} 满红` };
  const ratio = ms / peer.median_ms;
  const lv = Math.max(clamp(Math.log2(ratio) / 2), ms >= HARD ? 1 : 0);
  return { lv: lv < .12 ? 0 : lv, why: `同类 ${peer.samples} 次中位数 ${formatDuration(peer.median_ms)}，本次 ${ratio.toFixed(1)}×` };
}

export const toolMs = (m: TraceMessage) =>
  m.tool && m.tool.started_at !== null && m.tool.outcome !== "cancelled" && m.tool.outcome !== "dispatched" ? m.created_at - m.tool.started_at : null;

export function attempts(run: TraceRun | null, call: TraceCall): TraceCall[] {
  if (!run) return [call];
  const ordered = [...run.calls].sort((a, b) => a.started_at - b.started_at || a.id - b.id);
  const index = ordered.findIndex((c) => c.id === call.id);
  let start = index;
  while (start > 0 && ordered[start]!.attempt > 1) start--;
  let end = index + 1;
  while (end < ordered.length && ordered[end]!.attempt > 1) end++;
  return ordered.slice(start, end);
}

export const steps = (run: TraceRun) => run.calls.filter((c) => attempts(run, c).at(-1)!.id === c.id);

// 执行器在下一次模型请求前收齐工具批次，结果属于其之前最近的回复。
export function owner(run: TraceRun, message: TraceMessage) {
  return run.calls.filter((c) => c.reply_id !== null && c.reply_id < message.id).sort((a, b) => b.reply_id! - a.reply_id!)[0];
}

export function toolsFor(run: TraceRun, call: TraceCall) {
  const last = attempts(run, call).at(-1)!;
  return run.messages.filter((m) => m.tool && owner(run, m)?.id === last.id).sort((a, b) => a.created_at - b.created_at || a.id - b.id);
}

export function stepOf(run: TraceRun | null, call: TraceCall) {
  const calls = attempts(run, call);
  const tools = run ? toolsFor(run, call) : [];
  const start = calls[0]!.started_at;
  const modelEnd = calls.at(-1)!.finished_at;
  const model = calls.reduce((n, c) => n + c.finished_at - c.started_at, 0);
  const knownTools = tools.filter((m) => toolMs(m) !== null);
  const unknown = knownTools.length < tools.length;
  const end = Math.max(modelEnd, ...knownTools.map((m) => m.created_at));
  const known = end - start;
  return { calls, tools, start, modelEnd, model, wait: modelEnd - start - model, known, unknown,
    end, span: Math.max(unknown ? known / .7 : known, 1),
    toolStart: knownTools.length ? Math.min(...knownTools.map((m) => m.tool!.started_at!)) : modelEnd,
  };
}

export function bar(run: TraceRun, from: number, to: number, call?: TraceCall): string {
  const step = call ? stepOf(run, call) : { start: from, span: Math.max(to - from, 1) };
  const left = Math.max(0, (from - step.start) / step.span * 100);
  return `left:${left}%;width:${Math.min(Math.max((to - from) / step.span * 100, .8), 100 - left)}%`;
}

export function usageOf(calls: TraceCall[]) {
  const known = calls.filter((c) => c.usage !== null);
  const sum = (key: "input_tokens" | "output_tokens" | "cache_read_tokens" | "cache_write_tokens" | "reasoning_tokens") => known.reduce((n, c) => n + (c.usage![key] ?? 0), 0);
  const priced = calls.filter((c) => c.cost !== null);
  return { input: sum("input_tokens"), output: sum("output_tokens"), cache: sum("cache_read_tokens"),
    cacheWrite: sum("cache_write_tokens"), reasoning: sum("reasoning_tokens"),
    reasoningKnown: known.some((c) => c.usage!.reasoning_tokens !== null),
    known: known.length, unknown: calls.length - known.length,
    cacheKnown: known.length > 0 && known.every((c) => c.usage!.cache_read_tokens !== null),
    cost: priced.length ? priced.reduce((n, c) => n + c.cost!, 0) : null,
    unknownCost: calls.length - priced.length,
  };
}

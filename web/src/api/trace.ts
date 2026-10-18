import { bool, arr, nullable, num, obj, oneOf, str, type Decoder } from "./decode";
import { runStats, type ContextKind, type ReasoningEffort, type RunStats } from "./types";

// 与 docs/blueprints/run-trace.md §六 及 mic-store `trace` 的 serde 形状一一对应。

export type Purpose = "conversation" | "compaction" | "title";
export type StopReason = "end_turn" | "tool_use" | "max_tokens" | "content_filter" | "stopped";
export type ToolOutcome = "completed" | "failed" | "cancelled" | "dispatched";

export type Usage = {
  input_tokens: number;
  output_tokens: number;
  cache_read_tokens: number | null;
  cache_write_tokens: number | null;
  reasoning_tokens: number | null;
};

export type TraceSession = {
  session: number;
  channel: string;
  title: string | null;
  last_run_at: number | null;
  failed_runs: number;
  interrupted_runs: number;
  tool_errors: number;
};

export type TraceCall = {
  id: number;
  purpose: Purpose;
  model_id: number | null;
  model: string;
  reasoning_effort: ReasoningEffort | null;
  started_at: number;
  first_output_at: number | null;
  finished_at: number;
  stop: StopReason | null;
  error: string | null;
  usage: Usage | null;
  cost: number | null;
  attempt: number;
  reply_id: number | null;
  boundary_id: number | null;
  /** 旁路调用的模型原文（`SideOutput` 消息）。 */
  side_id: number | null;
};

export type TraceTool = {
  name: string;
  call_id: string;
  started_at: number | null;
  outcome: ToolOutcome;
  error: string | null;
};

export type TraceMessage = {
  id: number;
  kind: string;
  created_at: number;
  preview: string;
  tool: TraceTool | null;
};

export type TraceRun = {
  id: number;
  state: string;
  created_at: number;
  finished_at: number | null;
  calls: TraceCall[];
  messages: TraceMessage[];
  stats: RunStats;
};

export type TraceRunPage = { runs: TraceRun[]; title_calls: TraceCall[]; next_after: number | null };

export type Peers = {
  models: { purpose: Purpose; model_id: number; median_ms: number; samples: number }[];
  tools: { name: string; median_ms: number; samples: number }[];
};

export type CallStat = {
  calls: number;
  failed_calls: number;
  input_tokens: number;
  output_tokens: number;
  cache_read_tokens: number;
  cache_write_tokens: number;
  unknown_usage: number;
  cost: number;
  unknown_cost: number;
  avg_ms: number | null;
};

export type DayStat = CallStat & { day: string; runs: number; completed_runs: number };
export type ModelStat = CallStat & { purpose: Purpose; model_id: number | null; model: string };
export type ToolStat = { name: string; calls: number; errors: number; avg_ms: number | null };
export type Dashboard = { days: DayStat[]; models: ModelStat[]; tools: ToolStat[] };

const purpose = oneOf<Purpose>("conversation", "compaction", "title");

const usage: Decoder<Usage> = obj({
  input_tokens: num,
  output_tokens: num,
  cache_read_tokens: nullable(num),
  cache_write_tokens: nullable(num),
  reasoning_tokens: nullable(num),
});

const traceSession: Decoder<TraceSession> = obj({
  session: num,
  channel: str,
  title: nullable(str),
  last_run_at: nullable(num),
  failed_runs: num,
  interrupted_runs: num,
  tool_errors: num,
});

const traceCall: Decoder<TraceCall> = obj({
  id: num,
  purpose,
  model_id: nullable(num),
  model: str,
  reasoning_effort: nullable(oneOf<ReasoningEffort>("none", "minimal", "low", "medium", "high", "xhigh", "max")),
  started_at: num,
  first_output_at: nullable(num),
  finished_at: num,
  stop: nullable(oneOf<StopReason>("end_turn", "tool_use", "max_tokens", "content_filter", "stopped")),
  error: nullable(str),
  usage: nullable(usage),
  cost: nullable(num),
  attempt: num,
  reply_id: nullable(num),
  boundary_id: nullable(num),
  side_id: nullable(num),
});

const traceTool: Decoder<TraceTool> = obj({
  name: str,
  call_id: str,
  started_at: nullable(num),
  outcome: oneOf<ToolOutcome>("completed", "failed", "cancelled", "dispatched"),
  error: nullable(str),
});

const traceMessage: Decoder<TraceMessage> = obj({
  id: num,
  kind: str,
  created_at: num,
  preview: str,
  tool: nullable(traceTool),
});

const traceRun: Decoder<TraceRun> = obj({
  id: num,
  state: str,
  created_at: num,
  finished_at: nullable(num),
  calls: arr(traceCall),
  messages: arr(traceMessage),
  stats: runStats,
});

const runPage: Decoder<TraceRunPage> = obj({
  runs: arr(traceRun),
  title_calls: arr(traceCall),
  next_after: nullable(num),
});

const peers: Decoder<Peers> = obj({
  models: arr(obj({ purpose, model_id: num, median_ms: num, samples: num })),
  tools: arr(obj({ name: str, median_ms: num, samples: num })),
});

const callStat = {
  calls: num,
  failed_calls: num,
  input_tokens: num,
  output_tokens: num,
  cache_read_tokens: num,
  cache_write_tokens: num,
  unknown_usage: num,
  cost: num,
  unknown_cost: num,
  avg_ms: nullable(num),
};

const dashboard: Decoder<Dashboard> = obj({
  days: arr(obj<DayStat>({ day: str, runs: num, completed_runs: num, ...callStat })),
  models: arr(obj<ModelStat>({ purpose, model_id: nullable(num), model: str, ...callStat })),
  tools: arr(obj({ name: str, calls: num, errors: num, avg_ms: nullable(num) })),
});

export const parseTraceSessions = (v: unknown): TraceSession[] => arr(traceSession)(v, "trace 会话");
export const parseTraceRunPage = (v: unknown): TraceRunPage => runPage(v, "trace 运行");
export type PromptPart = { kind: Exclude<ContextKind, "conversation">; text: string };
export type PromptBlock = { kind: "instructions" | "persona" | "context"; parts: PromptPart[] };
/** 当前配置下的系统提示：`text` 为发给模型的原文，`blocks` 为同一份结构。 */
export type SystemPromptView = { text: string; blocks: PromptBlock[] };
const promptPart: Decoder<PromptPart> = obj({ kind: oneOf("instructions", "persona", "tools", "environment", "summary"), text: str });
export const parseSystemPrompt = (v: unknown): SystemPromptView =>
  obj({ text: str, blocks: arr(obj({ kind: oneOf("instructions", "persona", "context"), parts: arr(promptPart) })) })(v, "系统提示词");
export const parsePeers = (v: unknown): Peers => peers(v, "同类耗时");
export const parseDashboard = (v: unknown): Dashboard => dashboard(v, "看板");


export type SessionUsage = {
  input_tokens: number; output_tokens: number; cache_read_tokens: number; cache_write_tokens: number;
  reasoning_tokens: number | null; known_usage: number; unknown_usage: number;
  usage_complete: boolean; cache_complete: boolean; cost: number | null; unknown_cost: number;
};
export const parseSessionUsage = (v: unknown): SessionUsage => obj({
  input_tokens: num, output_tokens: num, cache_read_tokens: num, cache_write_tokens: num,
  reasoning_tokens: nullable(num), known_usage: num, unknown_usage: num,
  usage_complete: bool, cache_complete: bool, cost: nullable(num), unknown_cost: num,
})(v, "会话用量");

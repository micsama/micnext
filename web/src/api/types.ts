import { any, arr, bool, kinded, nullable, num, obj, oneOf, ProtocolError, str, tagged, type Decoder } from "./decode";

export { ProtocolError };

// 与 gateway §4.2、§5.1 及 mic-message 的 serde 形状一一对应。

export type FileRef = { path: string; mime: string; size_bytes: number };

export type ImageRef = { id: number };

export type ContentPart = { Text: { text: string } } | { File: FileRef } | { Image: ImageRef };

/** 提交给服务端的消息片段；图片为 base64 原始字节。 */
export type InputPart = { kind: "text"; text: string } | { kind: "image"; base64: string };

/** 一条消息的图片限额；服务端仍是最终裁决。 */
export type InputLimits = { max_images: number; max_image_bytes: number };

export type ExecFailureKind = "Input" | "Business" | "Dependency";

export type ExecOutcome =
  | { Completed: { output: ContentPart[] } }
  | { Failed: { kind: ExecFailureKind; message: string } }
  | { Cancelled: { message: string } };

export type ToolResultOutcome = { Terminal: ExecOutcome } | { Dispatched: { exec_id: string } };

export type Reasoning =
  | { Visible: { text: string; signature: string | null } }
  | { Redacted: { data: string } };

export type ReplyBlock =
  | { Reasoning: Reasoning }
  | { Text: { text: string; phase: "commentary" | "final_answer" | null } }
  | { ToolCall: { id: string; name: string; args: unknown } };

export type FileSnapshot = { path: string; outcome: { Read: { text: string; extent: "Complete" | "Truncated" } } | { Unavailable: { reason: string } } };
export type Compaction = { summary: string; keep_from: number | null; files: FileSnapshot[]; elided: number[]; quoted: number[] };
export type ContextBoundary = { Compaction: Compaction } | "UserClear";
export type SessionCommand = { Compact: { text: string | null } } | "Clear" | { RevokeCompaction: { boundary_id: number } };
export type CompactionView = { boundary_id: number; result: Compaction; state: { kind: "applied" } | { kind: "revoked"; at: number } };
/** 上下文占用，口径同自动压缩判定，总含估算。 */
export type Occupancy = { used_tokens: number; window: number; trigger_percent: number };
export type ContextKind = "instructions" | "persona" | "tools" | "environment" | "summary" | "conversation";
export type ContextBreakdown = Occupancy & { parts: { kind: ContextKind; tokens: number; tool_tokens: number | null }[] };
export type JobState = "running" | "completed" | "failed" | "timed_out" | "cancelled" | "interrupted";
type JobBase = { id: number; tool_name: string; owner_session_id: number; description: string; created_at: number };
type ToolJobFields = { state: JobState; finished_at: number | null; timeout_ms: number | null; output_bytes: number; tail: string };
/** `running` 为假即休眠；`last_run` 为最近一个已结束子 run 的状态。 */
type AgentJobFields = { child_session_id: number; running: boolean; last_run: RunState | null; last_active_at: number };
export type ToolJobView = JobBase & { kind: "tool" } & ToolJobFields;
export type AgentJobView = JobBase & { kind: "agent" } & AgentJobFields;
export type JobView = ToolJobView | AgentJobView;
export type JobOutput = { text: string; offset: number; next_offset: number; eof: boolean; available_from: number; total_bytes: number };
export type StopJobOutcome = "stopped" | "already_idle" | "already_finished";
export type ContextView = { active_boundary: number | null; compactions: CompactionView[]; occupancy: Occupancy | null };
/** 一轮的统计；聊天统计行与 trace 运行概览共用（chat-run-stats §1.1）。 */
export type RunStats = {
  total_ms: number | null;
  model_ms: number;
  tool_ms: number;
  steps: number;
  requests: number;
  tools: number;
  first_output_ms: number | null;
  output_tps: number | null;
  input_tokens: number;
  output_tokens: number;
  usage_complete: boolean;
  cache_read_tokens: number;
  cache_complete: boolean;
  cost: number | null;
};
export type ChatRunStats = RunStats & { run_id: number; state: RunState; anchor_id: number };
export type CommandSpec = { name: string; usage: string; description: string };

export type MessageBody =
  | { kind: "Command"; person: number; command: SessionCommand }
  | { kind: "UserInput"; person: number; parts: ContentPart[] }
  | { kind: "Reply"; model: string; blocks: ReplyBlock[] }
  | { kind: "ToolResult"; tool_name: string; tool_call_id: string; outcome: ToolResultOutcome; started_at: number | null }
  | { kind: "Completion"; person: number; tool_name: string; exec_id: string; outcome: ExecOutcome }
  | { kind: "HarnessNote"; text: string }
  | { kind: "Notification"; source: string; text: string }
  | { kind: "Boundary"; boundary: ContextBoundary }
  /** 旁路调用的模型原文，只在 trace 出现。 */
  | { kind: "SideOutput"; purpose: "title"; text: string };

export type Message = {
  id: number;
  session_id: number;
  body: MessageBody;
  created_at: number;
  delivered_at: number | null;
};

export type SessionItem = {
  id: number;
  channel: string;
  created_at: number;
  last_activity_at: number;
  preview: string | null;
  title: string | null;
  title_error: string | null;
  workdir: string;
  /** 能否从 Web 发消息。 */
  writable: boolean;
  archived_at: number | null;
  /** 下一轮用的人设；已删除时下一轮改用默认人设。 */
  persona_id: number;
  /** 下一轮用的模型；null = 下次取当时的默认模型。 */
  model_id: number | null;
  reasoning_effort: ReasoningEffort | null;
};

export type Preset = "generic" | "deepseek" | "ollama" | "openai" | "chatgpt";
export type ReasoningEffort = "none" | "minimal" | "low" | "medium" | "high" | "xhigh" | "max";
export type ReasoningOptions = { choices: ReasoningEffort[]; default: ReasoningEffort };
export type SessionDetail = SessionItem & { title_generating: boolean };
export type TitleModelSelection = { model_id: number | null; reasoning_effort: ReasoningEffort | null };

export type Endpoint = {
  id: number;
  name: string;
  kind: string;
  /** DeepSeek 地址固定，服务端不返回。 */
  config: { preset: Preset; base_url: string | null };
  key_set: boolean;
  /** 未保存 key 时读取的环境变量名。 */
  key_env: string | null;
};

/** 单价，美元 / 百万 token；`null` = 未填。 */
export type ModelPrice = { input: number | null; cache_read: number | null; cache_write: number | null; output: number | null };

export type Model = {
  context_window: number | null;
  /** 关闭时请求里的图片换成文字占位。 */
  images: boolean;
  price: ModelPrice;
  id: number;
  endpoint_id: number;
  name: string;
  config: { max_tokens: number | null };
  reasoning: ReasoningOptions | null;
};

export type ModelList = { items: Model[]; default_model_id: number | null };

/** 提交的 API key 操作：沿用 / 清除 / 设置新值。 */
export type Credential = { op: "keep" } | { op: "clear" } | { op: "set"; value: string };

export type EndpointInput = {
  name: string;
  kind: string;
  config: { preset: Preset; base_url?: string };
  credential: Credential;
};

export type EndpointTestInput = Omit<EndpointInput, "name"> & { endpoint_id?: number };

export type ModelInput = {
  context_window: number | null;
  images: boolean;
  price: ModelPrice;
  endpoint_id: number;
  name: string;
  config: { max_tokens?: number };
};

export type Settings = {
  compaction: { trigger_percent: number; keep_percent: number };
  default_persona_id: number;
  general_prompt: string;
  default_workdir: string;
  max_turns: number;
  /** 系统块，只读。 */
  system_prompt: string;
};

export type SettingsInput = Omit<Settings, "system_prompt">;

export type Persona = { id: number; name: string; prompt: string; builtin: boolean };

export type Cursor = { before_at: number; before_id: number };

export type SessionPage = { items: SessionItem[]; next: Cursor | null };
export type MessagePage = { messages: Message[]; has_earlier: boolean };

export type Created = { session_id: number };

export type RunState = "executing" | "completed" | "provider_failed" | "max_turns" | "interrupted" | "stopped" | "timed_out";

export type LinkedChannel = {
  account_id: string;
  user_id: string;
  session_id: number;
  connection: "connected" | "needs_login" | "faulted";
};

export type SetupProgress =
  | { state: "preparing" | "expired" | "cancelled" }
  | { state: "waiting" | "scanned" | "needs_code"; qr_content: string }
  | { state: "connected"; account: LinkedChannel }
  | { state: "failed"; reason: "network" | "verification_blocked" | "existing_binding" | "protocol" };

export type SetupAttempt = { id: string; progress: SetupProgress };
export type WechatView =
  | { available: false }
  | { available: true; account: LinkedChannel | null; login: SetupAttempt | null };

const linkedChannel: Decoder<LinkedChannel> = obj({
  account_id: str, user_id: str, session_id: num,
  connection: oneOf("connected", "needs_login", "faulted"),
});

const setupProgress: Decoder<SetupProgress> = (v, at) => {
  if (typeof v !== "object" || v === null || !("state" in v)) throw new ProtocolError(at);
  switch (v.state) {
    case "preparing": case "expired": case "cancelled":
      return obj({ state: oneOf("preparing", "expired", "cancelled") })(v, at);
    case "waiting": case "scanned": case "needs_code":
      return obj({ state: oneOf("waiting", "scanned", "needs_code"), qr_content: str })(v, at);
    case "connected":
      return obj({ state: oneOf("connected"), account: linkedChannel })(v, at);
    case "failed":
      return obj({ state: oneOf("failed"), reason: oneOf("network", "verification_blocked", "existing_binding", "protocol") })(v, at);
    default: throw new ProtocolError(at);
  }
};

const setupAttempt: Decoder<SetupAttempt> = obj({ id: str, progress: setupProgress });
export const parseSetupAttempt = (v: unknown): SetupAttempt => setupAttempt(v, "微信登录");
export const parseWechatView = (v: unknown): WechatView => {
  if (typeof v !== "object" || v === null || !("available" in v)) throw new ProtocolError("微信状态");
  if (v.available === false) {
    obj({ available: bool })(v, "微信状态");
    return { available: false };
  }
  const result = obj({ available: bool, account: nullable(linkedChannel), login: nullable(setupAttempt) })(v, "微信状态");
  if (result.available !== true) throw new ProtocolError("微信状态.available");
  return { ...result, available: true };
};

export type StreamEvent =
  | { event: "session_title_changed"; data: Record<string, never> }
  | { event: "jobs_changed"; data: Record<string, never> }
  | { event: "message"; data: Message }
  | { event: "text_delta"; data: { text: string } }
  | { event: "reasoning_delta"; data: { text: string } }
  | { event: "draft_discarded"; data: Record<string, never> }
  | { event: "run_started"; data: { run_id: number } }
  | { event: "run_finished"; data: { run_id: number; state: RunState } }
  | { event: "ready"; data: { executing_run: number | null; has_earlier: boolean | null } };

const fileRef: Decoder<FileRef> = obj({ path: str, mime: str, size_bytes: num });

const contentPart: Decoder<ContentPart> = tagged({
  Text: obj({ text: str }),
  File: fileRef,
  Image: obj({ id: num }),
});

const execOutcome: Decoder<ExecOutcome> = tagged({
  Completed: obj({ output: arr(contentPart) }),
  Failed: obj({ kind: oneOf("Input", "Business", "Dependency"), message: str }),
  Cancelled: obj({ message: str }),
});

const reasoning: Decoder<Reasoning> = tagged({
  Visible: obj({ text: str, signature: nullable(str) }),
  Redacted: obj({ data: str }),
});

const replyBlock: Decoder<ReplyBlock> = tagged({
  Reasoning: reasoning,
  Text: obj({ text: str, phase: nullable(oneOf("commentary", "final_answer")) }),
  ToolCall: obj({ id: str, name: str, args: any }),
});

const fileSnapshot: Decoder<FileSnapshot> = obj({ path: str, outcome: tagged({ Read: obj({ text: str, extent: oneOf("Complete", "Truncated") }), Unavailable: obj({ reason: str }) }) });
const compactionResult: Decoder<Compaction> = obj({ summary: str, keep_from: nullable(num), files: arr(fileSnapshot), elided: arr(num), quoted: arr(num) });
const compaction = tagged({ Compaction: compactionResult });
const command: Decoder<SessionCommand> = (v, at) => v === "Clear" ? v : tagged({ Compact: obj({ text: nullable(str) }), RevokeCompaction: obj({ boundary_id: num }) })(v, at);
const compactionState: Decoder<CompactionView["state"]> = kinded<{ applied: Record<string, never>; revoked: { at: number } }>({ applied: {}, revoked: { at: num } });
const runState: Decoder<RunState> = oneOf("executing", "completed", "provider_failed", "max_turns", "interrupted", "stopped", "timed_out");
const occupancy: Decoder<Occupancy> = obj({ used_tokens: num, window: num, trigger_percent: num });
const contextView: Decoder<ContextView> = obj({ active_boundary: nullable(num), compactions: arr(obj({ boundary_id: num, result: compactionResult, state: compactionState })), occupancy: nullable(occupancy) });
const runStatsFields = {
  total_ms: nullable(num), model_ms: num, tool_ms: num, steps: num, requests: num, tools: num,
  first_output_ms: nullable(num), output_tps: nullable(num), input_tokens: num, output_tokens: num,
  usage_complete: bool, cache_read_tokens: num, cache_complete: bool, cost: nullable(num),
};
export const runStats: Decoder<RunStats> = obj(runStatsFields);
const chatRunStats: Decoder<ChatRunStats> = obj({ ...runStatsFields, run_id: num, state: runState, anchor_id: num });
export const parseRunStats = (v: unknown): ChatRunStats[] => obj({ runs: arr(chatRunStats) })(v, "统计").runs;
const jobState: Decoder<JobState> = oneOf("running", "completed", "failed", "timed_out", "cancelled", "interrupted");
const jobBase = { id: num, tool_name: str, owner_session_id: num, description: str, created_at: num };
const jobView: Decoder<JobView> = kinded<{ tool: JobBase & ToolJobFields; agent: JobBase & AgentJobFields }>({
  tool: { ...jobBase, state: jobState, finished_at: nullable(num), timeout_ms: nullable(num), output_bytes: num, tail: str },
  agent: { ...jobBase, child_session_id: num, running: bool, last_run: nullable(runState), last_active_at: num },
});
/** 任务是否仍在活动：tool job 运行中，或 agent 有执行中的 run。 */
export const jobActive = (j: JobView): boolean => (j.kind === "tool" ? j.state === "running" : j.running);
const jobOutput: Decoder<JobOutput> = obj({ text: str, offset: num, next_offset: num, eof: bool, available_from: num, total_bytes: num });
export const parseJobs = (v: unknown): JobView[] => arr(jobView)(v, "任务列表");
export const parseJobOutput = (v: unknown): JobOutput => jobOutput(v, "任务输出");
export const parseStopJob = (v: unknown): StopJobOutcome => obj({ outcome: oneOf("stopped", "already_idle", "already_finished") })(v, "终止任务").outcome;
export const parseContextView = (v: unknown): ContextView => contextView(v, "上下文");
const contextKind: Decoder<ContextKind> = oneOf("instructions", "persona", "tools", "environment", "summary", "conversation");
const contextBreakdown: Decoder<ContextBreakdown> = obj({ used_tokens: num, window: num, trigger_percent: num, parts: arr(obj({ kind: contextKind, tokens: num, tool_tokens: nullable(num) })) });
export const parseContextBreakdown = (v: unknown): ContextBreakdown | null => nullable(contextBreakdown)(v, "上下文构成");
export const parseCommands = (v: unknown): CommandSpec[] => obj({ items: arr(obj({ name: str, usage: str, description: str })) })(v, "指令列表").items;
const boundary: Decoder<ContextBoundary> = (v, at) => (v === "UserClear" ? v : compaction(v, at));

const messageBody: Decoder<MessageBody> = kinded({
  Command: { person: num, command },
  UserInput: { person: num, parts: arr(contentPart) },
  Reply: { model: str, blocks: arr(replyBlock) },
  ToolResult: {
    tool_name: str,
    tool_call_id: str,
    outcome: tagged({ Terminal: execOutcome, Dispatched: obj({ exec_id: str }) }),
    started_at: nullable(num),
  },
  Completion: { person: num, tool_name: str, exec_id: str, outcome: execOutcome },
  HarnessNote: { text: str },
  Notification: { source: str, text: str },
  Boundary: { boundary },
  SideOutput: { purpose: oneOf<"title">("title"), text: str },
});

const message: Decoder<Message> = obj({
  id: num,
  session_id: num,
  body: messageBody,
  created_at: num,
  delivered_at: nullable(num),
});

const messagePage: Decoder<MessagePage> = obj({ messages: arr(message), has_earlier: bool });

const reasoningEffort = oneOf("none", "minimal", "low", "medium", "high", "xhigh", "max");

const sessionFields = {
  id: num,
  channel: str,
  created_at: num,
  last_activity_at: num,
  preview: nullable(str),
  title: nullable(str),
  title_error: nullable(str),
  workdir: str,
  writable: bool,
  archived_at: nullable(num),
  persona_id: num,
  model_id: nullable(num),
  reasoning_effort: nullable(reasoningEffort),
};
const sessionItem: Decoder<SessionItem> = obj(sessionFields);
const sessionDetail: Decoder<SessionDetail> = obj({ ...sessionFields, title_generating: bool });
const titleModel: Decoder<TitleModelSelection> = obj({ model_id: nullable(num), reasoning_effort: nullable(reasoningEffort) });

const preset = oneOf("generic", "deepseek", "ollama", "openai", "chatgpt");

const endpointList: Decoder<{ items: Endpoint[] }> = obj({
  items: arr(
    obj({
      id: num,
      name: str,
      kind: str,
      config: obj({ preset, base_url: nullable(str) }),
      key_set: bool,
      key_env: nullable(str),
    }),
  ),
});

const modelList: Decoder<ModelList> = obj({
  items: arr(
    obj({
      id: num,
      endpoint_id: num,
      name: str,
      context_window: nullable(num),
      images: bool,
      price: obj({ input: nullable(num), cache_read: nullable(num), cache_write: nullable(num), output: nullable(num) }),
      config: obj({
        max_tokens: nullable(num),
      }),
      reasoning: nullable(obj({ choices: arr(reasoningEffort), default: reasoningEffort })),
    }),
  ),
  default_model_id: nullable(num),
});

const kindList: Decoder<{ items: { kind: string; display_name: string }[]; input_limits: InputLimits }> = obj({
  items: arr(obj({ kind: str, display_name: str })),
  input_limits: obj({ max_images: num, max_image_bytes: num }),
});

const testResult: Decoder<{ models: string[] }> = obj({ models: arr(str) });

const settings: Decoder<Settings> = obj({
  compaction: obj({ trigger_percent: num, keep_percent: num }),
  default_persona_id: num,
  general_prompt: str,
  default_workdir: str,
  max_turns: num,
  system_prompt: str,
});

const personaList = obj({ items: arr(obj({ id: num, name: str, prompt: str, builtin: bool })) });

const idOnly = obj({ id: num });

const sessionPage: Decoder<SessionPage> = obj({
  items: arr(sessionItem),
  next: nullable(obj({ before_at: num, before_id: num })),
});

const created: Decoder<Created> = obj({ session_id: num });

const delta = obj({ text: str });

const streamData: { [E in StreamEvent["event"]]: Decoder<Extract<StreamEvent, { event: E }>["data"]> } = {
  message,
  text_delta: delta,
  reasoning_delta: delta,
  draft_discarded: obj({}),
  session_title_changed: obj({}),
  jobs_changed: obj({}),
  run_started: obj({ run_id: num }),
  run_finished: obj({
    run_id: num,
    state: runState,
  }),
  ready: obj({ executing_run: nullable(num), has_earlier: nullable(bool) }),
};

export const parseInputLimits = (v: unknown): InputLimits => kindList(v, "输入限额").input_limits;

export const parseSessionItem = (v: unknown): SessionItem => sessionItem(v, "会话");
export const parseSessionDetail = (v: unknown): SessionDetail => sessionDetail(v, "会话详情");
export const parseTitleModel = (v: unknown): TitleModelSelection => titleModel(v, "标题模型");

export const parseSessionPage = (v: unknown): SessionPage => sessionPage(v, "会话列表");

export const parseMessage = (v: unknown): Message => message(v, "消息");
export const parseMessages = (v: unknown): Message[] => arr(message)(v, "消息列表");
export const parseMessagePage = (v: unknown): MessagePage => messagePage(v, "历史消息");

export const parseCreated = (v: unknown): Created => created(v, "新建会话");

export const parseSettings = (v: unknown): Settings => settings(v, "设置");

export const parsePersonas = (v: unknown): Persona[] => personaList(v, "人设列表").items;

export const parseEndpoints = (v: unknown): Endpoint[] => endpointList(v, "服务商列表").items;
export const parseTestResult = (v: unknown): string[] => testResult(v, "测试结果").models;
export const parseModels = (v: unknown): ModelList => modelList(v, "模型列表");

export const parseId = (v: unknown): number => idOnly(v, "新建条目").id;

export const streamEvents = Object.keys(streamData) as readonly StreamEvent["event"][];

export function parseStreamEvent(event: string, data: string): StreamEvent {
  if (!Object.hasOwn(streamData, event)) throw new ProtocolError(`事件 ${event}`);
  let parsed: unknown;
  try {
    parsed = JSON.parse(data);
  } catch {
    throw new ProtocolError(`事件 ${event}`);
  }
  const decode = streamData[event as StreamEvent["event"]] as Decoder<unknown>;
  return { event, data: decode(parsed, `事件 ${event}`) } as StreamEvent;
}

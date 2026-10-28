import { any, arr, bool, kinded, nullable, num, obj, oneOf, ProtocolError, str, tagged, type Decoder } from "./decode";

export { ProtocolError };

// 与 gateway §4.2、§5.1 及 mic-message 的 serde 形状一一对应。

export type FileRef = { path: string; size_bytes: number };

export type ImageRef = { id: number };

export type ContentPart = { Text: { text: string } } | { File: FileRef } | { Image: ImageRef } | { Skill: { name: string; text: string } };

/** 提交给服务端的消息片段；图片为 base64 原始字节。 */
export type InputPart = { kind: "text"; text: string } | { kind: "image"; base64: string } | { kind: "file"; name: string; base64: string };

/** 一条消息的图片限额；服务端仍是最终裁决。 */
export type InputLimits = { max_images: number; max_image_bytes: number; max_files: number; max_file_bytes: number };

export type ExecFailureKind = "Input" | "Business" | "Dependency";

export type ExecOutcome =
  | { Completed: { output: ContentPart[] } }
  | { Failed: { kind: ExecFailureKind; message: string } }
  | { Cancelled: { message: string } }
  | { TimedOut: { output: ContentPart[] } };

export type ToolCallRef = { reply: number; index: number };
export const toolCallRef = obj({ reply: num, index: num });

export type ToolResultOutcome = { Terminal: ExecOutcome } | { Dispatched: { exec_id: string } };

export type Reasoning =
  | { Visible: { text: string; signature: string | null } }
  | { Redacted: { data: string } };

export type ReplyBlock =
  | { Reasoning: Reasoning }
  | { Text: { text: string; phase: "commentary" | "final_answer" | null } }
  | { ToolCall: { id: string; name: string; args: unknown } };

export type Compaction = { summary: string; keep_from: number | null; files: string[]; elided: number[]; quoted: number[] };
export type Trim = { base: number | null; elided: number[] };
export type ContextBoundary = { Compaction: Compaction } | { Trim: Trim } | "UserClear";
export type SessionCommand = { Compact: { text: string | null } } | "Clear" | { RevokeCompaction: { boundary_id: number } };
/** 可撤回的边界：压缩与冷时整理。 */
export type RevocableView = { boundary_id: number; state: { kind: "applied" } | { kind: "revoked"; at: number } };
/** 上下文占用，口径同自动压缩判定，总含估算。 */
export type Occupancy = { used_tokens: number; window: number; trigger_percent: number };
export type ContextKind = "instructions" | "persona" | "tools" | "environment" | "skills" | "summary" | "conversation";
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
export type ContextView = { active_boundary: number | null; revocable: RevocableView[]; occupancy: Occupancy | null };
/** Trace 执行合计。 */
export type RunStats = {
  total_ms: number | null;
  model_ms: number;
  tool_ms: number;
  steps: number;
  requests: number;
  tools: number;
};
export type ReplyUsage = { input_tokens: number; output_tokens: number; cache_read_tokens: number | null };
export type ReplyStats = { model_ms: number; first_output_ms: number | null; usage: ReplyUsage | null };
export type CommandSpec = { name: string; usage: string; description: string };
/** 数据目录下的 skill；不合法的只有目录名与原因。 */
export type SkillList = { skills: { name: string; description: string }[]; invalid: { dir: string; reason: string }[] };

export type MessageBody =
  | { kind: "Command"; person: number; command: SessionCommand }
  | { kind: "UserInput"; person: number; parts: ContentPart[] }
  | { kind: "Reply"; model: string; blocks: ReplyBlock[]; stats: ReplyStats }
  | { kind: "ToolResult"; call: ToolCallRef; tool_name: string; tool_call_id: string; outcome: ToolResultOutcome; started_at: number | null }
  | { kind: "Completion"; person: number; tool_name: string; exec_id: string; outcome: ExecOutcome }
  /** 运行中的后台任务给模型的中途通知，不投递给用户。 */
  | { kind: "JobNotice"; person: number; tool_name: string; exec_id: string; text: string }
  /** 定时任务到点发给 bot 的指令。 */
  | { kind: "Trigger"; source: string; text: string }
  | { kind: "HarnessNote"; text: string }
  | { kind: "Notification"; source: string; text: string; files: FileRef[] }
  /** 模型用 send_file 交给用户的文件快照。 */
  | { kind: "Attachment"; files: FileRef[] }
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

export type SessionSource = { kind: "channel"; channel: string } | { kind: "schedule" };

export type SessionItem = {
  schedule: { id: number; name: string } | null;
  id: number;
  source: SessionSource;
  created_at: number;
  last_activity_at: number;
  preview: string | null;
  title: string | null;
  title_error: string | null;
  workdir: string;
  /** 能否从 Web 发消息。 */
  writable: boolean;
  archived_at: number | null;
  /** 下次执行用的人设；已删除时改用默认人设。 */
  persona_id: number;
  /** 下次执行用的模型；null = 下次取当时的默认模型。 */
  model_id: number | null;
  reasoning_effort: ReasoningEffort | null;
  /** 临时明文：开启后的新消息发给模型时不按规则脱敏。 */
  plaintext: boolean;
  pinned_at: number | null;
  /** Web 会话有未看过的回复或通知。 */
  unread: boolean;
  running: boolean;
  /** id 升序；名字查 `/tags` 词表。 */
  tag_ids: number[];
};

export type Tag = { id: number; name: string; sessions: number };

export type Preset = "generic" | "deepseek" | "ollama" | "openai" | "chatgpt";
export type ReasoningEffort = "none" | "minimal" | "low" | "medium" | "high" | "xhigh" | "max";
export type ReasoningOptions = { choices: ReasoningEffort[]; default: ReasoningEffort };
export type SessionDetail = SessionItem & { title_generating: boolean };
export type UtilityModelSelection = { model_id: number | null; reasoning_effort: ReasoningEffort | null };

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
  /** 收藏后模型选择器里该服务商只列收藏的模型。 */
  starred: boolean;
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
  starred: boolean;
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

/** 搜索命中的一条消息：`snippet` 已截在命中词附近。 */
export type SearchHit = { message_id: number; created_at: number; from_user: boolean; snippet: string };
export type SessionHit = { session: SessionItem; title_hit: boolean; hits: SearchHit[]; hit_count: number };
export type SearchPage = { items: SessionHit[]; next: Cursor | null };
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
  | { event: "session_changed"; data: Record<string, never> }
  | { event: "jobs_changed"; data: Record<string, never> }
  | { event: "message"; data: Message }
  | { event: "text_delta"; data: { text: string } }
  | { event: "reasoning_delta"; data: { text: string } }
  | { event: "model_retry"; data: { attempt: number; delay_ms: number } }
  | { event: "draft_discarded"; data: Record<string, never> }
  | { event: "inputs_claimed"; data: { run_id: number; inputs: number[] } }
  | { event: "run_finished"; data: { run_id: number; state: RunState; resume: Resumable | null } }
  | { event: "ready"; data: { executing_run: number | null; queued: number[]; has_earlier: boolean | null; resume: Resumable | null } };

/** 可原地接续的意外结束 run。 */
export type Resumable = { run_id: number; state: RunState };

export const fileRef: Decoder<FileRef> = obj({ path: str, size_bytes: num });

const contentPart: Decoder<ContentPart> = tagged({
  Text: obj({ text: str }),
  File: fileRef,
  Image: obj({ id: num }),
  Skill: obj({ name: str, text: str }),
});

const execOutcome: Decoder<ExecOutcome> = tagged({
  Completed: obj({ output: arr(contentPart) }),
  Failed: obj({ kind: oneOf("Input", "Business", "Dependency"), message: str }),
  Cancelled: obj({ message: str }),
  TimedOut: obj({ output: arr(contentPart) }),
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

const compactionResult: Decoder<Compaction> = obj({ summary: str, keep_from: nullable(num), files: arr(str), elided: arr(num), quoted: arr(num) });
const compaction = tagged({ Compaction: compactionResult, Trim: obj({ base: nullable(num), elided: arr(num) }) });
const command: Decoder<SessionCommand> = (v, at) => v === "Clear" ? v : tagged({ Compact: obj({ text: nullable(str) }), RevokeCompaction: obj({ boundary_id: num }) })(v, at);
const revocationState: Decoder<RevocableView["state"]> = kinded<{ applied: Record<string, never>; revoked: { at: number } }>({ applied: {}, revoked: { at: num } });
const runState: Decoder<RunState> = oneOf("executing", "completed", "provider_failed", "max_turns", "interrupted", "stopped", "timed_out");
const occupancy: Decoder<Occupancy> = obj({ used_tokens: num, window: num, trigger_percent: num });
const contextView: Decoder<ContextView> = obj({ active_boundary: nullable(num), revocable: arr(obj({ boundary_id: num, state: revocationState })), occupancy: nullable(occupancy) });
const runStatsFields = {
  total_ms: nullable(num), model_ms: num, tool_ms: num, steps: num, requests: num, tools: num,
};
export const runStats: Decoder<RunStats> = obj(runStatsFields);
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
const contextKind: Decoder<ContextKind> = oneOf("instructions", "persona", "tools", "environment", "skills", "summary", "conversation");
const contextBreakdown: Decoder<ContextBreakdown> = obj({ used_tokens: num, window: num, trigger_percent: num, parts: arr(obj({ kind: contextKind, tokens: num, tool_tokens: nullable(num) })) });
export const parseContextBreakdown = (v: unknown): ContextBreakdown | null => nullable(contextBreakdown)(v, "上下文构成");
export const parseCommands = (v: unknown): CommandSpec[] => obj({ items: arr(obj({ name: str, usage: str, description: str })) })(v, "指令列表").items;
export const parseSkills = (v: unknown): SkillList => obj({ skills: arr(obj({ name: str, description: str })), invalid: arr(obj({ dir: str, reason: str })) })(v, "skill 列表");
const boundary: Decoder<ContextBoundary> = (v, at) => (v === "UserClear" ? v : compaction(v, at));

const messageBody: Decoder<MessageBody> = kinded({
  Command: { person: num, command },
  UserInput: { person: num, parts: arr(contentPart) },
  Reply: { model: str, blocks: arr(replyBlock), stats: obj({ model_ms: num, first_output_ms: nullable(num), usage: nullable(obj({ input_tokens: num, output_tokens: num, cache_read_tokens: nullable(num) })) }) },
  ToolResult: {
    call: toolCallRef,
    tool_name: str,
    tool_call_id: str,
    outcome: tagged({ Terminal: execOutcome, Dispatched: obj({ exec_id: str }) }),
    started_at: nullable(num),
  },
  Completion: { person: num, tool_name: str, exec_id: str, outcome: execOutcome },
  JobNotice: { person: num, tool_name: str, exec_id: str, text: str },
  Trigger: { source: str, text: str },
  HarnessNote: { text: str },
  Notification: { source: str, text: str, files: arr(fileRef) },
  Attachment: { files: arr(fileRef) },
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
  schedule: nullable(obj({ id: num, name: str })),
  id: num,
  source: kinded<{ channel: { channel: string }; schedule: Record<never, never> }>({ channel: { channel: str }, schedule: {} }),
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
  plaintext: bool,
  pinned_at: nullable(num),
  unread: bool,
  running: bool,
  tag_ids: arr(num),
};
const sessionItem: Decoder<SessionItem> = obj(sessionFields);
const sessionDetail: Decoder<SessionDetail> = obj({ ...sessionFields, title_generating: bool });
const utilityModel: Decoder<UtilityModelSelection> = obj({ model_id: nullable(num), reasoning_effort: nullable(reasoningEffort) });

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
      starred: bool,
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
  input_limits: obj({ max_images: num, max_image_bytes: num, max_files: num, max_file_bytes: num }),
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

const pinnedSessions = obj({ items: arr(sessionItem) });

const searchPage: Decoder<SearchPage> = obj({
  items: arr(obj({
    session: sessionItem,
    title_hit: bool,
    hits: arr(obj({ message_id: num, created_at: num, from_user: bool, snippet: str })),
    hit_count: num,
  })),
  next: nullable(obj({ before_at: num, before_id: num })),
});

const created: Decoder<Created> = obj({ session_id: num });

const delta = obj({ text: str });

const resumable: Decoder<Resumable> = obj({ run_id: num, state: runState });
const streamData: { [E in StreamEvent["event"]]: Decoder<Extract<StreamEvent, { event: E }>["data"]> } = {
  message,
  text_delta: delta,
  reasoning_delta: delta,
  model_retry: obj({ attempt: num, delay_ms: num }),
  draft_discarded: obj({}),
  session_changed: obj({}),
  jobs_changed: obj({}),
  inputs_claimed: obj({ run_id: num, inputs: arr(num) }),
  run_finished: obj({
    run_id: num,
    state: runState,
    resume: nullable(resumable),
  }),
  ready: obj({ executing_run: nullable(num), queued: arr(num), has_earlier: nullable(bool), resume: nullable(resumable) }),
};

export const parseInputLimits = (v: unknown): InputLimits => kindList(v, "输入限额").input_limits;

export const parseSessionItem = (v: unknown): SessionItem => sessionItem(v, "会话");
export const parseSessionDetail = (v: unknown): SessionDetail => sessionDetail(v, "会话详情");
export const parseUtilityModel = (v: unknown): UtilityModelSelection => utilityModel(v, "杂活模型");

export const parseSessionPage = (v: unknown): SessionPage => sessionPage(v, "会话列表");
export const parseSearchPage = (v: unknown): SearchPage => searchPage(v, "搜索结果");
export const parseTags = (v: unknown): Tag[] => arr(obj({ id: num, name: str, sessions: num }))(v, "标签");
export const parseCreatedTag = (v: unknown): number => obj({ id: num })(v, "新建标签").id;
export const parsePinnedSessions = (v: unknown): SessionItem[] => pinnedSessions(v, "置顶会话").items;

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

/** `ask_user` 调用入参；只解码已成功的调用，入参已由工具校验。 */
export type AskQuestion = { question: string; header: string; options: { label: string; description: string }[]; multi_select: boolean };
const askArgs = obj<{ questions: AskQuestion[] }>({
  questions: arr(obj<AskQuestion>({ question: str, header: str, options: arr(obj({ label: str, description: str })), multi_select: bool })),
});
export const parseAskArgs = (v: unknown): AskQuestion[] => askArgs(v, "ask_user").questions;

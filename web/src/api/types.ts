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

export type ContextBoundary = { Compaction: { summary: string } } | "UserClear";

export type MessageBody =
  | { kind: "UserInput"; person: number; parts: ContentPart[] }
  | { kind: "Reply"; model: string; blocks: ReplyBlock[] }
  | { kind: "ToolResult"; tool_name: string; tool_call_id: string; outcome: ToolResultOutcome }
  | { kind: "Completion"; person: number; tool_name: string; exec_id: string; outcome: ExecOutcome }
  | { kind: "HarnessNote"; text: string }
  | { kind: "Notification"; source: string; text: string }
  | { kind: "Boundary"; boundary: ContextBoundary };

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
  workdir: string;
  /** 能否从 Web 发消息。 */
  writable: boolean;
  /** 下一轮用的人设；已删除时下一轮改用默认人设。 */
  persona_id: number;
  /** 下一轮用的模型；null = 下次取当时的默认模型。 */
  model_id: number | null;
};

export type Preset = "generic" | "deepseek" | "ollama" | "openai";
export type ReasoningEffort = "none" | "minimal" | "low" | "medium" | "high" | "xhigh" | "max";

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

export type Model = {
  id: number;
  endpoint_id: number;
  name: string;
  config: { max_tokens: number | null; reasoning_effort: ReasoningEffort | null };
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
  endpoint_id: number;
  name: string;
  config: { max_tokens?: number; reasoning_effort?: string };
};

export type Settings = {
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

export type Created = { session_id: number; message_id: number };

export type RunState = "executing" | "completed" | "provider_failed" | "max_turns" | "interrupted";

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
  | { event: "message"; data: Message }
  | { event: "text_delta"; data: { text: string } }
  | { event: "reasoning_delta"; data: { text: string } }
  | { event: "draft_discarded"; data: Record<string, never> }
  | { event: "run_started"; data: { run_id: number } }
  | { event: "run_finished"; data: { run_id: number; state: RunState } }
  | { event: "ready"; data: { executing_run: number | null } };

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

const compaction = tagged({ Compaction: obj({ summary: str }) });
const boundary: Decoder<ContextBoundary> = (v, at) => (v === "UserClear" ? v : compaction(v, at));

const messageBody: Decoder<MessageBody> = kinded({
  UserInput: { person: num, parts: arr(contentPart) },
  Reply: { model: str, blocks: arr(replyBlock) },
  ToolResult: {
    tool_name: str,
    tool_call_id: str,
    outcome: tagged({ Terminal: execOutcome, Dispatched: obj({ exec_id: str }) }),
  },
  Completion: { person: num, tool_name: str, exec_id: str, outcome: execOutcome },
  HarnessNote: { text: str },
  Notification: { source: str, text: str },
  Boundary: { boundary },
});

const message: Decoder<Message> = obj({
  id: num,
  session_id: num,
  body: messageBody,
  created_at: num,
  delivered_at: nullable(num),
});

const sessionItem: Decoder<SessionItem> = obj({
  id: num,
  channel: str,
  created_at: num,
  last_activity_at: num,
  preview: nullable(str),
  workdir: str,
  writable: bool,
  persona_id: num,
  model_id: nullable(num),
});

const preset = oneOf("generic", "deepseek", "ollama", "openai");

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
      config: obj({
        max_tokens: nullable(num),
        reasoning_effort: nullable(oneOf("none", "minimal", "low", "medium", "high", "xhigh", "max")),
      }),
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

const created: Decoder<Created> = obj({ session_id: num, message_id: num });

const delta = obj({ text: str });

const streamData: { [E in StreamEvent["event"]]: Decoder<Extract<StreamEvent, { event: E }>["data"]> } = {
  message,
  text_delta: delta,
  reasoning_delta: delta,
  draft_discarded: obj({}),
  run_started: obj({ run_id: num }),
  run_finished: obj({
    run_id: num,
    state: oneOf("executing", "completed", "provider_failed", "max_turns", "interrupted"),
  }),
  ready: obj({ executing_run: nullable(num) }),
};

export const parseInputLimits = (v: unknown): InputLimits => kindList(v, "输入限额").input_limits;

export const parseSessionItem = (v: unknown): SessionItem => sessionItem(v, "会话");

export const parseSessionPage = (v: unknown): SessionPage => sessionPage(v, "会话列表");

export const parseCreated = (v: unknown): Created => created(v, "新建会话");

export const parseSettings = (v: unknown): Settings => settings(v, "设置");

export const parsePersonas = (v: unknown): Persona[] => personaList(v, "人设列表").items;

export const parseEndpoints = (v: unknown): Endpoint[] => endpointList(v, "服务商列表").items;
export const parseTestResult = (v: unknown): string[] => testResult(v, "测试结果").models;
export const parseModels = (v: unknown): ModelList => modelList(v, "模型列表");

export const parseId = (v: unknown): number => idOnly(v, "新建条目").id;

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

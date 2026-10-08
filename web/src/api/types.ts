import { any, arr, bool, kinded, nullable, num, obj, oneOf, ProtocolError, str, tagged, type Decoder } from "./decode";

export { ProtocolError };

// 与 gateway §4.2、§5.1 及 mic-message 的 serde 形状一一对应。

export type FileRef = { path: string; mime: string; size_bytes: number };

export type ContentPart = { Text: { text: string } } | { File: FileRef };

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
  | { Text: { text: string } }
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

export type StreamEvent =
  | { event: "message"; data: Message }
  | { event: "text_delta"; data: { text: string } }
  | { event: "reasoning_delta"; data: { text: string } }
  | { event: "draft_discarded"; data: Record<string, never> }
  | { event: "run_started"; data: { run_id: number } }
  | { event: "run_finished"; data: { run_id: number; state: RunState } }
  | { event: "ready"; data: { executing_run: number | null } };

const fileRef: Decoder<FileRef> = obj({ path: str, mime: str, size_bytes: num });

const contentPart: Decoder<ContentPart> = tagged({ Text: obj({ text: str }), File: fileRef });

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
  Text: obj({ text: str }),
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
});

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

export const parseSessionItem = (v: unknown): SessionItem => sessionItem(v, "会话");

export const parseSessionPage = (v: unknown): SessionPage => sessionPage(v, "会话列表");

export const parseCreated = (v: unknown): Created => created(v, "新建会话");

export const parseSettings = (v: unknown): Settings => settings(v, "设置");

export const parsePersonas = (v: unknown): Persona[] => personaList(v, "人设列表").items;

export const parsePersonaId = (v: unknown): number => idOnly(v, "新建人设").id;

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

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
};

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

const BODY_KINDS = new Set([
  "UserInput",
  "Reply",
  "ToolResult",
  "Completion",
  "HarnessNote",
  "Notification",
  "Boundary",
]);
const EVENTS = new Set([
  "message",
  "text_delta",
  "reasoning_delta",
  "draft_discarded",
  "run_started",
  "run_finished",
  "ready",
]);

/** 服务端返回的形状与本界面不符：前后端版本不一致。 */
export class ProtocolError extends Error {
  constructor(what: string) {
    super(`界面与服务版本不一致，请刷新页面（${what}）`);
  }
}

const isObject = (v: unknown): v is Record<string, unknown> => typeof v === "object" && v !== null;

function assertMessage(v: unknown): asserts v is Message {
  if (!isObject(v) || typeof v.id !== "number" || !isObject(v.body) || !BODY_KINDS.has(v.body.kind as string)) {
    throw new ProtocolError("消息");
  }
}

export function parseSessionPage(v: unknown): SessionPage {
  if (!isObject(v) || !Array.isArray(v.items)) throw new ProtocolError("会话列表");
  return v as SessionPage;
}

export function parseCreated(v: unknown): Created {
  if (!isObject(v) || typeof v.session_id !== "number") throw new ProtocolError("新建会话");
  return v as Created;
}

export function parseStreamEvent(event: string, data: string): StreamEvent {
  if (!EVENTS.has(event)) throw new ProtocolError(`事件 ${event}`);
  const parsed: unknown = JSON.parse(data);
  if (!isObject(parsed)) throw new ProtocolError(`事件 ${event}`);
  if (event === "message") assertMessage(parsed);
  return { event, data: parsed } as StreamEvent;
}

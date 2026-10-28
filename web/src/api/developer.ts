import { arr, bool, kinded, nullable, num, obj, oneOf, ProtocolError, str, type Decoder } from "./decode";

// 与 docs/blueprints/developer-diagnostics.md §2.4、§3.4，developer-log-history.md §四 一一对应。

export const LOG_LEVELS = ["debug", "info", "warn", "error"] as const;
export type LogLevel = (typeof LOG_LEVELS)[number];

export type LogRecord = {
  timestamp_ms: number;
  level: LogLevel;
  target: string;
  message: string;
  fields: { name: string; value: string }[];
  file: string | null;
  line: number | null;
  truncated: boolean;
};

/** 日志落盘去向；非 sqlite 时之前启动的日志不可读。 */
export type LogStorage =
  | { kind: "sqlite" }
  | { kind: "file"; path: string; reason: string }
  | { kind: "memory"; reason: string };

export type StoredLogRecord = LogRecord & { boot_id: number };

export type LogHistory = { storage: LogStorage; boot_id: number; records: StoredLogRecord[] };

export type LogEvent =
  | { event: "ready"; data: { retained_count: number; history_trimmed: boolean } }
  | { event: "log"; data: LogRecord };

export type Cell =
  | { type: "null" }
  | { type: "integer"; value: string }
  | { type: "real"; value: string }
  | { type: "text"; value: string }
  | { type: "blob"; bytes: number };

/** `changes` 与 `backup` 仅写入模式有值。 */
export type QueryResult = { columns: string[]; rows: Cell[][]; truncated: boolean; elapsed_ms: number; changes: number | null; backup: string | null };

export type SqlRisk = "low" | "medium" | "high";

/** 写入前把关；`risk` 为 null 时 `ai_error` 是未经 AI 评估的原因。 */
export type SqlReview = { changes: number; tables: string[]; risk: SqlRisk | null; reason: string | null; ai_error: string | null };

export type TableSchema = { name: string; group: string; description: string; columns: { name: string; decl_type: string; secret: boolean }[] };

const logRecordFields = {
  timestamp_ms: num,
  level: oneOf(...LOG_LEVELS),
  target: str,
  message: str,
  fields: arr(obj({ name: str, value: str })),
  file: nullable(str),
  line: nullable(num),
  truncated: bool,
};

const logRecord: Decoder<LogRecord> = obj(logRecordFields);

const storage: Decoder<LogStorage> = kinded<{
  sqlite: Record<never, never>;
  file: { path: string; reason: string };
  memory: { reason: string };
}>({
  sqlite: {},
  file: { path: str, reason: str },
  memory: { reason: str },
});

const logHistory: Decoder<LogHistory> = obj({
  storage,
  boot_id: num,
  records: arr(obj<StoredLogRecord>({ ...logRecordFields, boot_id: num })),
});

export const parseStoredLogs = (v: unknown): StoredLogRecord[] => arr(obj<StoredLogRecord>({ ...logRecordFields, boot_id: num }))(v, "运行日志");

export const parseLogHistory = (v: unknown): LogHistory => logHistory(v, "日志历史");

const logData: { [E in LogEvent["event"]]: Decoder<Extract<LogEvent, { event: E }>["data"]> } = {
  ready: obj({ retained_count: num, history_trimmed: bool }),
  log: logRecord,
};

const cells: { [T in Cell["type"]]: Decoder<Extract<Cell, { type: T }>> } = {
  null: obj({ type: oneOf("null") }),
  integer: obj({ type: oneOf("integer"), value: str }),
  real: obj({ type: oneOf("real"), value: str }),
  text: obj({ type: oneOf("text"), value: str }),
  blob: obj({ type: oneOf("blob"), bytes: num }),
};

/** serde 内部标签 `{"type": …}`。 */
const cell: Decoder<Cell> = (v, at) => {
  const t = typeof v === "object" && v !== null ? (v as { type?: unknown }).type : undefined;
  if (typeof t !== "string" || !Object.hasOwn(cells, t)) throw new ProtocolError(`${at}.type`);
  return (cells[t as Cell["type"]] as Decoder<Cell>)(v, at);
};

const queryResult: Decoder<QueryResult> = obj({
  columns: arr(str),
  rows: arr(arr(cell)),
  truncated: bool,
  elapsed_ms: num,
  changes: nullable(num),
  backup: nullable(str),
});

const sqlReview: Decoder<SqlReview> = obj({
  changes: num,
  tables: arr(str),
  risk: nullable(oneOf("low", "medium", "high")),
  reason: nullable(str),
  ai_error: nullable(str),
});

const tables: Decoder<TableSchema[]> = arr(
  obj({ name: str, group: str, description: str, columns: arr(obj({ name: str, decl_type: str, secret: bool })) }),
);

export const parseQueryResult = (v: unknown): QueryResult => queryResult(v, "查询结果");

export const parseSqlReview = (v: unknown): SqlReview => sqlReview(v, "把关结果");

export const parseTables = (v: unknown): TableSchema[] => tables(v, "表结构");

export const logEvents = Object.keys(logData) as readonly LogEvent["event"][];

export function parseLogEvent(event: string, data: string): LogEvent {
  if (!Object.hasOwn(logData, event)) throw new ProtocolError(`事件 ${event}`);
  let parsed: unknown;
  try {
    parsed = JSON.parse(data);
  } catch {
    throw new ProtocolError(`事件 ${event}`);
  }
  const decode = logData[event as LogEvent["event"]] as Decoder<unknown>;
  return { event, data: decode(parsed, `事件 ${event}`) } as LogEvent;
}

// docs/blueprints/self-update.md §4.2。
export type UpdateStage = "checking" | "pulling" | "building" | "restarting";

export type UpdateStatus =
  | { state: "unavailable" }
  | { state: "idle"; version: string | null; running: string | null; committed_at: number | null; started_at: number; size: number | null }
  | { state: "checking"; output: string }
  | { state: "pulling"; from: string; attempt: number; output: string }
  | { state: "building"; from: string; to: string; from_size: number | null; output: string }
  | { state: "restarting"; from: string; to: string; from_size: number | null; to_size: number }
  | { state: "up_to_date"; commit: string }
  | { state: "failed"; stage: UpdateStage; from: string | null; error: string; output: string };

const stage = oneOf("checking", "pulling", "building", "restarting");

const updateStates: { [S in UpdateStatus["state"]]: Decoder<Extract<UpdateStatus, { state: S }>> } = {
  unavailable: obj({ state: oneOf("unavailable") }),
  idle: obj({ state: oneOf("idle"), version: nullable(str), running: nullable(str), committed_at: nullable(num), started_at: num, size: nullable(num) }),
  checking: obj({ state: oneOf("checking"), output: str }),
  pulling: obj({ state: oneOf("pulling"), from: str, attempt: num, output: str }),
  building: obj({ state: oneOf("building"), from: str, to: str, from_size: nullable(num), output: str }),
  restarting: obj({ state: oneOf("restarting"), from: str, to: str, from_size: nullable(num), to_size: num }),
  up_to_date: obj({ state: oneOf("up_to_date"), commit: str }),
  failed: obj({ state: oneOf("failed"), stage, from: nullable(str), error: str, output: str }),
};

/** serde 内部标签 `{"state": …}`。 */
export const parseUpdateStatus = (v: unknown): UpdateStatus => {
  const s = typeof v === "object" && v !== null ? (v as { state?: unknown }).state : undefined;
  if (typeof s !== "string" || !Object.hasOwn(updateStates, s)) throw new ProtocolError("更新状态.state");
  return (updateStates[s as UpdateStatus["state"]] as Decoder<UpdateStatus>)(v, "更新状态");
};

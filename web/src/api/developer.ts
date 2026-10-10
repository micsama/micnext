import { arr, bool, nullable, num, obj, oneOf, ProtocolError, str, type Decoder } from "./decode";

// 与 docs/blueprints/developer-diagnostics.md §2.4、§3.4 一一对应。

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

export type LogEvent =
  | { event: "ready"; data: { retained_count: number; history_trimmed: boolean } }
  | { event: "log"; data: LogRecord };

export type Cell =
  | { type: "null" }
  | { type: "integer"; value: string }
  | { type: "real"; value: string }
  | { type: "text"; value: string }
  | { type: "blob"; bytes: number };

export type QueryResult = { columns: string[]; rows: Cell[][]; truncated: boolean; elapsed_ms: number };

export type TableSchema = { name: string; columns: { name: string; decl_type: string; secret: boolean }[] };

const logRecord: Decoder<LogRecord> = obj({
  timestamp_ms: num,
  level: oneOf(...LOG_LEVELS),
  target: str,
  message: str,
  fields: arr(obj({ name: str, value: str })),
  file: nullable(str),
  line: nullable(num),
  truncated: bool,
});

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
});

const tables: Decoder<TableSchema[]> = arr(
  obj({ name: str, columns: arr(obj({ name: str, decl_type: str, secret: bool })) }),
);

export const parseQueryResult = (v: unknown): QueryResult => queryResult(v, "查询结果");

export const parseTables = (v: unknown): TableSchema[] => tables(v, "表结构");

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
  | { state: "idle" }
  | { state: "checking"; output: string }
  | { state: "pulling"; from: string; attempt: number; output: string }
  | { state: "building"; from: string; to: string; output: string }
  | { state: "restarting"; from: string; to: string }
  | { state: "up_to_date"; commit: string }
  | { state: "failed"; stage: UpdateStage; from: string | null; error: string; output: string };

const stage = oneOf("checking", "pulling", "building", "restarting");

const updateStates: { [S in UpdateStatus["state"]]: Decoder<Extract<UpdateStatus, { state: S }>> } = {
  unavailable: obj({ state: oneOf("unavailable") }),
  idle: obj({ state: oneOf("idle") }),
  checking: obj({ state: oneOf("checking"), output: str }),
  pulling: obj({ state: oneOf("pulling"), from: str, attempt: num, output: str }),
  building: obj({ state: oneOf("building"), from: str, to: str, output: str }),
  restarting: obj({ state: oneOf("restarting"), from: str, to: str }),
  up_to_date: obj({ state: oneOf("up_to_date"), commit: str }),
  failed: obj({ state: oneOf("failed"), stage, from: nullable(str), error: str, output: str }),
};

/** serde 内部标签 `{"state": …}`。 */
export const parseUpdateStatus = (v: unknown): UpdateStatus => {
  const s = typeof v === "object" && v !== null ? (v as { state?: unknown }).state : undefined;
  if (typeof s !== "string" || !Object.hasOwn(updateStates, s)) throw new ProtocolError("更新状态.state");
  return (updateStates[s as UpdateStatus["state"]] as Decoder<UpdateStatus>)(v, "更新状态");
};

import { auth } from "../state/auth.svelte";
import { obj, str } from "./decode";
import { parseStoredLogs, type StoredLogRecord } from "./developer";
import {
  parseSessionUsage, type SessionUsageView,
  parseDashboard,
  parsePeers,
  parseSystemPrompt,
  parseTraceRunPage,
  parseTraceSessions,
  type Dashboard,
  type Peers,
  type SystemPromptView,
  type TraceRunPage,
  type TraceSession,
} from "./trace";
import {
  parseLogHistory,
  parseQueryResult,
  parseTables,
  parseUpdateStatus,
  type LogHistory,
  type QueryResult,
  type TableSchema,
  type UpdateStatus,
} from "./developer";
import { parsePushPage, parsePushSettings, type PushPage, type PushSettings } from "./pushes";
import { parseMessage, parseMessages, type Message } from "./types";
import {
  parseContextBreakdown, parseContextView, parseCommands,
  parseJobs, parseJobOutput, parseStopJob, type JobView, type JobOutput, type StopJobOutcome,
  type ContextBreakdown, type ContextView, type CommandSpec,
  parseCreated,
  parseEndpoints,
  parseModels,
  parseTestResult,
  parseId,
  parseInputLimits,
  parsePersonas,
  parseSessionDetail,
  parseTitleModel,
  parseSessionPage,
  parseMessagePage,
  parseSettings,
  parseSetupAttempt,
  parseWechatView,
  type Created,
  type Cursor,
  type Endpoint,
  type EndpointInput,
  type EndpointTestInput,
  type InputLimits,
  type InputPart,
  type ModelInput,
  type ModelList,
  type ReasoningEffort,
  type Persona,
  type SessionDetail,
  type TitleModelSelection,
  type SessionPage,
  type MessagePage,
  type Settings,
  type SettingsInput,
  type SetupAttempt,
  type WechatView,
} from "./types";

/** 服务端以 `{"error": "…"}` 返回的失败。 */
export class ApiError extends Error {
  constructor(
    readonly status: number,
    message: string,
  ) {
    super(message);
  }
}

function authHeaders(): HeadersInit {
  return { Authorization: `Bearer ${auth.token}` };
}

/** 非 2xx 转成 `ApiError`；401 同时让 token 失效。 */
async function ensureOk(res: Response): Promise<Response> {
  if (res.ok) return res;
  if (res.status === 401) auth.expire();
  let message = `请求失败（${res.status}）`;
  try {
    const body: unknown = await res.json();
    if (typeof body === "object" && body !== null && typeof (body as { error?: unknown }).error === "string") {
      message = (body as { error: string }).error;
    }
  } catch {
    // 非 JSON 响应体（如代理错误页）：保留状态码说明。
  }
  throw new ApiError(res.status, message);
}

export async function request(method: string, path: string, body?: unknown): Promise<unknown> {
  let res: Response;
  try {
    res = await fetch(`/api${path}`, {
      method,
      headers: body === undefined ? authHeaders() : { ...authHeaders(), "Content-Type": "application/json" },
      body: body === undefined ? undefined : JSON.stringify(body),
    });
  } catch {
    throw new ApiError(0, "连不上服务，请确认 micnext 正在运行");
  }
  await ensureOk(res);
  return res.status === 204 || res.status === 202 ? undefined : res.json();
}

/** 换一张实时流的一次性票据，`path` 为流路径。 */
export async function getStreamTicket(path: string): Promise<string> {
  return obj({ ticket: str })(await request("POST", `${path}-ticket`), "票据").ticket;
}

export async function listSessions(channel: string, before: Cursor | null): Promise<SessionPage> {
  const q = new URLSearchParams({ channel });
  if (before) {
    q.set("before_at", String(before.before_at));
    q.set("before_id", String(before.before_id));
  }
  return parseSessionPage(await request("GET", `/sessions?${q}`));
}

export async function getSession(id: number): Promise<SessionDetail> {
  return parseSessionDetail(await request("GET", `/sessions/${id}`));
}

export async function generateTitle(id: number): Promise<void> {
  await request("POST", `/sessions/${id}/title`);
}

export async function archiveSession(id: number): Promise<void> {
  await request("POST", `/sessions/${id}/archive`);
}

export async function getTitleModel(): Promise<TitleModelSelection> {
  return parseTitleModel(await request("GET", "/settings/title-model"));
}

export async function putTitleModel(selection: TitleModelSelection): Promise<void> {
  await request("PUT", "/settings/title-model", selection);
}

export async function getMessages(sessionId: number, before: number): Promise<MessagePage> {
  return parseMessagePage(await request("GET", `/sessions/${sessionId}/messages?before=${before}`));
}

export async function createSession(parts: InputPart[], personaId: number, modelId: number, reasoningEffort: ReasoningEffort | null): Promise<Created> {
  return parseCreated(await request("POST", "/sessions", { parts, persona_id: personaId, model_id: modelId, reasoning_effort: reasoningEffort }));
}

export async function setSessionPersona(sessionId: number, personaId: number): Promise<void> {
  await request("PUT", `/sessions/${sessionId}/persona`, { persona_id: personaId });
}

export async function setSessionPlaintext(sessionId: number, on: boolean): Promise<void> {
  await request("PUT", `/sessions/${sessionId}/plaintext`, { on });
}

export async function setSessionModel(sessionId: number, modelId: number, reasoningEffort: ReasoningEffort | null): Promise<void> {
  await request("PUT", `/sessions/${sessionId}/model`, { model_id: modelId, reasoning_effort: reasoningEffort });
}

export async function listEndpoints(): Promise<Endpoint[]> {
  return parseEndpoints(await request("GET", "/endpoints"));
}

export async function createEndpoint(input: EndpointInput): Promise<number> {
  return parseId(await request("POST", "/endpoints", input));
}

export async function updateEndpoint(id: number, input: EndpointInput): Promise<void> {
  await request("PUT", `/endpoints/${id}`, input);
}

export async function deleteEndpoint(id: number): Promise<void> {
  await request("DELETE", `/endpoints/${id}`);
}

/** 联网取该服务商的模型名列表；失败时 message 已是用户可读的原因。 */
export async function testEndpoint(input: EndpointTestInput): Promise<string[]> {
  return parseTestResult(await request("POST", "/endpoints/test", input));
}

export async function listModels(): Promise<ModelList> {
  return parseModels(await request("GET", "/models"));
}

export async function createModel(input: ModelInput): Promise<number> {
  return parseId(await request("POST", "/models", input));
}

export async function updateModel(id: number, input: ModelInput): Promise<void> {
  await request("PUT", `/models/${id}`, input);
}

export async function deleteModel(id: number): Promise<void> {
  await request("DELETE", `/models/${id}`);
}

export async function setDefaultModel(id: number): Promise<void> {
  await request("PUT", "/models/default", { model_id: id });
}

export async function getSettings(): Promise<Settings> {
  return parseSettings(await request("GET", "/settings"));
}

export async function getWechat(): Promise<WechatView> {
  return parseWechatView(await request("GET", "/channels/wechat"));
}

export async function beginWechatLogin(): Promise<SetupAttempt> {
  return parseSetupAttempt(await request("POST", "/channels/wechat/login", {}));
}

export async function submitWechatCode(id: string, code: string): Promise<void> {
  await request("POST", `/channels/wechat/login/${encodeURIComponent(id)}/code`, { code });
}

export async function cancelWechatLogin(id: string): Promise<void> {
  await request("DELETE", `/channels/wechat/login/${encodeURIComponent(id)}`);
}

export async function putSettings(s: SettingsInput): Promise<void> {
  await request("PUT", "/settings", s);
}

export async function listPersonas(): Promise<Persona[]> {
  return parsePersonas(await request("GET", "/personas"));
}

export async function createPersona(name: string, prompt: string): Promise<number> {
  return parseId(await request("POST", "/personas", { name, prompt }));
}

export async function updatePersona(id: number, name: string, prompt: string): Promise<void> {
  await request("PUT", `/personas/${id}`, { name, prompt });
}

export async function deletePersona(id: number): Promise<void> {
  await request("DELETE", `/personas/${id}`);
}

export async function sendMessage(sessionId: number, parts: InputPart[]): Promise<void> {
  await request("POST", `/sessions/${sessionId}/messages`, { parts });
}

export async function getInputLimits(): Promise<InputLimits> {
  return parseInputLimits(await request("GET", "/model-kinds"));
}

/** 带 token 取会话内图片原件；token 不进 URL。 */
export async function fetchImage(sessionId: number, imageId: number): Promise<Blob> {
  let res: Response;
  try {
    res = await fetch(`/api/sessions/${sessionId}/images/${imageId}`, { headers: authHeaders() });
  } catch {
    throw new ApiError(0, "连不上服务，请确认 micnext 正在运行");
  }
  return (await ensureOk(res)).blob();
}

/** 只读诊断 SQL；失败时 message 已是用户可读的原因。 */
export async function runSql(sql: string): Promise<QueryResult> {
  return parseQueryResult(await request("POST", "/developer/sql", { sql }));
}

export async function getSqlSchema(): Promise<TableSchema[]> {
  return parseTables(await request("GET", "/developer/sql/schema"));
}

export async function getLogHistory(): Promise<LogHistory> {
  return parseLogHistory(await request("GET", "/developer/logs/history"));
}

export async function getUpdateStatus(): Promise<UpdateStatus> {
  return parseUpdateStatus(await request("GET", "/developer/update"));
}

export async function startUpdate(): Promise<void> {
  await request("POST", "/developer/update");
}

export async function getContext(sessionId: number): Promise<ContextView> {
  return parseContextView(await request("GET", `/sessions/${sessionId}/context`));
}

export async function getContextBreakdown(sessionId: number): Promise<ContextBreakdown | null> {
  return parseContextBreakdown(await request("GET", `/sessions/${sessionId}/context/breakdown`));
}
export async function listCommands(): Promise<CommandSpec[]> {
  return parseCommands(await request("GET", "/commands"));
}
/** 停止指定执行；该执行已结束时 409。 */
export async function stopRun(sessionId: number, runId: number): Promise<void> {
  await request("POST", `/sessions/${sessionId}/runs/${runId}/stop`);
}
export async function listJobs(sessionId: number, includeOld = false): Promise<JobView[]> {
  return parseJobs(await request("GET", `/sessions/${sessionId}/jobs?include_old=${includeOld}`));
}
/** `offset` 缺省读尾部。 */
export async function jobOutput(sessionId: number, jobId: number, offset?: number, limit?: number): Promise<JobOutput> {
  const q = new URLSearchParams({ ...(offset === undefined ? {} : { offset: String(offset) }), ...(limit === undefined ? {} : { limit: String(limit) }) });
  return parseJobOutput(await request("GET", `/sessions/${sessionId}/jobs/${jobId}/output?${q}`));
}
export async function stopJob(sessionId: number, jobId: number): Promise<StopJobOutcome> {
  return parseStopJob(await request("POST", `/sessions/${sessionId}/jobs/${jobId}/stop`));
}
export async function revokeCompaction(sessionId: number, boundaryId: number): Promise<void> {
  await request("POST", `/sessions/${sessionId}/compactions/${boundaryId}/revoke`);
}

export async function getTraceSessions(): Promise<TraceSession[]> {
  return parseTraceSessions(await request("GET", "/trace/sessions"));
}

export async function getTraceRuns(session: number, after: number | null): Promise<TraceRunPage> {
  const q = after === null ? "" : `?after=${after}`;
  return parseTraceRunPage(await request("GET", `/trace/sessions/${session}${q}`));
}

export async function getTraceSystem(session: number): Promise<SystemPromptView> {
  return parseSystemPrompt(await request("GET", `/trace/sessions/${session}/system`));
}

export async function getTraceMessage(id: number): Promise<Message> {
  return parseMessage(await request("GET", `/trace/messages/${id}`));
}

export async function getTraceCallInput(id: number): Promise<Message[]> {
  return parseMessages(await request("GET", `/trace/calls/${id}/input`));
}

export async function getTraceRunLogs(run: number): Promise<StoredLogRecord[]> {
  return parseStoredLogs(await request("GET", `/trace/runs/${run}/logs`));
}

export async function getTracePeers(): Promise<Peers> {
  return parsePeers(await request("GET", "/trace/peers"));
}

export async function getDashboard(days: number): Promise<Dashboard> {
  return parseDashboard(await request("GET", `/trace/dashboard?days=${days}`));
}

export async function exportSettings(): Promise<string> {
  return obj({ text: str })(await request("POST", "/settings/export"), "设置备份").text;
}

export async function importSettings(text: string): Promise<void> {
  await request("POST", "/settings/import", { text });
}


export async function getSessionUsage(session: number): Promise<SessionUsageView> {
  return parseSessionUsage(await request("GET", `/trace/sessions/${session}/usage`));
}

export async function listPushes(before: number | null): Promise<PushPage> {
  const q = new URLSearchParams();
  if (before !== null) q.set("before", String(before));
  return parsePushPage(await request("GET", `/pushes?${q}`));
}

export async function markPushesRead(through: number): Promise<void> {
  await request("POST", "/pushes/read", { through });
}

export async function getPushSettings(): Promise<PushSettings> {
  return parsePushSettings(await request("GET", "/settings/push"));
}

export async function putPushSettings(channels: string[]): Promise<void> {
  await request("PUT", "/settings/push", { channels });
}

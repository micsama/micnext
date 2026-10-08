import { auth } from "../state/auth.svelte";
import {
  parseCreated,
  parseEndpoints,
  parseModels,
  parseTestResult,
  parseId,
  parsePersonas,
  parseSessionItem,
  parseSessionPage,
  parseSettings,
  type Created,
  type Cursor,
  type Endpoint,
  type EndpointInput,
  type EndpointTestInput,
  type ModelInput,
  type ModelList,
  type Persona,
  type SessionItem,
  type SessionPage,
  type Settings,
  type SettingsInput,
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

export function authHeaders(): HeadersInit {
  return { Authorization: `Bearer ${auth.token}` };
}

/** 非 2xx 转成 `ApiError`；401 同时让 token 失效。 */
export async function ensureOk(res: Response): Promise<Response> {
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

async function request(method: string, path: string, body?: unknown): Promise<unknown> {
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
  return res.status === 204 ? undefined : res.json();
}

export async function listSessions(channel: string, before: Cursor | null): Promise<SessionPage> {
  const q = new URLSearchParams({ channel });
  if (before) {
    q.set("before_at", String(before.before_at));
    q.set("before_id", String(before.before_id));
  }
  return parseSessionPage(await request("GET", `/sessions?${q}`));
}

export async function getSession(id: number): Promise<SessionItem> {
  return parseSessionItem(await request("GET", `/sessions/${id}`));
}

export async function createSession(text: string, personaId: number, modelId: number): Promise<Created> {
  return parseCreated(await request("POST", "/sessions", { text, persona_id: personaId, model_id: modelId }));
}

export async function setSessionPersona(sessionId: number, personaId: number): Promise<void> {
  await request("PUT", `/sessions/${sessionId}/persona`, { persona_id: personaId });
}

export async function setSessionModel(sessionId: number, modelId: number): Promise<void> {
  await request("PUT", `/sessions/${sessionId}/model`, { model_id: modelId });
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

export async function sendMessage(sessionId: number, text: string): Promise<void> {
  await request("POST", `/sessions/${sessionId}/messages`, { text });
}

import { auth } from "../state/auth.svelte";
import { parseCreated, parseSessionPage, type Created, type Cursor, type SessionPage } from "./types";

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
  return res.json();
}

export async function listSessions(channel: string, before: Cursor | null): Promise<SessionPage> {
  const q = new URLSearchParams({ channel });
  if (before) {
    q.set("before_at", String(before.before_at));
    q.set("before_id", String(before.before_id));
  }
  return parseSessionPage(await request("GET", `/sessions?${q}`));
}

export async function createSession(text: string): Promise<Created> {
  return parseCreated(await request("POST", "/sessions", { text }));
}

export async function sendMessage(sessionId: number, text: string): Promise<void> {
  await request("POST", `/sessions/${sessionId}/messages`, { text });
}

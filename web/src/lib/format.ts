import type { SessionItem } from "../api/types";

const DAY = 86_400_000;

function startOfToday(): number {
  const d = new Date();
  d.setHours(0, 0, 0, 0);
  return d.getTime();
}

export function formatTime(ms: number): string {
  const d = new Date(ms);
  const time = d.toLocaleTimeString("zh-CN", { hour: "2-digit", minute: "2-digit" });
  return ms >= startOfToday() ? time : `${d.toLocaleDateString("zh-CN")} ${time}`;
}

/** 通知用：刚刚 / N 分钟前 / 今天时刻 / 昨天 / 日期。 */
export function formatAgo(ms: number): string {
  const diff = Date.now() - ms;
  if (diff < 60_000) return "刚刚";
  if (diff < 3_600_000) return `${Math.floor(diff / 60_000)} 分钟前`;
  const time = new Date(ms).toLocaleTimeString("zh-CN", { hour: "2-digit", minute: "2-digit" });
  const today = startOfToday();
  if (ms >= today) return time;
  if (ms >= today - DAY) return `昨天 ${time}`;
  return formatTime(ms);
}

export type SessionGroup = { label: string; items: SessionItem[] };

/** 按最近活跃分组：今天 / 昨天 / 最近 7 天 / 更早。输入已按活跃时间倒序。 */
export function groupSessions(items: SessionItem[]): SessionGroup[] {
  const today = startOfToday();
  const bounds: [string, number][] = [
    ["今天", today],
    ["昨天", today - DAY],
    ["最近 7 天", today - 6 * DAY],
    ["更早", -Infinity],
  ];
  const groups: SessionGroup[] = [];
  for (const s of items) {
    const label = bounds.find(([, from]) => s.last_activity_at >= from)![0];
    const last = groups.at(-1);
    if (last?.label === label) last.items.push(s);
    else groups.push({ label, items: [s] });
  }
  return groups;
}

/** 工具参数预览：`description` 是模型写给用户的标签；`detail` 取其余第一个字符串值，压成一行，没有时取任务号。 */
export function argsPreview(args: unknown): { description: string; detail: string } {
  if (typeof args === "string") return { description: "", detail: oneLine(args) };
  if (typeof args !== "object" || args === null) return { description: "", detail: "" };
  const { description, ...rest } = args as Record<string, unknown>;
  const first = Object.values(rest).find((v) => typeof v === "string");
  const detail = typeof first === "string" ? oneLine(first) : typeof rest.job_id === "number" ? `#${rest.job_id}` : "";
  return { description: typeof description === "string" ? oneLine(description) : "", detail };
}

function oneLine(s: string): string {
  return s.replace(/\s+/g, " ").trim();
}

export function prettyArgs(args: unknown): string {
  return typeof args === "string" ? args : JSON.stringify(args, null, 2);
}

/** 耗时：<1 秒用毫秒，其余用秒。 */
export function formatDuration(ms: number): string {
  if (ms < 1000) return `${ms} ms`;
  if (ms < 60_000) return `${(ms / 1000).toFixed(ms < 10_000 ? 2 : 1)} s`;
  const s = Math.floor(ms / 1000);
  const pad = (n: number) => String(n).padStart(2, "0");
  return s < 3600 ? `${Math.floor(s / 60)}m ${pad(s % 60)}s` : `${Math.floor(s / 3600)}h ${pad(Math.floor(s / 60) % 60)}m`;
}

export function formatTokens(n: number): string {
  return n < 10_000 ? String(n) : n < 1_000_000 ? `${(n / 1000).toFixed(1)}k` : `${(n / 1_000_000).toFixed(2)}M`;
}

/** 美元，小额保留足够有效位。 */
export function formatCost(usd: number): string {
  return `$${usd < 0.01 ? usd.toFixed(5) : usd.toFixed(usd < 1 ? 4 : 2)}`;
}

export function formatBytes(n: number): string {
  return n < 1024 ? `${n} B` : n < 1024 * 1024 ? `${(n / 1024).toFixed(1)} KB` : `${(n / 1024 / 1024).toFixed(1)} MB`;
}

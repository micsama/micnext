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

/** 工具参数摘要：参数对象的第一个字符串值，压成一行。 */
export function argsSummary(args: unknown): string {
  if (typeof args === "string") return oneLine(args);
  if (typeof args === "object" && args !== null) {
    const first = Object.values(args).find((v) => typeof v === "string");
    if (typeof first === "string") return oneLine(first);
  }
  return "";
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

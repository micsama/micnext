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

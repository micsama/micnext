import { arr, bool, kinded, nullable, num, obj, str, type Decoder } from "./decode";
import { request } from "./client";
import { delivery, type PushDelivery } from "./pushes";
import { callStat } from "./trace";

export type FireOutcome =
  | { kind: "pending" | "failed" | "timed_out" | "stopped" | "interrupted" | "busy" }
  | { kind: "completed"; pushed: boolean }
  | { kind: "missed"; count: number };
export type Fire = { id: number; schedule_id: number; due_at: number; outcome: FireOutcome; run_id: number | null };
export type ScheduleFire = Fire & { pushes: { id: number; deliveries: PushDelivery[] }[]; reply: string | null };
export type FirePage = { fires: ScheduleFire[]; has_more: boolean };
const usage = obj(callStat);
export type ScheduleUsage = ReturnType<typeof usage>;
export type Schedule = {
  id: number; person: number; name: string; rule: string; prompt: string;
  next_run_at: number | null; paused_reason: string | null; enabled_at: number;
  session_id: number; created_at: number; updated_at: number; enabled: boolean;
  recent: Fire[]; usage: ScheduleUsage;
};
export type ScheduleUpdate = Partial<Pick<Schedule, "name" | "rule" | "prompt" | "enabled">>;
const outcome: Decoder<FireOutcome> = kinded<{
  pending: Record<string, never>; completed: { pushed: boolean }; failed: Record<string, never>;
  timed_out: Record<string, never>; stopped: Record<string, never>; interrupted: Record<string, never>;
  missed: { count: number }; busy: Record<string, never>;
}>({
  pending: {}, completed: { pushed: bool }, failed: {}, timed_out: {}, stopped: {},
  interrupted: {}, missed: { count: num }, busy: {},
});
const fireFields = { id: num, schedule_id: num, due_at: num, outcome, run_id: nullable(num) };
const fire = obj(fireFields);
const schedule: Decoder<Schedule> = obj({
  id: num, person: num, name: str, rule: str, prompt: str, next_run_at: nullable(num),
  paused_reason: nullable(str), enabled_at: num, session_id: num, created_at: num, updated_at: num,
  enabled: bool, recent: arr(fire), usage,
});
const firePage: Decoder<FirePage> = obj({
  fires: arr(obj({ ...fireFields, reply: nullable(str), pushes: arr(obj({ id: num, deliveries: arr(delivery) })) })),
  has_more: bool,
});
export const listSchedules = async (): Promise<Schedule[]> => arr(schedule)(await request("GET", "/schedules"), "定时任务");
export const updateSchedule = async (id: number, edit: ScheduleUpdate): Promise<Schedule> => schedule(await request("PATCH", `/schedules/${id}`, edit), "定时任务");
export const deleteSchedule = async (id: number): Promise<void> => { await request("DELETE", `/schedules/${id}`); };
export const listFires = async (id: number, before: number | null): Promise<FirePage> => firePage(await request("GET", `/schedules/${id}/fires${before === null ? "" : `?before=${before}`}`), "触发列表");

export function outcomeLabel(o: FireOutcome): string {
  switch (o.kind) {
    case "pending": return "等待 / 运行中";
    case "completed": return o.pushed ? "完成 · 已推送" : "完成 · 未推送";
    case "failed": return "失败";
    case "timed_out": return "超时";
    case "stopped": return "已停止";
    case "interrupted": return "中断";
    case "missed": return `错过 ${o.count} 次`;
    case "busy": return "忙 · 跳过";
  }
}

/** 只翻译明确匹配的常见形态，其余保留原文。 */
export function ruleLabel(rule: string): string {
  const once = /^(\d{4})-(\d{2})-(\d{2}) (\d{2}:\d{2})$/.exec(rule);
  if (once) return `${once[1]} 年 ${Number(once[2])} 月 ${Number(once[3])} 日 ${once[4]} 一次`;
  const every = /^\*\/(\d+) \* \* \* \*$/.exec(rule);
  if (every && 60 % Number(every[1]) === 0) return `每 ${every[1]} 分钟`;
  const daily = /^(\d+) (\d+) \* \* (\*|1-5)$/.exec(rule);
  if (daily) return `${daily[3] === "*" ? "每天" : "工作日"} ${daily[2]!.padStart(2, "0")}:${daily[1]!.padStart(2, "0")}`;
  return rule;
}

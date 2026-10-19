import type { IconName } from "../lib/Icon.svelte";
import type { JobState, JobView, RunState } from "../api/types";

/** `bad` 表示结局需要用户留意。 */
export type JobLook = { label: string; icon: IconName; tone: string; bad: boolean };

const STATE: Record<JobState, JobLook> = {
  running: { label: "运行中", icon: "loader", tone: "text-primary", bad: false },
  completed: { label: "已完成", icon: "check", tone: "text-success", bad: false },
  failed: { label: "失败", icon: "error", tone: "text-destructive", bad: true },
  timed_out: { label: "超时", icon: "clock", tone: "text-warning", bad: true },
  cancelled: { label: "已终止", icon: "stop", tone: "text-muted-foreground", bad: false },
  interrupted: { label: "已中断", icon: "alert", tone: "text-warning", bad: true },
};
/** agent 最近一个子 run 的结局；休眠时显示。 */
const LAST_RUN: Partial<Record<RunState, JobLook>> = {
  completed: { label: "休眠 · 上次完成", icon: "pause", tone: "text-muted-foreground", bad: false },
  max_turns: { label: "休眠 · 上次轮数用尽", icon: "pause", tone: "text-warning", bad: true },
  provider_failed: { label: "休眠 · 上次调用失败", icon: "error", tone: "text-destructive", bad: true },
  timed_out: { label: "休眠 · 上次超时", icon: "clock", tone: "text-warning", bad: true },
  stopped: { label: "休眠 · 上次被终止", icon: "stop", tone: "text-muted-foreground", bad: false },
  interrupted: { label: "休眠 · 上次中断", icon: "alert", tone: "text-warning", bad: true },
};
const IDLE: JobLook = { label: "休眠", icon: "pause", tone: "text-muted-foreground", bad: false };

export function jobLook(j: JobView): JobLook {
  if (j.kind === "tool") return STATE[j.state];
  if (j.running) return STATE.running;
  return (j.last_run && LAST_RUN[j.last_run]) || IDLE;
}

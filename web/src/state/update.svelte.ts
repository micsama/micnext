import { ApiError, getUpdateStatus, startUpdate } from "../api/client";
import { ProtocolError } from "../api/decode";
import type { UpdateStatus } from "../api/developer";
import { confirm } from "./confirm.svelte";

const POLL_MS = 1000;
// 目标提交按浏览器记住：随机 token 重启后需重新登录，页面会重新挂载。
const TARGET_KEY = "micnext.developer.updateTarget";

function loadTarget(): string | null {
  try {
    return localStorage.getItem(TARGET_KEY);
  } catch {
    return null;
  }
}
function saveTarget(v: string | null) {
  try {
    if (v === null) localStorage.removeItem(TARGET_KEY);
    else localStorage.setItem(TARGET_KEY, v);
  } catch {
    // NOTE: 存储不可用时仅本页有效。
  }
}

/** 一键更新的状态，侧边栏快捷按钮与「更新」页签共用；进行中自动轮询。 */
class Updater {
  status = $state.raw<UpdateStatus | null>(null);
  error = $state<string | null>(null);
  /** 已请求重启，等待服务重新可连。 */
  reconnecting = $state(false);
  /** 重启后核对：新进程报告的版本是否为目标提交。 */
  verdict = $state<{ target: string; running: string | null } | null>(null);
  readonly running = $derived(
    this.status !== null && ["checking", "pulling", "building", "restarting"].includes(this.status.state),
  );

  #target = loadTarget();
  /** 本页是否已从服务取到过状态；首次之后再见到重启结果，说明页面代码是旧版。 */
  #loaded = false;
  #timer: ReturnType<typeof setInterval> | null = null;

  async refresh(): Promise<void> {
    try {
      const next = await getUpdateStatus();
      this.reconnecting = false;
      if (next.state === "building" || next.state === "restarting") {
        if (this.#target !== next.to) saveTarget((this.#target = next.to));
      } else if (next.state === "idle" && this.#target !== null) {
        // 服务已换新版，重载取新界面，核对由重载后的页面按已存目标完成。
        if (this.#loaded) return location.reload();
        this.verdict = { target: this.#target, running: next.running };
        saveTarget((this.#target = null));
      } else if (next.state !== "unavailable" && this.#target !== null) {
        saveTarget((this.#target = null));
      }
      this.status = next;
      this.error = null;
      this.#loaded = true;
    } catch (e) {
      if (this.#loaded && this.#target !== null && e instanceof ProtocolError) {
        location.reload();
      } else if (this.status?.state === "restarting" && e instanceof ApiError && e.status === 0) {
        this.reconnecting = true;
      } else {
        this.error = (e as Error).message;
      }
    }
    this.#poll();
  }

  /** 确认后开始更新；取消返回 false。 */
  async start(): Promise<boolean> {
    const ok = await confirm.ask({
      title: "拉取并更新？",
      description: "有新提交时会构建并重启服务，正在进行的对话会被中断。页面关闭不影响更新。",
      action: "更新",
    });
    if (!ok) return false;
    this.verdict = null;
    try {
      await startUpdate();
    } catch (e) {
      this.error = (e as Error).message;
    }
    await this.refresh();
    return true;
  }

  #poll(): void {
    const active = this.running || this.reconnecting;
    if (active && this.#timer === null) this.#timer = setInterval(() => void this.refresh(), POLL_MS);
    else if (!active && this.#timer !== null) {
      clearInterval(this.#timer);
      this.#timer = null;
    }
  }
}

export const updater = new Updater();

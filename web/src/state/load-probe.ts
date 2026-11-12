import { tick } from "svelte";
import { reportSessionLoad, type SessionLoadReport } from "../api/client";
import type { ReadyTiming, StreamEvent } from "../api/types";

/** 打开会话超过这么久（含没等到就离开）才上报。 */
const SLOW_MS = 800;
/** 主线程心跳间隔；实际间隔超出的部分即主线程延迟。 */
const HEARTBEAT_MS = 50;

type Phase = "session" | "open" | "first" | "ready" | "painted";

const round = (ms: number) => Math.max(0, Math.round(ms));

/**
 * 临时诊断（session-load-diagnostics），排查完整体删除。
 * 一次打开会话（首次或重连）的分段计时与旁证；慢了报给服务端记进运行日志，快的丢弃。
 * 契约：docs/blueprints/session-load-diagnostics.md。
 */
export class LoadProbe {
  readonly #t0 = performance.now();
  readonly #phases: Partial<Record<Phase, number>> = {};
  #server: ReadyTiming | null = null;
  #events = 0;
  #chars = 0;
  #lastEvent = 0;
  #maxGap = 0;
  #beat = this.#t0;
  #lag = 0;
  readonly #timer: ReturnType<typeof setInterval>;
  readonly #hiddenAtStart = document.visibilityState === "hidden";
  #visibilityChanged = false;
  readonly #onVisibility = () => { this.#visibilityChanged = true; };
  #done = false;
  #frame: number | null = null;

  constructor(readonly sessionId: number, readonly reconnect: boolean, readonly running: number) {
    // 缓冲满了就不再记新条目，每次清空，保证取得到本次的 `getSession`。
    performance.clearResourceTimings();
    this.#timer = setInterval(() => {
      const now = performance.now();
      this.#lag = Math.max(this.#lag, now - this.#beat - HEARTBEAT_MS);
      this.#beat = now;
    }, HEARTBEAT_MS);
    document.addEventListener("visibilitychange", this.#onVisibility);
  }

  mark(phase: Phase): void {
    this.#phases[phase] ??= performance.now() - this.#t0;
    if (phase === "open") this.#lastEvent = performance.now();
  }

  /** 记到 `ready` 为止；之后是实时增量，不算加载。 */
  event(e: StreamEvent, chars: number): void {
    if (this.#phases.ready !== undefined) return;
    const now = performance.now();
    this.#maxGap = Math.max(this.#maxGap, now - this.#lastEvent);
    this.#lastEvent = now;
    this.#events++;
    this.#chars += chars;
    if (e.event === "message") this.mark("first");
    if (e.event === "ready") {
      this.mark("ready");
      this.#server = e.data.timing;
    }
  }

  /** `ready` 已归约进状态：等列表画出来再收尾。 */
  async painted(messages: () => number): Promise<void> {
    await tick();
    if (this.#done) return;
    this.#frame = requestAnimationFrame(() => {
      this.#frame = null;
      this.mark("painted");
      this.finish(messages());
    });
  }

  /** 画出来、连接结束或离开时调用，只生效一次。 */
  finish(messages: number): void {
    if (this.#done) return;
    this.#done = true;
    if (this.#frame !== null) cancelAnimationFrame(this.#frame);
    clearInterval(this.#timer);
    document.removeEventListener("visibilitychange", this.#onVisibility);
    const elapsed = performance.now() - this.#t0;
    if (elapsed < SLOW_MS) return;
    const phase = (p: Phase) => (this.#phases[p] === undefined ? null : round(this.#phases[p]));
    const report: SessionLoadReport = {
      session_id: this.sessionId,
      reconnect: this.reconnect,
      elapsed: round(elapsed),
      session: phase("session"),
      open: phase("open"),
      first: phase("first"),
      ready: phase("ready"),
      painted: phase("painted"),
      server: this.#server,
      net: this.#phases.session === undefined ? null : this.#net(),
      events: this.#events,
      chars: this.#chars,
      max_gap: round(this.#maxGap),
      lag: round(this.#lag),
      hidden_at_start: this.#hiddenAtStart,
      visibility_changed: this.#visibilityChanged,
      running: this.running,
      messages,
      ua: navigator.userAgent,
    };
    void reportSessionLoad(report).catch(() => {});
  }

  #net(): SessionLoadReport["net"] {
    const url = new URL(`/api/sessions/${this.sessionId}`, location.href).href;
    const t = performance.getEntriesByName(url).at(-1) as PerformanceResourceTiming | undefined;
    if (!t) return null;
    return {
      dns: round(t.domainLookupEnd - t.domainLookupStart),
      connect: round(t.connectEnd - t.connectStart),
      tls: t.secureConnectionStart > 0 ? round(t.connectEnd - t.secureConnectionStart) : 0,
      ttfb: round(t.responseStart - t.requestStart),
      download: round(t.responseEnd - t.responseStart),
      protocol: t.nextHopProtocol,
    };
  }
}

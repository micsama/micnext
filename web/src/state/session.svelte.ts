import { ApiError, generateTitle, getContext, getMessages, revokeCompaction, getSession } from "../api/client";
import { readStream } from "../api/stream";
import { ProtocolError, type ContextView, type Message, type RunState, type SessionDetail, type StreamEvent } from "../api/types";
import { SvelteMap } from "svelte/reactivity";
import { sleep } from "../lib/sleep";
import { otherSessions, webSessions } from "./sessions.svelte";

export type Draft = { reasoning: string; text: string };

const RETRY_MIN_MS = 1000;
const RETRY_MAX_MS = 10_000;

const FINISH_NOTICE: Partial<Record<RunState, string>> = {
  provider_failed: "模型调用失败，详情见服务日志",
  max_turns: "已达到单轮调用上限，任务可能没有完成",
  interrupted: "这一轮因服务停止而中断",
};

/** 一个打开的会话：稳定消息 + 正在生成的草稿，按 web-ui §五 归约流事件并断线重连。 */
export class SessionView {
  /** 会话本身的信息，首次连上前取得。 */
  info = $state<SessionDetail | null>(null);
  titleSyncError = $state<string | null>(null);
  titleActionError = $state<string | null>(null);
  requestingTitle = $state(false);
  #titleRefresh: Promise<void> | null = null;
  #titleDirty = false;

  async regenerateTitle(): Promise<void> {
    if (this.requestingTitle || this.info?.title_generating) return;
    this.requestingTitle = true;
    this.titleActionError = null;
    try { await generateTitle(this.id); }
    catch (error) { this.titleActionError = (error as Error).message; }
    finally { this.requestingTitle = false; this.#refreshTitle(); }
  }

  #refreshTitle(): void {
    this.#titleDirty = true;
    if (this.#titleRefresh) return;
    this.#titleRefresh = (async () => {
      while (this.#titleDirty && !this.#abort.signal.aborted) {
        this.#titleDirty = false;
        try {
          const info = await getSession(this.id);
          if (this.#abort.signal.aborted) return;
          if (this.info) Object.assign(this.info, { title: info.title, title_error: info.title_error, title_generating: info.title_generating });
          for (const list of [webSessions, ...otherSessions]) list.patchTitle(info);
          this.titleSyncError = null;
        } catch (error) {
          if (!this.#abort.signal.aborted) this.titleSyncError = (error as Error).message;
        }
      }
    })().finally(() => { this.#titleRefresh = null; });
  }
  messages = $state<Message[]>([]);
  context = $state<ContextView | null>(null);
  contextError = $state<string | null>(null);
  revoking = $state<number | null>(null);
  #contextRefresh: Promise<void> | null = null;
  #contextDirty = false;

  async revoke(boundaryId: number): Promise<void> {
    this.revoking = boundaryId;
    try { await revokeCompaction(this.id, boundaryId); }
    catch (error) { this.revoking = null; throw error; }
  }

  #refreshContext(): void {
    this.#contextDirty = true;
    if (this.#contextRefresh) return;
    this.#contextRefresh = (async () => {
      while (this.#contextDirty && !this.#abort.signal.aborted) {
        this.#contextDirty = false;
        try {
          this.context = await getContext(this.id);
          this.contextError = null;
          if (this.revoking !== null && this.context.active_boundary !== this.revoking) this.revoking = null;
        } catch (error) { this.contextError = (error as Error).message; }
      }
    })().finally(() => { this.#contextRefresh = null; });
  }
  draft = $state<Draft | null>(null);
  executingRun = $state<number | null>(null);
  ready = $state(false);
  /** 连接断开、正在重连。 */
  reconnecting = $state(false);
  /** 本页观察到的一轮非正常结束。 */
  notice = $state<string | null>(null);
  /** 无法继续的错误（会话不存在、版本不一致），不再重连。 */
  fatal = $state<string | null>(null);
  /** 首次回放之前的历史：`idle` 表示可能还有、可以取。 */
  earlier = $state<"idle" | "loading" | "none" | { error: string }>("idle");
  /** 每次加载更早一页后的交界：插入前的首条消息 id → 本次取到的可见条数。 */
  readonly loadedAt = new SvelteMap<number, number>();
  /** 最近一次成功加载：每次换新对象，供界面弹出完成提示。 */
  lastLoaded = $state<{ count: number; reachedStart: boolean } | null>(null);

  #abort = new AbortController();

  constructor(readonly id: number) {
    void this.#run();
  }

  close(): void {
    this.#abort.abort();
  }

  get lastId(): number | null {
    return this.messages.at(-1)?.id ?? null;
  }

  /** 取 messages[0] 之前的一页插到最前面；同一时刻只跑一个。 */
  async loadEarlier(): Promise<void> {
    const first = this.messages[0];
    if (!this.ready || !first || this.earlier === "loading" || this.earlier === "none") return;
    this.earlier = "loading";
    try {
      const page = await getMessages(this.id, first.id);
      if (this.#abort.signal.aborted) return;
      this.messages.unshift(...page.messages);
      const shown = page.messages.filter((m) => m.body.kind !== "HarnessNote").length;
      if (shown > 0) this.loadedAt.set(first.id, shown);
      this.lastLoaded = { count: shown, reachedStart: !page.has_earlier };
      this.earlier = page.has_earlier ? "idle" : "none";
    } catch (error) {
      if (!this.#abort.signal.aborted) this.earlier = { error: (error as Error).message };
    }
  }

  async #run(): Promise<void> {
    const signal = this.#abort.signal;
    let delay = RETRY_MIN_MS;
    while (!signal.aborted) {
      // 接入后只收到剩余增量，保留旧半截会拼出缺段的文字。
      this.draft = null;
      try {
        this.info ??= await getSession(this.id);
        await readStream(this.id, this.lastId, signal, (e) => {
          this.#apply(e);
          if (e.event === "ready") delay = RETRY_MIN_MS;
        });
      } catch (e) {
        if (signal.aborted) return;
        if (e instanceof ApiError && e.status === 401) return;
        if (e instanceof ApiError && e.status === 404) {
          this.fatal = "会话不存在";
          return;
        }
        if (e instanceof ProtocolError) {
          this.fatal = e.message;
          return;
        }
      }
      if (signal.aborted) return;
      this.reconnecting = true;
      await sleep(delay, signal);
      delay = Math.min(delay * 2, RETRY_MAX_MS);
    }
  }

  #apply(e: StreamEvent): void {
    if (e.event === "ready" || e.event === "session_title_changed") this.#refreshTitle();
    if (e.event === "ready" || e.event === "run_finished" || (e.event === "message" && ["Boundary", "Notification", "Command"].includes(e.data.body.kind))) this.#refreshContext();
    switch (e.event) {
      case "message":
        this.messages.push(e.data);
        if (e.data.body.kind === "Reply") this.draft = null;
        if (e.data.body.kind === "UserInput") this.notice = null;
        break;
      case "text_delta":
        (this.draft ??= { reasoning: "", text: "" }).text += e.data.text;
        break;
      case "reasoning_delta":
        (this.draft ??= { reasoning: "", text: "" }).reasoning += e.data.text;
        break;
      case "draft_discarded":
        this.draft = null;
        break;
      case "ready":
        this.ready = true;
        this.reconnecting = false;
        if (e.data.has_earlier !== null) this.earlier = e.data.has_earlier ? "idle" : "none";
        this.executingRun = e.data.executing_run;
        break;
      case "run_started":
        this.executingRun = e.data.run_id;
        this.notice = null;
        break;
      case "run_finished":
        this.executingRun = null;
        this.notice = FINISH_NOTICE[e.data.state] ?? null;
        void webSessions.refresh();
        break;
    }
  }
}

import { ApiError, generateTitle, getContext, listJobs, getMessages, markSessionSeen, resumeRun, revokeCompaction, getSession, stopRun } from "../api/client";
import { readStream } from "../api/stream";
import { ProtocolError, jobActive, type ContextView, type JobView, type Message, type Resumable, type RunState, type SessionDetail, type StreamEvent } from "../api/types";
import { SvelteMap, SvelteSet } from "svelte/reactivity";
import { sleep } from "../lib/sleep";
import { sessionsSync } from "./sessions.svelte";

export type Draft = { reasoning: string; text: string; afterId: number };
/** 输入框的回复引用：`label` 显示在输入框内，发送时 `quote` 以引用块前置到正文。 */
export type ComposerReply = { label: string; quote: string };

const RETRY_MIN_MS = 1000;
const RETRY_MAX_MS = 10_000;
/** 连续这么多次取票成功却没等到 `ready`，判为连接失败、停止自动重连。 */
const STREAM_FAILURE_LIMIT = 5;

const FINISH_NOTICE: Partial<Record<RunState, string>> = {
  provider_failed: "模型调用失败，详情见服务日志",
  max_turns: "本次执行已达到模型调用上限，任务可能没有完成",
  interrupted: "本次执行因服务停止而中断",
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
          if (this.info) Object.assign(this.info, { schedule: info.schedule, title: info.title, title_error: info.title_error, title_generating: info.title_generating, pinned_at: info.pinned_at, archived_at: info.archived_at, tag_ids: info.tag_ids });
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
  /** 当前 Root 名下的后台任务。 */
  jobs = $state<JobView[]>([]);
  jobsError = $state<string | null>(null);
  #jobsRefresh: Promise<void> | null = null;
  #jobsDirty = false;

  refreshJobs(): void {
    if (this.info?.source.kind === "schedule") return;
    this.#jobsDirty = true;
    if (this.#jobsRefresh) return;
    this.#jobsRefresh = (async () => {
      while (this.#jobsDirty && !this.#abort.signal.aborted) {
        this.#jobsDirty = false;
        try {
          const jobs = await listJobs(this.id);
          if (this.#abort.signal.aborted) return;
          this.#trackUnseen(jobs);
          this.jobs = jobs;
          this.jobsError = null;
        } catch (error) {
          if (!this.#abort.signal.aborted) this.jobsError = (error as Error).message;
        }
      }
    })().finally(() => { this.#jobsRefresh = null; });
  }
  /** 本页打开后结束、还没展开看过的任务；首次加载时已结束的算看过。 */
  readonly unseenJobs = new SvelteSet<number>();
  #jobsActive: Map<number, boolean> | null = null;

  #trackUnseen(jobs: JobView[]): void {
    const before = this.#jobsActive;
    this.#jobsActive = new Map(jobs.map((j) => [j.id, jobActive(j)]));
    if (before) for (const j of jobs) if (!jobActive(j) && before.get(j.id) !== false) this.unseenJobs.add(j.id);
  }
  /** 任务条是否展开；`jobFocus` 是消息流里点任务号要定位的任务，`seq` 让重复点击也生效。 */
  jobsOpen = $state(false);
  jobFocus = $state<{ id: number; seq: number } | null>(null);

  /** 输入框当前的回复引用。 */
  reply = $state<ComposerReply | null>(null);

  revealJob(id: number): void {
    this.jobsOpen = true;
    this.jobFocus = { id, seq: (this.jobFocus?.seq ?? 0) + 1 };
  }
  draft = $state<Draft | null>(null);
  executingRun = $state<number | null>(null);
  /** 排队等待认领的输入 id：`ready` 给全量，新输入加入，认领时移出。 */
  readonly #queued = new SvelteSet<number>();

  /** 归档会话不再认领输入，不显示排队。 */
  isQueued(id: number): boolean {
    return !this.info?.archived_at && this.#queued.has(id);
  }
  modelRetry = $state<{ attempt: number; until: number } | null>(null);
  /** 已对本次执行发出停止，等它收尾。 */
  stopping = $state<number | null>(null);

  async stop(): Promise<void> {
    const run = this.executingRun;
    if (run === null || this.stopping === run) return;
    this.stopping = run;
    try { await stopRun(this.id, run); }
    catch (error) {
      this.stopping = null;
      if (!(error instanceof ApiError && error.status === 409)) throw error;
    }
  }
  /** 可原地接续的 run；认领或有新输入即失效。 */
  resume = $state<Resumable | null>(null);
  resuming = $state(false);
  resumeError = $state<string | null>(null);

  async resumeRun(): Promise<void> {
    const target = this.resume;
    if (target === null || this.resuming) return;
    this.resuming = true;
    this.resumeError = null;
    try { await resumeRun(this.id, target.run_id); }
    catch (error) {
      if (error instanceof ApiError && error.status === 409) this.resume = null;
      this.resumeError = (error as Error).message;
    }
    finally { this.resuming = false; }
  }
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
  #onVisible = () => this.#reportSeen();

  constructor(readonly id: number) {
    void this.#run();
    document.addEventListener("visibilitychange", this.#onVisible);
  }

  close(): void {
    this.#abort.abort();
    document.removeEventListener("visibilitychange", this.#onVisible);
  }

  /** 已成功上报的已读水位；失败不记，下次时机到了重报同一条。 */
  #seen = 0;
  #seenReport: Promise<void> | null = null;

  /** 页面可见时把已加载的最新消息报为已读；只有 Web 会话计未读。 */
  #reportSeen(): void {
    const source = this.info?.source;
    if (this.#seenReport || source?.kind !== "channel" || source.channel !== "web") return;
    this.#seenReport = (async () => {
      let last = this.lastId;
      while (document.visibilityState === "visible" && last !== null && last > this.#seen && !this.#abort.signal.aborted) {
        try {
          await markSessionSeen(this.id, last);
        } catch {
          return;
        }
        this.#seen = last;
        sessionsSync.markRead(this.id);
        last = this.lastId;
      }
    })().finally(() => { this.#seenReport = null; });
  }

  get lastId(): number | null {
    return this.messages.reduce<number | null>((id, m) => Math.max(id ?? 0, m.id), null);
  }

  /** 按写入游标取更早一页，按位置合入；同一时刻只跑一个。 */
  async loadEarlier(): Promise<void> {
    const first = this.messages[0];
    const before = this.messages.reduce((id, m) => Math.min(id, m.id), Infinity);
    if (!this.ready || !first || this.earlier === "loading" || this.earlier === "none") return;
    this.earlier = "loading";
    try {
      const page = await getMessages(this.id, before);
      if (this.#abort.signal.aborted) return;
      this.#merge(page.messages);
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
    let failures = 0;
    while (!signal.aborted) {
      // 接入后只收到剩余增量，保留旧半截会拼出缺段的文字。
      this.draft = null;
      let ready = false;
      try {
        this.info ??= await getSession(this.id);
        await readStream(this.id, this.lastId, signal, (e) => {
          this.#apply(e);
          if (e.event === "ready") {
            ready = true;
            failures = 0;
            delay = RETRY_MIN_MS;
          }
        });
        if (!ready && !signal.aborted && ++failures >= STREAM_FAILURE_LIMIT) {
          this.fatal = "实时连接反复失败，请刷新页面重试";
          return;
        }
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
      } finally {
        this.modelRetry = null;
      }
      if (signal.aborted) return;
      this.reconnecting = true;
      await sleep(delay, signal);
      delay = Math.min(delay * 2, RETRY_MAX_MS);
    }
  }

  #merge(messages: Message[]): void {
    const merged = new Map(this.messages.map((m) => [m.id, m]));
    for (const message of messages) merged.set(message.id, message);
    this.messages = [...merged.values()].sort((a, b) => a.created_at - b.created_at || a.id - b.id);
  }

  #apply(e: StreamEvent): void {
    if (e.event === "ready" || e.event === "session_changed") this.#refreshTitle();
    if (e.event === "ready" || e.event === "jobs_changed") this.refreshJobs();
    if (e.event === "ready" || e.event === "run_finished" || (e.event === "message" && ["Reply", "Boundary", "Notification", "Command"].includes(e.data.body.kind))) this.#refreshContext();
    switch (e.event) {
      case "message":
        this.#merge([e.data]);
        if (["Reply", "Boundary", "Notification"].includes(e.data.body.kind)) this.modelRetry = null;
        if (e.data.body.kind === "Reply") this.draft = null;
        if (e.data.body.kind === "UserInput") this.notice = null;
        if (e.data.body.kind === "UserInput" || e.data.body.kind === "Command") { this.#queued.add(e.data.id); this.resume = null; }
        break;
      case "text_delta":
        (this.draft ??= { reasoning: "", text: "", afterId: this.lastId ?? 0 }).text += e.data.text;
        break;
      case "reasoning_delta":
        (this.draft ??= { reasoning: "", text: "", afterId: this.lastId ?? 0 }).reasoning += e.data.text;
        break;
      case "model_retry":
        this.modelRetry = { attempt: e.data.attempt, until: Date.now() + e.data.delay_ms };
        break;
      case "draft_discarded":
        this.draft = null;
        break;
      case "ready":
        this.ready = true;
        this.reconnecting = false;
        if (e.data.has_earlier !== null) this.earlier = e.data.has_earlier ? "idle" : "none";
        this.executingRun = e.data.executing_run;
        this.resume = e.data.resume;
        this.#queued.clear();
        for (const id of e.data.queued) this.#queued.add(id);
        break;
      case "inputs_claimed":
        this.executingRun = e.data.run_id;
        this.notice = null;
        this.resume = null;
        this.resumeError = null;
        for (const id of e.data.inputs) this.#queued.delete(id);
        break;
      case "run_finished":
        this.modelRetry = null;
        this.executingRun = null;
        this.stopping = null;
        this.notice = FINISH_NOTICE[e.data.state] ?? null;
        this.resume = e.data.resume;
        break;
    }
    if (e.event === "ready" || e.event === "message") this.#reportSeen();
  }
}

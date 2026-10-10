import { ApiError, getSession } from "../api/client";
import { readStream } from "../api/stream";
import { ProtocolError, type Message, type RunState, type SessionItem, type StreamEvent } from "../api/types";
import { sleep } from "../lib/sleep";
import { webSessions } from "./sessions.svelte";

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
  info = $state<SessionItem | null>(null);
  messages = $state<Message[]>([]);
  draft = $state<Draft | null>(null);
  executingRun = $state<number | null>(null);
  ready = $state(false);
  /** 连接断开、正在重连。 */
  reconnecting = $state(false);
  /** 本页观察到的一轮非正常结束。 */
  notice = $state<string | null>(null);
  /** 无法继续的错误（会话不存在、版本不一致），不再重连。 */
  fatal = $state<string | null>(null);

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

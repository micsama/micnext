<script lang="ts">
  import "./trace.css";
  import { tick } from "svelte";
  import { ApiError, getSession, getTraceCallInput, getTraceMessage, getTracePeers, getTraceRunLogs, getTraceRuns, getTraceSystem } from "../../api/client";
  import type { StoredLogRecord } from "../../api/developer";
  import type { Peers, SystemPromptView, TraceCall, TraceMessage, TraceRun } from "../../api/trace";
  import { cn } from "$lib/utils";
  import { formatDuration, formatTokens } from "../../lib/format";
  import Icon from "../../lib/Icon.svelte";
  import type { Message, SessionDetail } from "../../api/types";
  import { settingsStore } from "../../state/settings.svelte";
  import { router } from "../../state/route.svelte";
  import { traceSessions } from "../../state/trace-sessions.svelte";
  import { attempts, steps, owner, toolsFor, stepOf, bar, usageOf, heat, toolMs, jsonHtml, isLong } from "./trace-presentation";
  import TraceUsage from "./TraceUsage.svelte";
  import TraceContent from "./TraceContent.svelte";
  import TraceSystem from "./TraceSystem.svelte";
  import TraceCard from "./TraceCard.svelte";
  import CopyButton from "../CopyButton.svelte";

  /** target 来自地址：侧栏选会话、浏览器前进后退都经它切换。 */
  let { active, target }: { active: boolean; target: number | null } = $props();
  let issuesOnly = $state(false);
  let folded = $state<Set<number>>(new Set());
  let expanded = $state(false);
  let loading = $state(false);
  let stream: HTMLDivElement;
  const revealSelection = () => void tick().then(() => stream?.querySelector(".selection-active")?.scrollIntoView({ block: "nearest" }));
  const time = (at: number) => new Date(at).toLocaleTimeString("zh-CN", { hour12: false, hour: "2-digit", minute: "2-digit", second: "2-digit" });
  // 运行日志里这两个字段与所在运行重复，不再显示。
  const LOG_CONTEXT = new Set(["session", "run"]);
  const hm = (at: number) => new Date(at).toLocaleTimeString("zh-CN", { hour12: false, hour: "2-digit", minute: "2-digit" });
  const role = (m: TraceMessage) => m.tool ? "工具" : ({ UserInput: "用户", Reply: "AI", Command: "指令", Boundary: "压缩", Notification: "通知", HarnessNote: "系统", Completion: "完成" }[m.kind] ?? m.kind);
  const purpose = (c: TraceCall) => ({ conversation: "模型调用", compaction: "压缩调用", title: "旁路 · 标题" }[c.purpose]);
  const stateName = (r: TraceRun) => ({ completed: "已完成", interrupted: "已中断", stopped: "用户停止", provider_failed: "调用失败", max_turns: "轮数上限" }[r.state] ?? r.state);
  const tokens = (c: TraceCall) => c.usage ? formatTokens(c.usage.input_tokens + c.usage.output_tokens) : "—";
  const toggle = (id: number) => { const next = new Set(folded); next.has(id) ? next.delete(id) : next.add(id); folded = next; };
  const callPreview = (r: TraceRun | null, c: TraceCall) => c.error ?? r?.messages.find((m) => m.id === (c.reply_id ?? c.boundary_id))?.preview ?? c.model;
  const badRun = (r: TraceRun) => runBad(r) || r.calls.some((c) => c.error) || r.messages.some((m) => m.tool?.outcome === "failed");

  type Pick = { run: TraceRun | null; call: TraceCall } | { run: TraceRun; message: TraceMessage };
  type DetailTab = "overview" | "detail" | "input" | "logs";

  let session = $state<number | null>(null);
  let runs = $state.raw<TraceRun[]>([]);
  let titleCalls = $state.raw<TraceCall[]>([]);
  let nextAfter = $state<number | null>(null);
  let peers = $state.raw<Peers>({ models: [], tools: [] });
  let pick = $state.raw<Pick | null>(null);
  let selectedRun = $state.raw<TraceRun | null>(null);
  // 消息概览直接放全文；工具另带发起时的参数。
  let full = $state.raw<{ message: Message; args: string | null } | null>(null);
  // 旁路概览：输入为与生成同源的重建文本，产出为模型原文；旧调用两者皆无。
  let side = $state.raw<{ input: string | null; output: string | null } | null>(null);
  let tab = $state<DetailTab>("overview");
  let error = $state<string | null>(null);
  let loaded = false;
  let pageRequest = 0;
  let detailRequest = 0;
  let paging = $state(false);
  type Entry = { kind: "run"; at: number; run: TraceRun } | { kind: "side"; at: number; call: TraceCall };
  const entries = $derived([
    ...runs.map((run): Entry => ({ kind: "run", at: run.created_at, run })),
    ...titleCalls.filter((c) => nextAfter === null || c.started_at <= (runs.at(-1)?.finished_at ?? 0)).map((call): Entry => ({ kind: "side", at: call.started_at, call })),
  ].sort((a, b) => a.at - b.at));

  // 详情页签按需加载，换选中项即作废。
  let detail = $state.raw<{ key: string; messages: Message[]; system?: string; note?: string } | { key: string; logs: StoredLogRecord[] } | null>(null);
  let detailError = $state<string | null>(null);

  async function guard<T>(f: () => Promise<T>): Promise<T | undefined> {
    try {
      error = null;
      return await f();
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    }
  }

  async function refresh() {
    const [, p] = await Promise.all([traceSessions.refresh(), guard(getTracePeers)]);
    if (p) peers = p;
    const first = session ?? target ?? traceSessions.items[0]?.session;
    if (first !== undefined) await open(first);
  }

  async function open(id: number) {
    remember();
    const request = ++pageRequest;
    ++detailRequest;
    session = id;
    if (target !== id) router.replace(`/s/${id}/trace`);
    systemSelected = false;
    sessionSelected = false;
    selectedRun = null;
    runs = [];
    titleCalls = [];
    nextAfter = null;
    detailError = null;
    pick = null;
    detail = null;
    loading = true;
    const page = await guard(() => getTraceRuns(id, null));
    if (request === pageRequest) loading = false;
    if (!page || request !== pageRequest) return;
    runs = page.runs;
    titleCalls = page.title_calls;
    nextAfter = page.next_after;
    chooseSession();
  }

  async function more() {
    if (session === null || nextAfter === null || paging) return;
    const id = session;
    const request = pageRequest;
    paging = true;
    try {
      const page = await guard(() => getTraceRuns(id, nextAfter));
      if (!page || request !== pageRequest) return;
      runs = [...runs, ...page.runs];
      nextAfter = page.next_after;
    } finally { paging = false; }
  }

  $effect(() => {
    if (active && !loaded) {
      loaded = true;
      void refresh();
    }
  });

  $effect(() => {
    if (active && session !== null && target !== null && target !== session) void open(target);
  });

  const failed = (c: TraceCall) => c.error !== null;
  const runBad = (r: TraceRun) => !["completed", "executing", "stopped"].includes(r.state);

  type Item = { at: number; call?: TraceCall; message?: TraceMessage };
  const items = (r: TraceRun): Item[] =>
    [
      ...steps(r).map((call) => ({ at: attempts(r, call)[0]!.started_at, call })),
      ...r.messages.filter((m) => !r.calls.some((c) => c.reply_id === m.id || c.boundary_id === m.id) && !(m.tool && owner(r, m))).map((message) => ({ at: message.created_at, message })),
    ].sort((a, b) => a.at - b.at);

  // 每次选择都进返回栈，跨会话也能退回。
  type View = { session: number } & ({ pick: Pick; tab: DetailTab } | { run: TraceRun; tab: DetailTab } | { system: true } | { session_info: true });
  let back = $state.raw<View[]>([]);
  const SPARK_RUNS = 60;
  const BACK_LIMIT = 50;
  let restoring = false;
  function here(): View | null {
    if (session === null) return null;
    const at = { session };
    return pick ? { ...at, pick, tab } : selectedRun ? { ...at, run: selectedRun, tab } : systemSelected ? { ...at, system: true } : sessionSelected ? { ...at, session_info: true } : null;
  }
  function remember() {
    const v = here();
    if (!restoring && v) back = [...back.slice(1 - BACK_LIMIT), v];
  }
  async function goBack() {
    const v = back.at(-1);
    if (!v) return;
    const rest = back.slice(0, -1);
    restoring = true;
    try {
      if (v.session !== session) await open(v.session);
      if ("pick" in v) { tab = v.tab; choose(v.pick); }
      else if ("run" in v) { chooseRun(v.run); if (v.tab !== "overview") setTab(v.tab); }
      else if ("system" in v) void chooseSystem();
      else void chooseSession();
    } finally { restoring = false; }
    back = rest;
  }
  const viewName = (v: View) =>
    v.session !== session ? traceSessions.title(v.session)
    : "pick" in v ? ("call" in v.pick ? (v.pick.call.purpose === "title" ? "标题调用" : v.pick.call.purpose === "compaction" ? "压缩调用" : "AI 调用") : v.pick.message.tool?.name ?? "消息")
    : "run" in v ? `轮次 #${runNumber(v.run)}` : "system" in v ? "系统提示" : "会话";

  let systemSelected = $state(false);
  let systemView = $state<SystemPromptView | null>(null);
  let systemRaw = $state(false);
  async function chooseSystem() {
    if (session === null) return;
    remember();
    const request = ++detailRequest;
    selectedRun = null;
    sessionSelected = false;
    systemSelected = true;
    pick = null;
    systemView = null;
    detailError = null;
    try {
      const value = await getTraceSystem(session);
      if (request === detailRequest) systemView = value;
    } catch (e) { if (request === detailRequest) detailError = String(e); }
  }

  function related(p: Pick): Item[] {
    const r = p.run;
    if (!r) return [];
    const c = "call" in p ? p.call : p.message.tool ? owner(r, p.message) : undefined;
    if (!c) return [];
    const source = r.messages.filter((m) => m.kind === "UserInput" && m.created_at <= attempts(r, c)[0]!.started_at).at(-1);
    const tools = toolsFor(r, c).filter((m) => !("message" in p && m.id === p.message.id)).map((message): Item => ({ at: message.created_at, message }));
    if ("message" in p) return [{ at: c.started_at, call: c }, ...tools];
    return source ? [{ at: source.created_at, message: source }, ...tools] : tools;
  }
  const modelMedian = (c: TraceCall) =>
    c.model_id === null ? undefined : peers.models.find((p) => p.purpose === c.purpose && p.model_id === c.model_id);
  const toolPeer = (name: string) => peers.tools.find((p) => p.name === name);
  const runMs = (r: TraceRun) => r.stats.total_ms;
  const tokensOf = (calls: TraceCall[]) => { const u = usageOf(calls); return u.known ? `${formatTokens(u.input + u.output)}${u.unknown ? "+?" : ""}` : "—"; };
  const stopName = (c: TraceCall) => c.stop === null ? "—" : { end_turn: "正常结束", tool_use: "发起工具调用", max_tokens: "达到输出上限", content_filter: "被内容过滤", stopped: "用户停止" }[c.stop];
  const speed = (c: TraceCall) => c.usage && c.first_output_at !== null && c.finished_at > c.first_output_at ? c.usage.output_tokens / ((c.finished_at - c.first_output_at) / 1000) : null;
  const runToolErrors = (r: TraceRun) => r.messages.filter((m) => m.tool?.outcome === "failed").length;

  const pickKey = (p: Pick) => ("call" in p ? `call${p.call.id}` : `msg${p.message.id}`);
  const isPicked = (call?: TraceCall, message?: TraceMessage) =>
    pick !== null && (call ? "call" in pick && pick.call.id === call.id : "message" in pick && pick.message.id === message!.id);

  // 详情只给有长产出的调用；标题旁路只有概览。
  const tabOk = (p: Pick, t: DetailTab) =>
    t === "overview" || (t === "logs" ? p.run !== null : "call" in p && p.call.purpose !== "title" && (t === "input" || p.call.reply_id !== null || p.call.boundary_id !== null));

  function choose(p: Pick) {
    remember();
    systemSelected = false;
    sessionSelected = false;
    selectedRun = null;
    full = null;
    side = null;
    pick = p;
    if (p.run && folded.has(p.run.id)) toggle(p.run.id);
    revealSelection();
    detail = null;
    detailError = null;
    if (!tabOk(p, tab)) tab = "overview";
    void load();
  }

  async function load() {
    if (!pick) return;
    const request = ++detailRequest;
    const p = pick;
    const key = `${pickKey(p)}:${tab}`;
    if (tab === "overview" && "call" in p && p.call.purpose !== "title") return;
    detailError = null;
    try {
      let result: typeof detail;
      if (tab === "overview" && "message" in p) {
        const [message, args] = await Promise.all([getTraceMessage(p.message.id), p.message.tool ? toolArgsOf(p.run, p.message) : null]);
        if (request === detailRequest) full = { message, args };
        return;
      } else if (tab === "overview" && "call" in p) {
        const c = p.call;
        const [input, output] = await Promise.all([
          getTraceCallInput(c.id).then((ms) => ms[0] ?? null, (e) => { if (e instanceof ApiError && e.status === 404) return null; throw e; }),
          c.side_id === null ? null : getTraceMessage(c.side_id),
        ]);
        const text = (m: Message | null) => (m && (m.body.kind === "HarnessNote" || m.body.kind === "SideOutput") ? m.body.text : null);
        if (request === detailRequest) side = { input: text(input), output: text(output) };
        return;
      } else if (tab === "detail") {
        const id = "call" in p ? p.call.reply_id ?? p.call.boundary_id : p.message.id;
        result = { key, messages: id === null ? [] : [await getTraceMessage(id)], note: id === null ? "该调用没有产出会话消息" : undefined };
      } else if (tab === "input") {
        if (!("call" in p) || p.call.purpose === "title") return;
        const [system, messages] = await Promise.all([
          p.call.purpose === "conversation" && session !== null ? getTraceSystem(session).then((v) => v.text) : Promise.resolve(undefined),
          getTraceCallInput(p.call.id),
        ]);
        result = { key, system, messages };
      } else {
        if (!p.run) return;
        result = { key, logs: await getTraceRunLogs(p.run.id) };
      }
      if (request === detailRequest) detail = result;
    } catch (e) {
      if (request === detailRequest) {
        const message = e instanceof Error ? e.message : String(e);
        detailError = message;
      }
    }
  }

  async function toolArgsOf(r: TraceRun, m: TraceMessage) {
    const parent = owner(r, m);
    if (parent?.reply_id == null) throw new Error("未找到发起工具的模型回复");
    const reply = await getTraceMessage(parent.reply_id);
    const block = reply.body.kind === "Reply" ? reply.body.blocks.find((b) => "ToolCall" in b && b.ToolCall.id === m.tool!.call_id) : undefined;
    if (!block || !("ToolCall" in block)) throw new Error("未找到工具调用参数");
    return JSON.stringify(block.ToolCall.args, null, 2);
  }

  let sessionSelected = $state(false);
  let sessionInfo = $state.raw<SessionDetail | null>(null);
  async function chooseSession() {
    if (session === null) return;
    remember();
    const id = session;
    const request = ++detailRequest;
    selectedRun = null;
    systemSelected = false;
    pick = null;
    sessionSelected = true;
    detailError = null;
    if (sessionInfo?.id !== id) sessionInfo = null;
    try {
      const info = await getSession(id);
      if (request === detailRequest) sessionInfo = info;
    } catch (e) { if (request === detailRequest) detailError = String(e); }
  }

  function setTab(t: DetailTab) {
    tab = t;
    if (selectedRun) { void loadRunLogs(); return; }
    detail = null;
    detailError = null;
    void load();
  }

  function chooseRun(run: TraceRun) {
    remember();
    ++detailRequest;
    selectedRun = run;
    revealSelection();
    pick = null;
    systemSelected = false;
    sessionSelected = false;
    detailError = null;
    detail = null;
    tab = "overview";
  }
  async function loadRunLogs() {
    const run = selectedRun;
    const request = ++detailRequest;
    detail = null;
    detailError = null;
    if (!run || tab !== "logs") return;
    try {
      const logs = await getTraceRunLogs(run.id);
      if (request === detailRequest) detail = { key: `run${run.id}`, logs };
    } catch (e) { if (request === detailRequest) detailError = String(e); }
  }

  const tabs: [DetailTab, string][] = [
    ["overview", "概览"],
    ["detail", "详情"],
    ["input", "输入"],
    ["logs", "日志"],
  ];

  const currentSession = $derived(traceSessions.items.find((s) => s.session === session));
  const isBad = (e: Entry) => e.kind === "run" ? badRun(e.run) : e.call.error !== null;
  const visibleEntries = $derived(entries.filter((e) => !issuesOnly || isBad(e)));
  const badCount = $derived(entries.filter(isBad).length);

  const runNumber = (r: TraceRun) => runs.findIndex((run) => run.id === r.id) + 1;
  const selected = "selection-active";
</script>

<svelte:window onkeydown={(e) => { if (e.key === "Escape") expanded = false; }} />

{#snippet durCap(ms: number | null, kind: "model" | "tool" | "run", peer?: { median_ms: number; samples: number })}
  {#if ms === null}<span class="cap c-ghost du">未知</span>
  {:else}{@const h = heat(ms, kind, peer)}<span class="cap lat du" style="--lv:{h.lv.toFixed(2)}" title={h.why}>{formatDuration(ms)}</span>{/if}
{/snippet}

{#snippet callTime(r: TraceRun | null, c: TraceCall)}
  {@const step = stepOf(r, c)}
  {@const ttft = c.first_output_at === null ? null : Math.min(c.first_output_at - c.started_at, step.model)}
  {@const toolWall = step.end - step.modelEnd}
  {@const whole = step.tools.length > 0 || step.calls.length > 1}
  {@const med = modelMedian(c)}
  <div class="metric">
    <div class="m-head"><span class="m-label"><Icon name="clock" class="size-3.5" />{whole ? "本步" : "耗时"}</span><span class="m-val">{step.unknown ? "≥ " : ""}{formatDuration(whole ? step.known : step.model)}</span></div>
    <div class="m-bar"><i class="s-ttft" style="flex:{ttft ?? 0}"></i><i class="s-gen" style="flex:{step.model - (ttft ?? 0)}"></i><i class="s-wait" style="flex:{step.wait}"></i><i class="s-tool" style="flex:{toolWall}"></i></div>
    <div class="m-row" title="全部尝试的模型耗时之和"><i class="dot s-gen"></i><span>模型{#if step.calls.length > 1}<small>{step.calls.length} 次尝试</small>{/if}</span>{@render durCap(step.model, "model", med)}</div>
    <div class="m-row sub"><i class="dot s-ttft"></i><span>首字</span><b>{ttft === null ? "无输出" : formatDuration(ttft)}</b></div>
    {#if step.wait > 0}<div class="m-row"><i class="dot s-wait"></i><span>重试等待</span><b>{formatDuration(step.wait)}</b></div>{/if}
    {#if step.tools.length}<div class="m-row" title={step.unknown ? "有工具终点未知" : "首个工具开始到最后一个结束"}><i class="dot s-tool"></i><span>工具<small>{step.tools.length} 个</small></span><b>{step.unknown ? "未知" : formatDuration(toolWall)}</b></div>{/if}
    {#if med && med.samples >= 2}<p class="m-note">同类中位 {formatDuration(med.median_ms)} · {med.samples} 次</p>{/if}
  </div>
{/snippet}

{#snippet runTime(r: TraceRun)}
  {@const total = runMs(r)}
  {@const { model_ms: model, tool_ms: tool } = r.stats}
  {@const rest = total === null ? 0 : Math.max(0, total - model - tool)}
  <div class="metric">
    <div class="m-head"><span class="m-label"><Icon name="clock" class="size-3.5" />总耗时</span><span class="m-val">{@render durCap(total, "run")}</span></div>
    <div class="m-bar"><i class="s-gen" style="flex:{model}"></i><i class="s-tool" style="flex:{tool}"></i><i class="s-wait" style="flex:{rest}"></i></div>
    <div class="m-row"><i class="dot s-gen"></i><span>模型<small>{r.stats.steps} 步 · {r.stats.requests} 次请求</small></span><b>{formatDuration(model)}</b></div>
    <div class="m-row"><i class="dot s-tool"></i><span>工具<small>{r.stats.tools} 个</small></span><b>{formatDuration(tool)}</b></div>
    {#if total !== null}<div class="m-row" title="排队、重试等待与框架处理"><i class="dot s-wait"></i><span>其他</span><b>{formatDuration(rest)}</b></div>{/if}
  </div>
{/snippet}

{#snippet toolRow(r: TraceRun, m: TraceMessage)}
  {@const t = m.tool!}
  {@const ms = toolMs(m)}
  {@const err = t.outcome === "failed"}
  <button type="button" class={cn("row cols tool", err && "err", isPicked(undefined, m) && selected)} onclick={() => choose({ run: r, message: m })}>
    <span class="sum" title={m.preview}><span class="mono">{t.name}</span>{err ? t.error ?? m.preview : m.preview || "（无文本内容）"}</span>
    <span class="tags">{#if err}<span class="signal c-bad" title={t.error ?? "执行报错"}><Icon name="error" class="size-[13px]" /></span>{:else if ms === null}<span class="signal c-warn" title={t.outcome === "dispatched" ? "已派发，等待执行结果" : "结果未知，恢复未重跑此工具"}><Icon name="unknown" class="size-[13px]" /></span>{/if}</span>
    <span class="track" class:track-empty={ms === null}>{#if ms !== null}<span class="tool-bar" class:bad-bar={err} style={bar(r, t.started_at!, m.created_at, owner(r, m))}></span>{/if}</span>
    <span class="tk"></span>
    {#if ms !== null}{@render durCap(ms, "tool", toolPeer(t.name))}{:else}<span class="du"></span>{/if}
    <time></time>
  </button>
{/snippet}

{#snippet callRow(c: TraceCall, r: TraceRun | null)}
  {@const step = stepOf(r, c)}
  {@const preview = callPreview(r, c)}
  <button type="button" class={cn("row cols", c.purpose === "compaction" ? "k-compact" : "k-ai", r === null && "side", isPicked(c) && selected)} title="{c.model}{c.reasoning_effort ? ` · ${c.reasoning_effort} 档` : ''}" onclick={() => choose({ run: r, call: c })}>
    <span class={cn("role", r === null ? "r-side" : c.purpose === "conversation" ? "r-ai" : c.purpose === "compaction" ? "r-compact" : "r-sys")}>{r === null ? "旁路" : c.purpose === "compaction" ? "压缩" : "AI"}</span>
    <span class="sum" class:bad={failed(c)}>{r === null ? c.error ?? "标题生成" : preview === c.model ? purpose(c) : preview}</span>
    <span class="tags">
      {#if failed(c)}<span class="signal c-bad" title={c.error ?? ""}><Icon name="error" class="size-[13px]" />{#if c.attempt > 1}<b>{c.attempt}</b>{/if}</span>
      {:else}
        {#if c.purpose === "conversation" && !step.tools.length}<span class="signal c-ai" title="直接回复"><Icon name="reply" class="size-[13px]" /></span>{/if}
        {#if c.attempt > 1}<span class="signal c-warn" title="重试 {c.attempt - 1} 次"><Icon name="refresh" class="size-[13px]" /><b>{c.attempt - 1}</b></span>{/if}
      {/if}
    </span>
    <span class="track" class:track-empty={!r || (!step.tools.length && step.calls.length < 2)} title="本步 {step.unknown ? '≥ ' : ''}{formatDuration(step.known)} · 模型 {formatDuration(step.model)}{step.wait ? ` · 重试等待 ${formatDuration(step.wait)}` : ''}（条按段压缩，短段放大）">
      {#if r && (step.tools.length || step.calls.length > 1)}
        {#each step.calls as attempt}<span class="llm-bar" class:fail-bar={failed(attempt)} style={bar(r, attempt.started_at, attempt.finished_at, c)}></span>{/each}
        {#if step.end > step.toolStart}<span class="batch-bar" style={bar(r, step.toolStart, step.end, c)}></span>{/if}
        {#if step.unknown}<span class="unknown-bar" style={`left:${step.known / step.span * 100}%;width:${(1 - step.known / step.span) * 100}%`}></span>{/if}
      {/if}
    </span>
    <span class="cap c-ghost tk" title="本步全部尝试的用量">{tokensOf(step.calls)} tok</span>
    {@render durCap(step.model, "model", modelMedian(c))}
    <time title={time(step.start)}>{hm(step.start)}</time>
  </button>
  {#if r && step.tools.length}<div class="tools">{#each step.tools as m (m.id)}{@render toolRow(r, m)}{/each}</div>{/if}
{/snippet}

{#snippet expandButton()}
  {#if back.length}<button type="button" class="backbtn" title="返回{viewName(back.at(-1)!)}" onclick={goBack}><Icon name="chevron" class="size-3.5" />返回</button>{/if}
  <button type="button" class="iconbtn" title={expanded ? "还原 · Esc" : "放大详情"} aria-label={expanded ? "还原详情" : "放大详情"} onclick={() => expanded = !expanded}><Icon name={expanded ? "x" : "expand"} class="size-3.5" /></button>
{/snippet}

{#snippet logList(logs: StoredLogRecord[])}
  {#if logs.length === 0}<p class="hint">本轮没有警告或错误日志。</p>
  {:else}<ul class="logs">
    {#each logs as l, i (i)}
      <li>
        <p><time class="lv-{l.level}" title={l.level.toUpperCase()}>{time(l.timestamp_ms)}</time>{l.message}</p>
        {#if l.fields.some((f) => !LOG_CONTEXT.has(f.name))}<dl>{#each l.fields.filter((f) => !LOG_CONTEXT.has(f.name)) as f (f.name)}<div><dt>{f.name}</dt><dd>{f.value}</dd></div>{/each}</dl>{/if}
      </li>
    {/each}
  </ul>{/if}
{/snippet}

{#snippet callRel(r: TraceRun, c: TraceCall)}
  <button type="button" class="rel" onclick={() => choose({ run: r, call: c })}><span class={cn("role", c.purpose === "compaction" ? "r-compact" : "r-ai")}>{c.purpose === "compaction" ? "压缩" : "AI"}</span><span class="sum" class:bad={!!c.error}>{c.error ?? (callPreview(r, c) === c.model ? c.model : callPreview(r, c))}</span><span class="tags">{#if c.error}<span class="signal c-bad"><Icon name="error" class="size-[13px]" /></span>{:else}{@render durCap(stepOf(r, c).model, "model", modelMedian(c))}{/if}</span></button>
{/snippet}

<div class="trace-layout">
  <section class="panel">
    <div class="panel-head stream-head">
      <button type="button" class={cn("s-head", sessionSelected && "on")} disabled={session === null} title="查看会话详情" onclick={chooseSession}><h2 class="truncate">{currentSession?.title ?? (session === null ? "Trace" : `会话 ${session}`)}</h2><Icon name="info" class="size-3.5" /></button>
      <div class="caps"><span class="cap">{runs.length} 轮</span><span class="cap">{runs.reduce((n, r) => n + r.calls.length, 0)} 次调用</span><span class="cap">{tokensOf(runs.flatMap((r) => r.calls))} token</span></div>
      <button type="button" class="toggle" class:on={issuesOnly} title="只看失败、中断或含报错的运行" onclick={() => issuesOnly = !issuesOnly}><Icon name="error" class="size-[13px]" />只看异常<b>{badCount}</b></button><button type="button" class="iconbtn sm" title="刷新 Trace" aria-label="刷新 Trace" onclick={refresh}><Icon name="refresh" class="size-3.5" /></button>
    </div>
    <div class="panel-body stream" bind:this={stream}>
      {#if session !== null}<button type="button" class={cn("row cols sys pinned", systemSelected && selected)} onclick={chooseSystem}><span class="role r-sys">系统</span><span class="sum"><b>系统提示</b>当前配置，非历史快照</span></button>{/if}
      {#if error}<div class="alert c-bad"><Icon name="error" class="size-[15px]" /><div>{error}</div></div>{/if}
      {#if loading}<p class="blank">正在加载运行记录…</p>{:else if visibleEntries.length === 0}<p class="blank">{issuesOnly ? "当前已加载的运行没有异常" : "这个会话还没有已结束的运行"}</p>{/if}
      {#each visibleEntries as entry (entry.kind === "run" ? `r${entry.run.id}` : `s${entry.call.id}`)}
        {#if entry.kind === "side"}
          <div class="side-entry">{@render callRow(entry.call, null)}</div>
        {:else}
          {@const r = entry.run}
          {@const isFolded = folded.has(r.id)}
          <article class="turn" class:folded={isFolded}>
            <div class={cn("turn-head", selectedRun?.id === r.id && selected)}>
              <button type="button" class="tw" title={isFolded ? "展开" : "折叠"} aria-label={isFolded ? "展开轮次" : "折叠轮次"} aria-expanded={!isFolded} onclick={() => toggle(r.id)}><Icon name="chevron" class="size-[13px]" /></button>
              <button type="button" class="cols turn-select" onclick={() => chooseRun(r)}>
                <b>轮次 #{runNumber(r)}</b>
                <span class="run-date">{new Date(r.created_at).toLocaleDateString("zh-CN")}</span>
                <span class="tags">
                  {#if isFolded && runToolErrors(r)}<span class="signal c-bad" title="本轮工具报错 {runToolErrors(r)} 个"><Icon name="error" class="size-[13px]" /><b>{runToolErrors(r)}</b></span>{/if}
                  {#if runBad(r)}<span class={cn("signal", r.state === "interrupted" ? "c-warn" : "c-bad")} title={r.state === "interrupted" ? "已中断 · 恢复只补记未知结果，不重跑工具" : stateName(r)}><Icon name={r.state === "interrupted" ? "pause" : "error"} class="size-[13px]" />{stateName(r)}</span>{:else if r.state === "stopped"}<span class="signal c-ghost" title="用户停止"><Icon name="stop" class="size-[13px]" />{stateName(r)}</span>{/if}
                </span>
                <span class="track track-empty"></span>
                <span class="cap c-ghost tk">{r.stats.requests ? `${r.stats.usage_complete ? "" : "≥"}${formatTokens(r.stats.input_tokens + r.stats.output_tokens)}` : "—"} tok</span>
                {@render durCap(runMs(r), "run")}
                <time title={time(r.created_at)}>{hm(r.created_at)}</time>
              </button>
            </div>
            {#if !isFolded}
              <div class="turn-body">
                {#each items(r) as it (it.call ? `c${it.call.id}` : `m${it.message!.id}`)}
                  {#if it.call}{@render callRow(it.call, r)}
                  {:else if it.message!.tool}<div class="tools">{@render toolRow(r, it.message!)}</div>
                  {:else}
                    {@const m = it.message!}
                    <button type="button" class={cn("row cols", m.kind === "UserInput" ? "k-user" : "sys", isPicked(undefined, m) && selected)} onclick={() => choose({ run: r, message: m })}>
                      <span class={cn("role", m.kind === "UserInput" ? "r-user" : "r-sys")}>{role(m)}</span>
                      <span class="sum" title={m.preview}>{m.preview || "（无文本内容）"}</span>
                      <span class="tags"></span><span class="track track-empty"></span><span class="tk"></span><span class="du"></span>
                      <time title={time(m.created_at)}>{hm(m.created_at)}</time>
                    </button>
                  {/if}
                {/each}
              </div>
            {/if}
          </article>
        {/if}
      {/each}
      {#if nextAfter !== null}<button type="button" class="load" disabled={paging} onclick={more}>{paging ? "加载中…" : "↓ 加载后续运行"}</button>{/if}
    </div>
  </section>

  <aside class="panel inspector" class:expanded>
    {#if sessionSelected}
      {@const s = currentSession}
      {@const info = sessionInfo?.id === session ? sessionInfo : null}
      {@const recent = runs.slice(-SPARK_RUNS)}
      {@const longest = Math.max(1, ...recent.map((r) => runMs(r) ?? 0))}
      <div class="d-head"><div><div class="d-type"><span class="role r-turn">会话</span>{#if s?.last_run_at}<time title="最近一次运行">{time(s.last_run_at)}</time>{/if}</div><div class="d-title">{s?.title ?? `会话 ${session}`}<small class="sub">{s?.channel}</small></div></div>{@render expandButton()}</div>
      <div class="tabs"></div>
      <div class="panel-body">
        {#if detailError}<div class="alert c-bad"><Icon name="error" class="size-[15px]" /><div>{detailError}</div></div>{/if}
        <div class="metrics">
          <div class="metric">
            <div class="m-head"><span class="m-label"><Icon name="refresh" class="size-3.5" />轮次</span><span class="m-val">{runs.length}{nextAfter !== null ? "+" : ""}</span></div>
            <div class="spark" title="{runs.length > SPARK_RUNS ? `最近 ${SPARK_RUNS} 轮` : "每轮"}总耗时，点击查看">
              {#each recent as r (r.id)}<button type="button" class={cn(r.state === "completed" ? "ok" : r.state === "interrupted" || r.state === "stopped" ? "warn" : "bad")} style="height:{Math.max(8, (runMs(r) ?? 0) / longest * 100)}%" title="#{runNumber(r)} · {runMs(r) === null ? "未结束" : formatDuration(runMs(r)!)} · {stateName(r)}" aria-label="轮次 {runNumber(r)}" onclick={() => chooseRun(r)}></button>{/each}
            </div>
            <div class="m-row"><i class="dot s-bad"></i><span>失败</span><b>{s?.failed_runs ?? 0}</b></div>
            <div class="m-row"><i class="dot s-warn"></i><span>中断</span><b>{s?.interrupted_runs ?? 0}</b></div>
            <div class="m-row"><i class="dot s-bad"></i><span>工具报错</span><b>{s?.tool_errors ?? 0}</b></div>
            {#if nextAfter !== null}<p class="m-note">图表与用量只含已加载的 {runs.length} 轮</p>{/if}
          </div>
          <TraceUsage calls={[...runs.flatMap((r) => r.calls), ...titleCalls]} />
        </div>
        <details open class="d-sec"><summary class="fold"><Icon name="chevron" class="size-[13px]" />会话信息</summary><div class="sec-body">
          {#if !info}<p class="hint">加载中…</p>
          {:else}
            {@const persona = settingsStore.persona(info.persona_id)}
            {@const model = info.model_id === null ? null : settingsStore.model(info.model_id)}
            <ul class="kv">
              <li><Icon name="user" class="size-3.5" /><span>人设</span><b>{persona?.name ?? "已删除，下轮用默认人设"}</b></li>
              <li title="下一轮使用的模型"><Icon name="sparkles" class="size-3.5" /><span>模型</span><b>{info.model_id === null ? "跟随默认模型" : model?.name ?? "已删除"}{info.reasoning_effort ? ` · ${info.reasoning_effort} 档` : ""}</b></li>
              <li><Icon name="monitor" class="size-3.5" /><span>渠道</span><b>{info.channel}{info.writable ? "" : " · Web 只读"}</b></li>
              <li><Icon name="folder" class="size-3.5" /><span>工作目录</span><b class="mono">{info.workdir}</b></li>
              <li><Icon name="clock" class="size-3.5" /><span>创建</span><b>{new Date(info.created_at).toLocaleString("zh-CN", { hour12: false })}</b></li>
              <li><Icon name="clock" class="size-3.5" /><span>最近活动</span><b>{new Date(info.last_activity_at).toLocaleString("zh-CN", { hour12: false })}</b></li>
              <li><Icon name="hash" class="size-3.5" /><span>编号</span><b class="mono">会话 {info.id}</b></li>
              {#if info.title_error}<li><Icon name="alert" class="size-3.5" /><span>标题</span><b class="bad">{info.title_error}</b></li>{/if}
            </ul>
          {/if}
        </div></details>
      </div>
    {:else if systemSelected}
      <div class="d-head"><div><div class="d-type"><span class="role r-sys">系统</span></div><div class="d-title">系统提示<small class="sub">当前配置 · 非历史快照</small></div></div>{@render expandButton()}</div>
      <nav class="tabs"><button type="button" class="tab" class:on={!systemRaw} onclick={() => systemRaw = false}>分块</button><button type="button" class="tab" class:on={systemRaw} onclick={() => systemRaw = true}>原文</button></nav>
      <div class="panel-body">
        {#if detailError}<div class="alert c-bad"><Icon name="error" class="size-[15px]" /><div>{detailError}</div></div>
        {:else if systemView !== null}<div class="pad"><TraceSystem view={systemView} raw={systemRaw} /></div>
        {:else}<p class="hint pad">加载中…</p>{/if}
      </div>
    {:else if selectedRun}
      {@const r = selectedRun}
      <div class="d-head"><div><div class="d-type"><span class="role r-turn">轮次</span><time>{time(r.created_at)}</time></div><div class="d-title">#{runNumber(r)}<small class="sub">{stateName(r)}</small></div></div>{@render expandButton()}</div>
      <nav class="tabs"><button type="button" class="tab" class:on={tab === "overview"} onclick={() => setTab("overview")}>概览</button><button type="button" class="tab" class:on={tab === "logs"} onclick={() => setTab("logs")}>日志</button></nav>
      <div class="panel-body">
        {#if tab === "logs"}
          <div class="pad">{#if detailError}<p class="bad">{detailError}</p>{:else if !detail}<p class="hint">加载中…</p>{:else if "logs" in detail}{@render logList(detail.logs)}{/if}</div>
        {:else}
          {#if r.state === "interrupted"}<div class="alert c-warn"><Icon name="pause" class="size-[15px]" /><div><b>本轮中断</b><div>恢复只补记结果，不重跑工具</div></div></div>
          {:else if r.state === "stopped"}<div class="alert c-warn"><Icon name="stop" class="size-[15px]" /><div><b>用户停止</b><div>已输出的正文保留，未完成的工具记为已取消</div></div></div>
          {:else if runBad(r)}<div class="alert c-bad"><Icon name="error" class="size-[15px]" /><div><b>{stateName(r)}</b></div></div>{/if}
          <div class="metrics">{@render runTime(r)}<TraceUsage calls={r.calls} /></div>
          <details open class="d-sec"><summary class="fold"><Icon name="chevron" class="size-[13px]" />执行记录</summary><div class="sec-body rels">
            {#each items(r).filter((it) => it.call || it.message?.kind === "UserInput") as it}
              {#if it.call}{@render callRel(r, it.call)}
              {:else}{@const m = it.message!}<button type="button" class="rel" onclick={() => choose({ run: r, message: m })}><span class="role r-user">用户</span><span class="sum">{m.preview}</span><span class="tags"><time>{hm(m.created_at)}</time></span></button>{/if}
            {/each}
          </div></details>
        {/if}
      </div>
    {:else if pick}
      <div class="d-head">
        <div><div class="d-type">
          {#if "call" in pick}<span class={cn("role", pick.call.purpose === "compaction" ? "r-compact" : pick.call.purpose === "title" ? "r-sys" : "r-ai")}>{pick.call.purpose === "conversation" ? "AI" : pick.call.purpose === "compaction" ? "压缩" : "旁路"}</span>
          {:else}<span class={cn("role", pick.message.tool ? "r-tool" : pick.message.kind === "UserInput" ? "r-user" : "r-sys")}>{role(pick.message)}</span>{/if}
          <time>{time("call" in pick ? pick.call.started_at : pick.message.created_at)}</time>
        </div>
        <div class="d-title">{#if "call" in pick}{pick.call.model}{#if pick.call.reasoning_effort}<small class="sub">{pick.call.reasoning_effort} 档</small>{/if}{:else if pick.message.tool}<span class="mono">{pick.message.tool.name}</span>{:else}{role(pick.message)}{/if}</div></div>
        {@render expandButton()}
      </div>
      <nav class="tabs">
        {#each tabs as [id, label] (id)}
          {#if tabOk(pick, id)}
            <button type="button" class="tab" class:on={tab === id} onclick={() => setTab(id)}>{label}</button>
          {/if}
        {/each}
      </nav>
      <div class="panel-body">
        {#if detailError}
          <div class="alert c-bad"><Icon name="error" class="size-[15px]" /><div>{detailError}</div></div>
        {:else if tab === "overview"}
          {#if "call" in pick}
            {@const c = pick.call}
            {@const tries = attempts(pick.run, c)}
            {#if tries.length > 1}
              <div class={cn("alert", tries.at(-1)!.error ? "c-bad" : "c-warn")}><Icon name="refresh" class="size-[15px]" /><div><b>重试 {tries.length - 1} 次后{tries.at(-1)!.error ? "仍失败" : "成功"}</b>
                {#each tries as attempt}<div class="attempt"><span class="n">#{attempt.attempt}</span><span class={cn("cap ic", attempt.error ? "c-bad" : "c-ok")}><Icon name={attempt.error ? "error" : "check"} class="size-3" />{attempt.error ? "失败" : "已完成"}</span><span class="e">{attempt.error ?? ""}</span><span class="cap c-ghost">{attempt.usage ? `${tokens(attempt)} tok` : "用量未知"}</span><span class="cap">{formatDuration(attempt.finished_at - attempt.started_at)}</span></div>{/each}
              </div></div>
            {:else if c.error}<div class="alert c-bad"><Icon name="error" class="size-[15px]" /><div><b>调用失败</b><code>{c.error}</code></div></div>{/if}
            {#if c.stop === "stopped"}<div class="alert c-warn"><Icon name="stop" class="size-[15px]" /><div><b>用户停止</b><div>只保留已收到的正文，用量未知</div></div></div>{/if}
            {#if c.stop === "max_tokens" || c.stop === "content_filter"}<div class="alert c-warn"><Icon name="alert" class="size-[15px]" /><div><b>{c.stop === "max_tokens" ? "达到输出上限" : "内容被上游过滤"}</b>{#if c.stop === "max_tokens"}<div>回复可能不完整</div>{/if}</div></div>{/if}
            <div class="metrics">{@render callTime(pick.run, c)}<TraceUsage calls={tries} /></div>
            {#if c.purpose === "title"}
              {#if !side}<p class="hint">加载中…</p>
              {:else if side.input === null && side.output === null}<p class="hint">旧调用，未记录输入与产出</p>
              {:else}
                <div class="cards">
                  {#if side.input !== null}<TraceCard icon="user" title="输入" tone="user" meta="{side.input.length} 字" long={isLong(side.input)} copy={side.input}><pre>{side.input}</pre></TraceCard>{/if}
                  {#if side.output !== null}<TraceCard icon="text" title="产出" tone={c.error ? "bad" : "ai"} meta={c.error ? "未采用" : ""} copy={side.output}><pre>{side.output}</pre></TraceCard>{/if}
                </div>
              {/if}
            {/if}
            {#if !c.error && pick.run && callPreview(pick.run, c) !== c.model}<details open class="d-sec"><summary class="fold"><Icon name="chevron" class="size-[13px]" />输出摘要</summary><div class="sec-copy"><CopyButton text={callPreview(pick.run, c)} /></div><div class="sec-body"><button type="button" class="excerpt" title="在「详情」中查看全文" onclick={() => setTab("detail")}>{callPreview(pick.run, c)}</button></div></details>{/if}
          {:else}
            {@const m = pick.message}
            {#if m.tool}
              {@const ms = toolMs(m)}
              {#if m.tool.error}<div class="alert c-bad"><Icon name="error" class="size-[15px]" /><div><b>执行报错</b><code>{m.tool.error}</code></div></div>{/if}
              {#if m.tool.outcome === "cancelled" || m.tool.outcome === "dispatched"}<div class="alert c-warn"><Icon name="unknown" class="size-[15px]" /><div><b>{m.tool.outcome === "cancelled" ? "结果未知" : "已派发"}</b><div>{m.tool.outcome === "cancelled" ? "恢复未重跑此工具" : "等待执行结果"}</div></div></div>{/if}
              {#if ms !== null}{@const peer = toolPeer(m.tool.name)}<div class="metrics one"><div class="metric">
                <div class="m-head"><span class="m-label"><Icon name="clock" class="size-3.5" />耗时</span><span class="m-val">{@render durCap(ms, "tool", peer)}</span></div>
                <div class="m-row"><i></i><span>开始</span><b>{time(m.tool.started_at!)}</b></div>
                <div class="m-row"><i></i><span>结束</span><b>{time(m.created_at)}</b></div>
                {#if peer}<p class="m-note">同类中位 {formatDuration(peer.median_ms)} · {peer.samples} 次</p>{/if}
              </div></div>{/if}
            {/if}
            {#if !full}<p class="hint pad">加载中…</p>
            {:else}
              <div class="pad cards">
                {#if full.args !== null}<TraceCard icon="wrench" title="参数 · {m.tool!.name}" tone="tool" copy={full.args} long={isLong(full.args)}><pre class="hl">{@html jsonHtml(full.args)}</pre></TraceCard>{/if}
                <TraceContent message={full.message} />
              </div>
            {/if}
          {/if}
          {#if pick.run}
            {@const r = pick.run}
            <details open class="d-sec"><summary class="fold"><Icon name="chevron" class="size-[13px]" />关联</summary><div class="sec-body rels">
              {#each related(pick) as it}
                {#if it.call}{@render callRel(r, it.call)}
                {:else}{@const m = it.message!}{@const ms = toolMs(m)}<button type="button" class="rel" onclick={() => choose({ run: r, message: m })}><span class={cn("role", m.tool ? "r-tool" : "r-user")}>{role(m)}</span><span class="sum">{#if m.tool}<span class="mono">{m.tool.name}</span>&nbsp;{/if}{m.preview}</span><span class="tags">{#if m.tool?.outcome === "failed"}<span class="signal c-bad" title={m.tool.error ?? ""}><Icon name="error" class="size-[13px]" /></span>{/if}{#if m.tool}{#if ms === null}<span class="signal c-warn" title="结果未知"><Icon name="unknown" class="size-[13px]" /></span>{:else}{@render durCap(ms, "tool", toolPeer(m.tool.name))}{/if}{:else}<time>{hm(m.created_at)}</time>{/if}</span></button>{/if}
              {/each}
              <button type="button" class="rel" onclick={() => chooseRun(r)}><span class="role r-turn">轮次</span><span class="sum">#{runNumber(r)}</span><span class="tags">{@render durCap(runMs(r), "run")}</span></button>
            </div></details>
          {/if}
          {#if "call" in pick}
            {@const c = pick.call}
            {@const v = speed(c)}
            <details open class="d-sec"><summary class="fold"><Icon name="chevron" class="size-[13px]" />调用细节</summary><div class="sec-body"><ul class="kv">
              {#if pick.run === null}<li><Icon name="layers" class="size-3.5" /><span>归属</span><b>会话旁路 · 标题生成</b></li>{/if}
              <li><Icon name="flag" class="size-3.5" /><span>结束</span><b class:bad={c.error !== null}>{c.error ? "调用失败" : stopName(c)}</b></li>
              {#if v !== null}<li title="输出 token ÷ 首字之后的生成时长"><Icon name="zap" class="size-3.5" /><span>输出速度</span><b>{v.toFixed(v < 10 ? 1 : 0)} tok/s</b></li>{/if}
              <li><Icon name="clock" class="size-3.5" /><span>时间</span><b>{time(c.started_at)} → {time(c.finished_at)}</b></li>
              {#if c.usage?.cache_write_tokens}<li><Icon name="layers" class="size-3.5" /><span>缓存写入</span><b>{formatTokens(c.usage.cache_write_tokens)}</b></li>{/if}
              <li><Icon name="hash" class="size-3.5" /><span>编号</span><b class="mono">调用 {c.id}{c.reply_id !== null ? ` · 消息 ${c.reply_id}` : ""}{c.attempt > 1 ? ` · 第 ${c.attempt} 次尝试` : ""}</b></li>
            </ul></div></details>
          {/if}
        {:else if detail === null}
          <p class="hint pad">加载中…</p>
        {:else if "messages" in detail}
          <div class="pad">
            {#if tab === "input"}
              <p class="hint">这次调用发给模型的内容：消息按当时记录还原；系统提示按当前设置生成，可能与当时不同；当时提供给模型的工具列表没有保存。</p>
              {#if detail.system}<button type="button" class="sys-link" onclick={chooseSystem}><Icon name="file" class="size-3.5" /><b>系统提示</b><span>{detail.system.length} 字 · 按当前配置生成</span><em>点击查看</em><Icon name="chevron" class="size-3.5" /></button>{/if}
            {/if}
            {#if detail.note}<p class="hint">{detail.note}</p>{/if}
            <div class="cards">
              {#each detail.messages as message, i (i)}<TraceContent {message} usage={tab === "detail" && "call" in pick ? pick.call.usage : null} bar={tab === "detail"} />{/each}
            </div>
          </div>
        {:else}
          <div class="pad">{@render logList(detail.logs)}</div>
        {/if}
      </div>
    {:else}
      <div class="blank">选择中间的一条记录查看详情<br /><small>耗时、用量、输入输出与关联的工具</small></div>
    {/if}
  </aside>
  {#if expanded}<button type="button" class="detail-backdrop" aria-label="关闭放大详情" onclick={() => expanded = false}></button>{/if}
</div>

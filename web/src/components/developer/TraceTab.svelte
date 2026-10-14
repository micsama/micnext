<script lang="ts">
  import "./trace.css";
  import { tick } from "svelte";
  import { getSession, getTraceCallInput, getTraceMessage, getTracePeers, getTraceRunLogs, getTraceRuns, getTraceSessions, getTraceSystem } from "../../api/client";
  import type { StoredLogRecord } from "../../api/developer";
  import type { Peers, TraceCall, TraceMessage, TraceRun, TraceSession } from "../../api/trace";
  import { cn } from "$lib/utils";
  import { formatDuration, formatTokens } from "../../lib/format";
  import Icon from "../../lib/Icon.svelte";
  import type { Message } from "../../api/types";
  import { attempts, steps, owner, toolsFor, stepOf, bar, usageOf } from "./trace-presentation";
  import TraceUsage from "./TraceUsage.svelte";
  import TraceContent from "./TraceContent.svelte";
  import Markdown from "../Markdown.svelte";
  import CopyButton from "../CopyButton.svelte";

  let { active, initialSession = null }: { active: boolean; initialSession?: number | null } = $props();
  let search = $state("");
  let channelFilter = $state("");
  let issuesOnly = $state(false);
  let folded = $state<Set<number>>(new Set());
  let expanded = $state(false);
  let loading = $state(false);
  let stream: HTMLDivElement;
  const revealSelection = () => void tick().then(() => stream?.querySelector(".selection-active")?.scrollIntoView({ block: "nearest" }));
  const time = (at: number) => new Date(at).toLocaleTimeString("zh-CN", { hour12: false, hour: "2-digit", minute: "2-digit", second: "2-digit" });
  const role = (m: TraceMessage) => m.tool ? "工具" : ({ UserInput: "用户", Reply: "AI", Command: "指令", Boundary: "压缩", Notification: "通知", HarnessNote: "系统", Completion: "完成" }[m.kind] ?? m.kind);
  const purpose = (c: TraceCall) => ({ conversation: "模型调用", compaction: "压缩调用", title: "旁路 · 标题" }[c.purpose]);
  const stateName = (r: TraceRun) => ({ completed: "已完成", interrupted: "已中断", provider_failed: "调用失败", max_turns: "轮数上限" }[r.state] ?? r.state);
  const tokens = (c: TraceCall) => c.usage ? formatTokens(c.usage.input_tokens + c.usage.output_tokens) : "—";
  const toggle = (id: number) => { const next = new Set(folded); next.has(id) ? next.delete(id) : next.add(id); folded = next; };
  const callPreview = (r: TraceRun | null, c: TraceCall) => c.error ?? r?.messages.find((m) => m.id === (c.reply_id ?? c.boundary_id))?.preview ?? c.model;
  const badRun = (r: TraceRun) => runBad(r) || r.calls.some((c) => c.error) || r.messages.some((m) => m.tool?.outcome === "failed");

  type Pick = { run: TraceRun | null; call: TraceCall } | { run: TraceRun; message: TraceMessage };
  type DetailTab = "overview" | "preview" | "input" | "logs";

  let sessions = $state.raw<TraceSession[]>([]);
  let session = $state<number | null>(null);
  let runs = $state.raw<TraceRun[]>([]);
  let titleCalls = $state.raw<TraceCall[]>([]);
  let nextAfter = $state<number | null>(null);
  let peers = $state.raw<Peers>({ models: [], tools: [] });
  let pick = $state.raw<Pick | null>(null);
  let selectedRun = $state.raw<TraceRun | null>(null);
  let toolArgs = $state<string | null>(null);
  let toolArgsError = $state<string | null>(null);
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
    const [s, p] = await Promise.all([guard(getTraceSessions), guard(getTracePeers)]);
    if (s) sessions = s;
    if (p) peers = p;
    if (session !== null) await open(session);
    else if (initialSession !== null) {
      const info = await guard(() => getSession(initialSession));
      if (info) channelFilter = info.channel;
      await open(initialSession);
    } else if (s?.length) await open(s[0]!.session);
  }

  async function open(id: number) {
    const request = ++pageRequest;
    ++detailRequest;
    session = id;
    systemSelected = false;
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
    const first = page.runs[0];
    if (first) chooseRun(first);
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

  const channels = $derived.by(() => {
    const by = new Map<string, TraceSession[]>();
    for (const s of sessions.filter((s) => (!channelFilter || s.channel === channelFilter) && (s.title ?? `会话 ${s.session}`).toLowerCase().includes(search.toLowerCase()))) by.set(s.channel, [...(by.get(s.channel) ?? []), s]);
    return [...by];
  });

  const failed = (c: TraceCall) => c.error !== null;
  const runBad = (r: TraceRun) => r.state !== "completed" && r.state !== "executing";

  type Item = { at: number; call?: TraceCall; message?: TraceMessage };
  const items = (r: TraceRun): Item[] =>
    [
      ...steps(r).map((call) => ({ at: attempts(r, call)[0]!.started_at, call })),
      ...r.messages.filter((m) => !r.calls.some((c) => c.reply_id === m.id || c.boundary_id === m.id)).map((message) => ({ at: message.created_at, message })),
    ].sort((a, b) => a.at - b.at);

  let systemSelected = $state(false);
  let systemText = $state<string | null>(null);
  async function chooseSystem() {
    if (session === null) return;
    const request = ++detailRequest;
    selectedRun = null;
    systemSelected = true;
    pick = null;
    systemText = null;
    detailError = null;
    try {
      const value = await getTraceSystem(session);
      if (request === detailRequest) systemText = value;
    } catch (e) { if (request === detailRequest) detailError = String(e); }
  }

  function related(p: Pick): Item[] {
    const r = p.run;
    if (!r) return [];
    const c = "call" in p ? p.call : p.message.tool ? owner(r, p.message) : undefined;
    if (!c) return [];
    const source = r.messages.filter((m) => m.kind === "UserInput" && m.created_at <= attempts(r, c)[0]!.started_at).at(-1);
    const ids = new Set(toolsFor(r, c).map((m) => m.id));
    return items(r).filter((it) => it.call ? it.call.id === c.id && !("call" in p) : (("call" in p && it.message!.id === source?.id) || ids.has(it.message!.id)) && !("message" in p && it.message!.id === p.message.id));
  }
  const modelMedian = (c: TraceCall) =>
    c.model_id === null ? undefined : peers.models.find((p) => p.purpose === c.purpose && p.model_id === c.model_id);

  const pickKey = (p: Pick) => ("call" in p ? `call${p.call.id}` : `msg${p.message.id}`);
  const isPicked = (call?: TraceCall, message?: TraceMessage) =>
    pick !== null && (call ? "call" in pick && pick.call.id === call.id : "message" in pick && pick.message.id === message!.id);

  function choose(p: Pick) {
    systemSelected = false;
    selectedRun = null;
    toolArgs = null;
    toolArgsError = null;
    pick = p;
    if (p.run && folded.has(p.run.id)) toggle(p.run.id);
    revealSelection();
    detail = null;
    detailError = null;
    if ((tab === "input" && (!("call" in p) || p.call.purpose === "title")) || (tab === "logs" && p.run === null)) tab = "overview";
    void load();
  }

  async function load() {
    if (!pick) return;
    const request = ++detailRequest;
    const p = pick;
    const key = `${pickKey(p)}:${tab}`;
    if (tab === "overview" && !("message" in p && p.message.tool)) return;
    detailError = null;
    try {
      let result: typeof detail;
      if (tab === "overview" && "message" in p && p.message.tool) {
        const parent = owner(p.run, p.message);
        if (parent?.reply_id == null) throw new Error("未找到发起工具的模型回复");
        const message = await getTraceMessage(parent.reply_id);
        const block = message.body.kind === "Reply" ? message.body.blocks.find((b) => "ToolCall" in b && b.ToolCall.id === p.message.tool!.call_id) : undefined;
        if (!block || !("ToolCall" in block)) throw new Error("未找到工具调用参数");
        if (request === detailRequest) toolArgs = JSON.stringify(block.ToolCall.args, null, 2);
        return;
      } else if (tab === "preview") {
        const id = "call" in p ? p.call.reply_id ?? p.call.boundary_id : p.message.id;
        result = { key, messages: id === null ? [] : [await getTraceMessage(id)], note: id === null ? "该调用没有产出会话消息" : undefined };
      } else if (tab === "input") {
        if (!("call" in p) || p.call.purpose === "title") return;
        const [system, messages] = await Promise.all([
          p.call.purpose === "conversation" && session !== null ? getTraceSystem(session) : Promise.resolve(undefined),
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
        if (tab === "overview" && "message" in p && p.message.tool) toolArgsError = message;
        else detailError = message;
      }
    }
  }

  function setTab(t: DetailTab) {
    tab = t;
    if (selectedRun) { void loadRunLogs(); return; }
    detail = null;
    detailError = null;
    void load();
  }

  function chooseRun(run: TraceRun) {
    ++detailRequest;
    selectedRun = run;
    revealSelection();
    pick = null;
    systemSelected = false;
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
  const stepTokens = (r: TraceRun | null, c: TraceCall) => {
    const u = usageOf(attempts(r, c));
    return u.known ? `${formatTokens(u.input + u.output)}${u.unknown ? "+?" : ""}` : "—";
  };

  const tabs: [DetailTab, string][] = [
    ["overview", "概览"],
    ["preview", "预览"],
    ["input", "输入"],
    ["logs", "日志"],
  ];

  const currentSession = $derived(sessions.find((s) => s.session === session));
  const visibleEntries = $derived(entries.filter((e) => !issuesOnly || (e.kind === "run" ? badRun(e.run) : e.call.error !== null)));

  const runNumber = (r: TraceRun) => runs.findIndex((run) => run.id === r.id) + 1;
  const selected = "selection-active";
</script>

<svelte:window onkeydown={(e) => { if (e.key === "Escape") expanded = false; }} />

{#snippet callRow(c: TraceCall, r: TraceRun | null)}
  {@const step = stepOf(r, c)}
  <button type="button" class={cn("trace-row call-row", r && toolsFor(r, c).length > 0 && "step-start", c.purpose === "compaction" && "compact-row", r === null && "side-row", failed(c) && "error-row", isPicked(c) && selected)} onclick={() => choose({ run: r, call: c })}>
    <span class={cn("role", c.purpose === "conversation" ? "role-ai" : "role-compact")}>{r === null ? "旁路" : c.purpose === "compaction" ? "压缩" : "AI"}</span>
    <span class="row-summary" title={callPreview(r, c)}><b>{c.purpose === "title" ? "标题生成" : c.model}</b><span>{callPreview(r, c) === c.model ? (c.reasoning_effort ? `${c.reasoning_effort} 档` : purpose(c)) : callPreview(r, c)}</span></span>
    <span class="signals">{#if failed(c)}<span class="bad" title={c.error ?? ""}><Icon name="alert" class="size-3.5" /></span>{/if}{#if c.purpose === "conversation" && !step.tools.length && !failed(c)}<span class="cap" title="直接回复">回复</span>{/if}{#if c.attempt > 1}<span class="warn" title="第 {c.attempt} 次尝试"><Icon name="refresh" class="size-3" />{c.attempt - 1}</span>{/if}{#if r === null}<span class="cap">标题</span>{/if}</span>
    <span class="track" class:track-empty={!step.tools.length} title="本步 {step.unknown ? '≥ ' : ''}{formatDuration(step.known)} · 模型 {formatDuration(step.model)} · 重试等待 {formatDuration(step.wait)}">
      {#if r && step.tools.length}
        {#each step.calls as attempt}<span class:bad-bar={failed(attempt)} style={bar(r, attempt.started_at, attempt.finished_at, c)}></span>{/each}
        {#if step.end > step.toolStart}<span class="tool-bar" style={bar(r, step.toolStart, step.end, c)}></span>{/if}
        {#if step.unknown}<span class="unknown-bar" style={`left:${step.known / step.span * 100}%;width:${(1 - step.known / step.span) * 100}%`}></span>{/if}
      {/if}
    </span>
    <span class="token" title="本步全部尝试的用量">{stepTokens(r, c)}</span>
    <span class="duration" class:slow={!!modelMedian(c) && step.model > modelMedian(c)!.median_ms * 2}>{formatDuration(step.model)}</span>
    <time>{time(step.start)}</time>
  </button>
{/snippet}

<div class="trace-layout">
  <aside class="trace-panel sessions-panel">
    <div class="panel-heading"><h2>会话</h2><span class="count">{sessions.filter((s) => !channelFilter || s.channel === channelFilter).length}</span><button type="button" class="icon-button ml-auto" title="刷新追踪" aria-label="刷新追踪" onclick={refresh}><Icon name="refresh" class="size-3.5" /></button></div>
    <label class="session-search"><Icon name="file" class="size-3.5" /><input bind:value={search} placeholder="搜索会话" aria-label="搜索会话" /></label>
    <div class="channel-filter"><select bind:value={channelFilter} aria-label="筛选渠道"><option value="">全部渠道</option>{#each [...new Set(sessions.map((s) => s.channel))] as channel}<option value={channel}>{channel === "wechat" ? "微信" : channel}</option>{/each}</select></div>
    <div class="panel-scroll session-list">
      {#each channels as [channel, list] (channel)}
        <div class="group-label"><Icon name={channel === "web" ? "monitor" : "terminal"} class="size-3" /><span>{channel === "wechat" ? "微信" : channel}</span><span class="ml-auto">{list.length}</span></div>
        {#each list as s (s.session)}
          <button type="button" class={cn("session-row", session === s.session && selected)} onclick={() => open(s.session)}>
            <Icon name={s.channel === "web" ? "monitor" : "terminal"} class="size-3.5 shrink-0 text-muted-foreground" />
            <span class="session-title">{s.title ?? `会话 ${s.session}`}</span>
            {#if s.failed_runs + s.interrupted_runs + s.tool_errors > 0}<span class="bad" title="失败 {s.failed_runs} · 中断 {s.interrupted_runs} · 工具报错 {s.tool_errors}"><Icon name="alert" class="size-3" /></span>{/if}
            <time>{s.last_run_at === null ? "" : new Date(s.last_run_at).toLocaleTimeString("zh-CN", { hour: "2-digit", minute: "2-digit", hour12: false })}</time>
          </button>
        {/each}
      {/each}
      {#if channels.length === 0}<p class="empty-state">没有匹配的会话</p>{/if}
    </div>
    <div class="panel-foot">仅展示已结束的执行</div>
  </aside>

  <section class="trace-panel stream-panel">
    <div class="panel-heading stream-heading"><div class="min-w-0"><h2 class="truncate">{currentSession?.title ?? (session === null ? "运行追踪" : `会话 ${session}`)}</h2><p>{currentSession?.channel ?? channelFilter} <span> / </span> {runs.length} 轮运行 · {runs.reduce((n, r) => n + r.calls.length, 0)} 次调用</p></div><button type="button" class="issue-toggle" class:enabled={issuesOnly} onclick={() => issuesOnly = !issuesOnly}><Icon name="alert" class="size-3" />只看异常</button></div>
    <div class="column-legend"><span>执行记录</span><span title="每次模型调用及其工具批次共用时间尺度">本步时间</span><span>Token</span><span>耗时</span><span>时间</span></div>
    <div class="panel-scroll stream-body" bind:this={stream}>
      {#if session !== null}<button type="button" class={cn("system-entry", systemSelected && selected)} onclick={chooseSystem}><span class="role role-system">SYS</span><b>System</b><span>当前系统提示</span><Icon name="chevron" class="ml-auto size-3" /></button>{/if}
      {#if error}<p class="error-banner">{error}</p>{/if}
      {#if loading}<p class="empty-state">正在加载运行记录…</p>{:else if visibleEntries.length === 0}<div class="empty-state"><Icon name="sparkles" class="mx-auto mb-3 size-6 opacity-40" /><p>{issuesOnly ? "当前已加载的运行没有异常" : "这个会话还没有已结束的运行"}</p></div>{/if}
      {#each visibleEntries as entry (entry.kind === "run" ? `r${entry.run.id}` : `s${entry.call.id}`)}
        {#if entry.kind === "side"}
          <div class="side-entry">{@render callRow(entry.call, null)}</div>
        {:else}
          {@const r = entry.run}
          <article class="run-group">
            <div class={cn("run-heading", selectedRun?.id === r.id && selected)}>
              <button type="button" class="fold-chevron" class:open={!folded.has(r.id)} onclick={() => toggle(r.id)} aria-label={folded.has(r.id) ? "展开轮次" : "折叠轮次"} aria-expanded={!folded.has(r.id)}><Icon name="chevron" class="size-3" /></button>
              <button type="button" class="run-select" onclick={() => chooseRun(r)}><b>轮次 {runNumber(r)}</b><span class="run-date">{new Date(r.created_at).toLocaleDateString("zh-CN")}</span><span class={cn("cap ml-auto", runBad(r) ? "cap-bad" : "cap-ok")}>{stateName(r)}</span><span class="duration">{r.finished_at === null ? "未知" : formatDuration(r.finished_at - r.created_at)}</span><time>{time(r.created_at)}</time></button>
            </div>
            {#if !folded.has(r.id)}
              <div class="run-body">
                {#each items(r) as it (it.call ? `c${it.call.id}` : `m${it.message!.id}`)}
                  {#if it.call}{@render callRow(it.call, r)}
                  {:else}
                    {@const m = it.message!}
                    <button type="button" class={cn("trace-row message-row", m.tool ? "tool-row" : m.kind === "UserInput" ? "user-row" : "system-row", m.tool?.outcome === "failed" && "error-row", isPicked(undefined, m) && selected)} onclick={() => choose({ run: r, message: m })}>
                      <span class={cn("role", m.tool ? "role-tool" : m.kind === "UserInput" ? "role-user" : "role-system")}>{m.tool ? "↳" : role(m)}</span>
                      <span class="row-summary">{#if m.tool}<b class="tool-name">{m.tool.name}</b>{/if}<span title={m.preview}>{m.preview || "（无文本内容）"}</span></span>
                      <span class="signals">{#if m.tool?.outcome === "failed"}<span class="bad"><Icon name="alert" class="size-3" /></span>{:else if m.tool?.outcome === "cancelled"}<span class="cap cap-warn">未知</span>{/if}</span>
                      <span class="track" class:track-empty={m.tool?.started_at == null || m.tool.outcome === "cancelled" || m.tool.outcome === "dispatched"}>{#if m.tool?.started_at != null && m.tool.outcome !== "cancelled" && m.tool.outcome !== "dispatched"}<span class="tool-bar" class:bad-bar={m.tool.outcome === "failed"} style={bar(r, m.tool.started_at, m.created_at, owner(r, m))}></span>{/if}</span><span class="token"></span><span class="duration" class:duration-empty={m.tool?.started_at == null || m.tool.outcome === "cancelled" || m.tool.outcome === "dispatched"}>{m.tool?.started_at != null && m.tool.outcome !== "cancelled" && m.tool.outcome !== "dispatched" ? formatDuration(m.created_at - m.tool.started_at) : ""}</span><time>{time(m.created_at)}</time>
                    </button>
                  {/if}
                {/each}
              </div>
            {/if}
          </article>
        {/if}
      {/each}
      {#if nextAfter !== null}<button type="button" class="load-more" disabled={paging} onclick={more}>{paging ? "加载中…" : "加载后续运行"}</button>{/if}
    </div>
  </section>

  <!-- 右：详情 -->
  <aside class="trace-panel inspector" class:expanded>
    {#if systemSelected}
      <div class="detail-heading"><div><span class="role role-system">SYS</span><h2>System</h2><p class="mt-2 text-xs text-muted-foreground">当前配置 · 非历史快照</p></div></div><div class="panel-scroll detail-body">{#if detailError}<p class="text-destructive">{detailError}</p>{:else if systemText !== null}<div class="flex justify-end"><CopyButton text={systemText} /></div><Markdown source={systemText} />{:else}<p>加载中…</p>{/if}</div>
    {:else if selectedRun}
      {@const r = selectedRun}
      <div class="detail-heading"><div class="flex-1"><div class="detail-kind"><span class="role role-system">轮次</span><time>{time(r.created_at)}</time></div><h2>轮次 {runNumber(r)} <small>{stateName(r)}</small></h2></div><button type="button" class="icon-button" aria-label="切换详情大小" onclick={() => expanded = !expanded}><Icon name={expanded ? "x" : "expand"} class="size-3.5" /></button></div>
      <nav class="detail-tabs"><button type="button" class:active={tab === "overview"} onclick={() => setTab("overview")}>概览</button><button type="button" class:active={tab === "logs"} onclick={() => setTab("logs")}>日志</button></nav>
      <div class="panel-scroll detail-body">
        {#if tab === "logs"}
          {#if detailError}<p class="error-banner">{detailError}</p>{:else if !detail}<p class="text-muted-foreground">加载中…</p>{:else if "logs" in detail}{#each detail.logs as log}<p class="run-log"><b>{log.level}</b> {log.message}{#each log.fields as field}<span class="text-muted-foreground"> {field.name}={field.value}</span>{/each}</p>{:else}<p class="text-muted-foreground">本轮没有警告或错误日志。</p>{/each}{/if}
        {:else}
          {#if runBad(r)}<div class="error-banner">{stateName(r)}{r.state === "interrupted" ? " · 恢复只补记未知结果，不重跑工具" : ""}</div>{/if}
          <div class="overview-metrics"><div><span>模型</span><strong>{steps(r).length} 步</strong><small>{r.calls.length} 次请求</small></div><div><span>总耗时</span><strong>{r.finished_at === null ? "未知" : formatDuration(r.finished_at - r.created_at)}</strong><small>含工具与等待</small></div><TraceUsage calls={r.calls} /></div>
          <details open class="detail-section"><summary>执行记录</summary>{#each items(r).filter((it) => it.call || it.message?.kind === "UserInput") as it}{#if it.call}{@const c = it.call}<button type="button" class="related-row" onclick={() => choose({ run: r, call: c })}><span class="role role-ai">{c.purpose === "compaction" ? "压缩" : "AI"}</span><span class="truncate">{c.model}</span><small class:bad={!!c.error}>{c.error ? "失败" : formatDuration(stepOf(r, c).model)}</small></button>{:else}{@const m = it.message!}<button type="button" class="related-row" onclick={() => choose({ run: r, message: m })}><span class="role role-user">用户</span><span class="truncate">{m.preview}</span></button>{/if}{/each}</details>
        {/if}
      </div>
    {:else if pick}
      <div class="detail-heading"><div class="min-w-0 flex-1"><div class="detail-kind"><span class="role" class:role-ai={"call" in pick} class:role-tool={"message" in pick && !!pick.message.tool}>{"call" in pick ? (pick.call.purpose === "conversation" ? "AI" : purpose(pick.call)) : role(pick.message)}</span><time>{time("call" in pick ? pick.call.started_at : pick.message.created_at)}</time></div><h2>{"call" in pick ? pick.call.model : pick.message.tool?.name ?? role(pick.message)}{#if "call" in pick && pick.call.reasoning_effort}<small>{pick.call.reasoning_effort} 档</small>{/if}</h2></div><button type="button" class="icon-button" title={expanded ? "还原 · Esc" : "放大详情"} aria-label={expanded ? "还原详情" : "放大详情"} onclick={() => expanded = !expanded}><Icon name={expanded ? "x" : "expand"} class="size-3.5" /></button></div>
      <nav class="detail-tabs">
        {#each tabs as [id, label] (id)}
          {#if (id !== "input" || ("call" in pick && pick.call.purpose !== "title")) && (id !== "logs" || pick.run !== null)}
            <button
              type="button"
              class:active={tab === id}
              onclick={() => setTab(id)}>{label}</button>
          {/if}
        {/each}
      </nav>
      <div class="panel-scroll detail-body">
        {#if detailError}
          <p class="text-destructive">{detailError}</p>
        {:else if tab === "overview"}
          {#if "call" in pick}
            {@const c = pick.call}
            {@const tries = attempts(pick.run, c)}
            {#if tries.length > 1}<div class="retry-card"><b>重试 {tries.length - 1} 次 · {tries.at(-1)!.error ? "调用失败" : "已成功"}</b>{#each tries as attempt}<div class="attempt-row"><span>#{attempt.attempt}</span><span class:bad={!!attempt.error}>{attempt.error ?? "已完成"}</span><small>{attempt.usage ? tokens(attempt) + " tok" : "用量未知"}</small><span class="duration">{formatDuration(attempt.finished_at - attempt.started_at)}</span></div>{/each}</div>{/if}
            {#if c.error}<div class="error-banner"><Icon name="alert" class="size-4 shrink-0" /><div><b>调用失败</b><p>{c.error}</p></div></div>{/if}
            {#if c.stop === "max_tokens" || c.stop === "content_filter"}<div class="unknown-banner">{c.stop === "max_tokens" ? "达到输出上限，回复可能不完整" : "内容被上游过滤"}</div>{/if}
            {@const step = stepOf(pick.run, c)}
            <div class="overview-metrics">
              <div title="全部尝试的模型耗时之和，不含重试等待与工具"><span>模型耗时</span><strong>{formatDuration(step.model)}</strong><small>{c.first_output_at === null ? "无输出" : `首字 ${formatDuration(c.first_output_at - c.started_at)}`}</small></div>
              {#if step.tools.length || tries.length > 1}<div><span>本步</span><strong>{step.unknown ? "≥ " : ""}{formatDuration(step.known)}</strong><small>{step.unknown ? "工具终点未知" : step.wait ? `重试等待 ${formatDuration(step.wait)}` : `工具 ${formatDuration(step.end - step.modelEnd)}`}</small></div>{/if}
              <TraceUsage calls={tries} />
            </div>
            {#if !c.error && pick.run && callPreview(pick.run, c) !== c.model}<details open class="detail-section"><summary>输出摘要</summary><div class="section-copy"><CopyButton text={callPreview(pick.run, c)} /></div><div class="summary-content"><Markdown source={callPreview(pick.run, c)} /></div><button class="preview-link" type="button" onclick={() => setTab("preview")}>查看完整输出 →</button></details>{/if}
          {:else}
            {@const m = pick.message}
            {#if m.tool?.error}<div class="error-banner"><Icon name="alert" class="size-4 shrink-0" /><div><b>工具执行失败</b><p>{m.tool.error}</p></div></div>{/if}
            {#if m.tool}
              {#if m.tool.outcome === "cancelled" || m.tool.outcome === "dispatched"}<div class="unknown-banner">{m.tool.outcome === "cancelled" ? "结果未知 · 恢复未重跑此工具" : "已派发 · 等待执行结果"}</div>{/if}
              <div class="overview-metrics"><div><span>耗时</span><strong>{m.tool.started_at === null || m.tool.outcome === "cancelled" || m.tool.outcome === "dispatched" ? "未知" : formatDuration(m.created_at - m.tool.started_at)}</strong><small>至 {time(m.created_at)}</small></div></div>
              <details open class="detail-section"><summary>参数</summary>{#if toolArgsError}<p class="text-destructive">{toolArgsError}</p>{:else if toolArgs === null}<p class="text-muted-foreground">加载参数…</p>{:else}<div class="section-copy"><CopyButton text={toolArgs} /></div><pre class="trace-code">{toolArgs}</pre>{/if}</details>
            {/if}
            {#if !m.tool}<details open class="detail-section"><summary>{m.kind === "UserInput" ? "用户输入" : "输出摘要"}</summary><div class="section-copy"><CopyButton text={m.preview} /></div><div class="summary-content"><Markdown source={m.preview || "（无文本内容）"} /></div><button type="button" class="preview-link" onclick={() => setTab("preview")}>查看完整记录 <span>→</span></button></details>{/if}
          {/if}
          {#if pick.run}
            {@const r = pick.run}
            <details open class="detail-section"><summary>关联</summary>
              {#each related(pick) as it}
                {#if it.call}{@const c = it.call}<button type="button" class="related-row" onclick={() => choose({ run: r, call: c })}><span class="role role-ai">{c.purpose === "compaction" ? "压缩" : "AI"}</span><span class="truncate">{c.model}</span><small class:bad={!!c.error}>{c.error ? "失败" : formatDuration(stepOf(r, c).model)}</small></button>
                {:else}{@const m = it.message!}<button type="button" class="related-row" onclick={() => choose({ run: r, message: m })}><span class={cn("role", m.tool ? "role-tool" : "role-user")}>{role(m)}</span><span class="truncate">{m.tool?.name ?? m.preview}</span><small class:bad={m.tool?.outcome === "failed"}>{m.tool ? m.tool.outcome === "failed" ? "报错" : m.tool.started_at === null || m.tool.outcome === "cancelled" || m.tool.outcome === "dispatched" ? "未知" : formatDuration(m.created_at - m.tool.started_at) : time(m.created_at)}</small></button>{/if}
              {/each}
              <button type="button" class="related-row" onclick={() => chooseRun(r)}><span class="role role-system">轮次</span><span>{runNumber(r)}</span><small>{r.finished_at === null ? "未知" : formatDuration(r.finished_at - r.created_at)}</small></button>
            </details>
          {/if}
          {#if "call" in pick}
            {@const c = pick.call}
            {@const med = modelMedian(c)}
            <details class="detail-section"><summary>更多调用信息</summary><dl class="detail-facts">
              {#if pick.run === null}<dt class="text-muted-foreground">归属</dt><dd>会话旁路 · 标题生成</dd>{/if}
              <dt class="text-muted-foreground">模型</dt><dd>{c.model}{c.reasoning_effort ? ` · ${c.reasoning_effort}` : ""}</dd>
              <dt class="text-muted-foreground">耗时</dt>
              <dd>
                {formatDuration(c.finished_at - c.started_at)}
                {#if c.first_output_at !== null}<span class="text-muted-foreground">（首字 {formatDuration(c.first_output_at - c.started_at)}）</span>{/if}
                {#if med}<span class="text-muted-foreground">· 同类中位 {formatDuration(med.median_ms)}（{med.samples} 次）</span>{/if}
              </dd>
              <dt class="text-muted-foreground">结束</dt><dd class={failed(c) ? "text-destructive" : ""}>{c.error ?? c.stop ?? "—"}</dd>
              {#if c.usage}
                <dt class="text-muted-foreground">用量</dt>
                <dd>
                  输入 {formatTokens(c.usage.input_tokens)}{c.usage.cache_read_tokens !== null ? `（缓存读 ${formatTokens(c.usage.cache_read_tokens)}）` : ""}
                  · 输出 {formatTokens(c.usage.output_tokens)}{c.usage.reasoning_tokens !== null ? `（推理 ${formatTokens(c.usage.reasoning_tokens)}）` : ""}
                </dd>
              {/if}
            </dl></details>
          {/if}
        {:else if detail === null}
          <p class="text-muted-foreground">加载中…</p>
        {:else if "messages" in detail}
          {#if tab === "input"}<p class="input-note">按调用事实重建消息；系统提示为当前配置，工具定义不保存历史。</p>{/if}
          {#if detail.note}<p class="text-muted-foreground">{detail.note}</p>{/if}
          {#if detail.system}
            <details class="mb-4 rounded border p-2">
              <summary class="cursor-pointer text-xs text-muted-foreground">系统提示（当前配置，非历史快照）</summary>
              <div class="flex justify-end"><CopyButton text={detail.system} /></div>
              <Markdown source={detail.system} />
            </details>
          {/if}
          {#each detail.messages as message, i (i)}
            <div class="mb-4 border-b pb-3">
              <p class="mb-2 text-xs text-muted-foreground">{message.body.kind}</p>
              <TraceContent body={message.body} />
            </div>
          {/each}
        {:else if detail.logs.length === 0}
          <p class="text-muted-foreground">本轮没有警告或错误日志。</p>
        {:else}
          {#each detail.logs as r, i (i)}
            <div class="mb-2 font-mono text-xs">
              <span class={r.level === "error" ? "text-destructive" : "text-amber-500"}>{r.level.toUpperCase()}</span>
              {r.message}
              {#each r.fields as f (f.name)}<span class="text-muted-foreground"> {f.name}={f.value}</span>{/each}
            </div>
          {/each}
        {/if}
      </div>
    {:else}
      <div class="inspector-empty"><Icon name="sparkles" class="size-7 opacity-30" /><h3>查看执行细节</h3><p>选择一条记录，查看耗时、用量、<br />输入与输出，以及关联的工具。</p></div>
    {/if}
  </aside>
  {#if expanded}<button type="button" class="detail-backdrop" aria-label="关闭放大详情" onclick={() => expanded = false}></button>{/if}
</div>

<script lang="ts">
  import { onMount } from "svelte";
  import { Popover } from "bits-ui";
  import EllipsisIcon from "@lucide/svelte/icons/ellipsis";
  import ArchiveIcon from "@lucide/svelte/icons/archive";
  import { listJobs, archiveSession } from "../api/client";
  import { jobActive } from "../api/types";
  import { confirm } from "../state/confirm.svelte";
  import type { SessionItem } from "../api/types";
  import type { Attachment } from "svelte/attachments";
  import { groupSessions } from "../lib/format";
  import Icon, { type IconName } from "../lib/Icon.svelte";
  import { router } from "../state/route.svelte";
  import { otherSessions, webSessions, type SessionList } from "../state/sessions.svelte";
  import { SvelteMap } from "svelte/reactivity";
  import type { TraceSession } from "../api/trace";
  import { issues, traceSessions } from "../state/trace-sessions.svelte";
  import { theme, type ThemeChoice } from "../state/theme.svelte";
  import { updater } from "../state/update.svelte";
  import { sidebar } from "../state/sidebar.svelte";

  let { activeSession, executingRun }: { activeSession: SessionItem | null; executingRun: number | null } = $props();

  const groups = $derived(groupSessions(webSessions.items));
  const activeId = $derived(router.current.kind === "session" ? router.current.id : null);
  // 同一侧栏两种展示：聊天按日期列会话；Trace 按渠道列有运行的会话，带异常角标。
  const tracing = $derived(router.current.kind === "trace");
  const traceId = $derived(router.current.kind === "trace" ? router.current.id : null);
  // 两种展示之间切换时保留当前会话。
  const modes = $derived([
    { tracing: false, icon: "message", label: "聊天", href: traceId === null ? "/" : `/s/${traceId}` },
    { tracing: true, icon: "sparkles", label: "Trace", href: activeId === null ? "/trace" : `/s/${activeId}/trace` },
  ] satisfies { tracing: boolean; icon: IconName; label: string; href: string }[]);
  let search = $state("");
  let channel = $state("");
  const channelName = (c: string) => (c === "wechat" ? "微信" : c);
  // 会话树：Root 按最近运行排在渠道下，子会话挂在父会话下；搜索命中后代时保留并展开祖先。
  const hit = (s: TraceSession) => traceSessions.title(s.session).toLowerCase().includes(search.trim().toLowerCase());
  const shown = (s: TraceSession): boolean => hit(s) || traceSessions.descendants(s.session).some(hit);
  const traceGroups = $derived.by(() => {
    const by = new Map<string, TraceSession[]>();
    for (const s of traceSessions.items)
      if (s.parent === null && (!channel || s.channel === channel) && shown(s)) by.set(s.channel, [...(by.get(s.channel) ?? []), s]);
    return [...by];
  });
  const toggled = new SvelteMap<number, boolean>();
  const branch = $derived(new Set(traceId === null ? [] : traceSessions.ancestors(traceId).map((s) => s.session)));
  const expanded = (s: TraceSession) =>
    toggled.get(s.session) ?? (branch.has(s.session) || (search.trim() !== "" && traceSessions.descendants(s.session).some(hit)));
  /** 收起时后代的异常：只给一个弱化的关联标记，悬停列出各子 agent。 */
  const hiddenIssues = (s: TraceSession) => {
    const bad = traceSessions.descendants(s.session).filter((d) => issues(d).bad + issues(d).interrupted > 0);
    return bad.length === 0 ? null : {
      count: bad.length,
      tip: bad.map((d) => {
        const { bad, interrupted } = issues(d);
        return `${traceSessions.title(d.session)}：${[bad && `异常 ${bad}`, interrupted && `中断 ${interrupted}`].filter(Boolean).join(" · ")}`;
      }).join("\n"),
    };
  };
  const lastRun = (at: number) => {
    const d = new Date(at);
    return d.toDateString() === new Date().toDateString()
      ? d.toLocaleTimeString("zh-CN", { hour12: false, hour: "2-digit", minute: "2-digit" })
      : `${d.getMonth() + 1}/${d.getDate()}`;
  };
  let othersOpen = $state(false);
  let archiving = $state<number | null>(null);
  let archiveError = $state<string | null>(null);

  async function archive(id: number) {
    archiving = id;
    archiveError = null;
    try {
      const jobs = await listJobs(id);
      const count = jobs.filter(jobActive).length + (activeSession?.id === id && executingRun !== null ? 1 : 0);
      const ok = await confirm.ask({
        title: "归档会话？",
        description: count > 0 ? `还有 ${count} 个任务在运行，归档会终止它们。历史记录会保留，归档后不能继续发送消息。` : "归档会停止会话中的执行与后台任务，保留历史记录；归档后不能继续发送消息。",
        action: "归档",
      });
      if (!ok) { archiving = null; return; }
      await archiveSession(id);
      if (activeId === id) router.go("/");
      location.reload();
    } catch (error) {
      archiveError = (error as Error).message;
      archiving = null;
    }
  }

  const THEME: Record<ThemeChoice, { icon: IconName; label: string }> = {
    system: { icon: "monitor", label: "跟随系统" },
    light: { icon: "sun", label: "浅色" },
    dark: { icon: "moon", label: "深色" },
  };

  onMount(() => void updater.refresh());
  const updatable = $derived(updater.status !== null && updater.status.state !== "unavailable");
  const updating = $derived(updater.running || updater.reconnecting);

  async function quickUpdate() {
    if (updating || (await updater.start())) go("/developer/update");
  }

  function go(path: string) {
    router.go(path);
    sidebar.drawer = false;
  }

  function toggleOthers() {
    othersOpen = !othersOpen;
    if (othersOpen) for (const list of otherSessions) void list.refresh();
  }

  const loadMoreWhenSeen =
    (list: SessionList): Attachment<HTMLElement> =>
    (el) => {
      const io = new IntersectionObserver((entries) => {
        if (entries.some((e) => e.isIntersecting)) void list.loadMore();
      });
      io.observe(el);
      return () => io.disconnect();
    };
</script>

{#snippet traceNode(s: TraceSession)}
  {@const kids = hit(s) ? traceSessions.children(s.session) : traceSessions.children(s.session).filter(shown)}
  {@const open = kids.length > 0 && expanded(s)}
  {@const own = issues(s)}
  {@const hidden = open ? null : hiddenIssues(s)}
  <div class={["flex items-center gap-1 rounded-md pl-1.5 hover:bg-accent", traceId === s.session && "bg-accent font-medium"]}>
    {#if kids.length > 0}
      <button type="button" aria-label={open ? "收起子 agent" : "展开子 agent"} title={open ? "收起子 agent" : "展开子 agent"} onclick={() => toggled.set(s.session, !open)} class="flex size-4 shrink-0 items-center justify-center rounded text-muted-foreground hover:text-foreground">
        <Icon name="chevron" class="size-3.5 transition-transform {open ? 'rotate-90' : ''}" />
      </button>
    {:else}
      <span class="flex size-4 shrink-0 items-center justify-center text-muted-foreground">{#if s.kind === "task"}<Icon name="bot" class="size-3.5" />{/if}</span>
    {/if}
    <a
      href="/s/{s.session}/trace"
      onclick={(e) => (e.preventDefault(), go(`/s/${s.session}/trace`))}
      title={s.kind === "task" ? `子 agent #${s.job} · ${s.label ?? ""}` : undefined}
      class="flex min-w-0 flex-1 items-center gap-2 py-1.5 pr-3 text-sm">
      <span class="min-w-0 flex-1 truncate">{traceSessions.title(s.session)}</span>
      {#if own.bad > 0}<span class="inline-flex items-center gap-0.5 text-xs text-destructive" title="调用失败 {s.failed_runs} 轮 · 工具报错 {s.tool_errors} 个"><Icon name="error" class="size-3" />{own.bad}</span>{/if}
      {#if own.interrupted > 0}<span class="inline-flex items-center gap-0.5 text-xs text-amber-600 dark:text-amber-400" title="中断 {own.interrupted} 轮"><Icon name="pause" class="size-3" />{own.interrupted}</span>{/if}
      {#if hidden}<span class="inline-flex items-center gap-0.5 text-xs text-muted-foreground/70" title={hidden.tip}><Icon name="bot" class="size-3" />{hidden.count}</span>{/if}
      {#if s.last_run_at !== null}<time class="w-9 shrink-0 text-right text-xs text-muted-foreground tabular-nums">{lastRun(s.last_run_at)}</time>{/if}
    </a>
  </div>
  {#if open}
    <div class="ml-3.5 border-l border-border pl-1">
      {#each kids as k (k.session)}{@render traceNode(k)}{/each}
    </div>
  {/if}
{/snippet}

{#snippet list(items: SessionList)}
  {#if items.next}
    <div {@attach loadMoreWhenSeen(items)} class="px-3 py-2 text-xs text-muted-foreground">{items.loading ? "加载中…" : ""}</div>
  {/if}
  {#if items.error}
    <p class="px-3 py-1 text-xs text-destructive">{items.error}</p>
  {/if}
{/snippet}

{#snippet entry(s: SessionItem)}
  {@const id = s.id}
  {@const current = activeSession?.id === id ? activeSession : s}
  <div class="group flex items-center rounded-md hover:bg-accent" class:bg-accent={activeId === id}>
    <a
      href="/s/{id}"
      onclick={(e) => (e.preventDefault(), go(`/s/${id}`))}
      class="min-w-0 flex-1 truncate px-3 py-1.5 text-sm"
      class:font-medium={activeId === id}>{current.title ?? current.preview ?? `会话 ${id}`}</a>
    <Popover.Root>
      <Popover.Trigger aria-label="会话操作" title="会话操作" class="mr-1 flex size-7 shrink-0 items-center justify-center rounded-md text-muted-foreground hover:bg-background/70 hover:text-foreground focus-visible:opacity-100 md:opacity-0 md:group-hover:opacity-100 data-[state=open]:opacity-100">
        <EllipsisIcon class="size-4" />
      </Popover.Trigger>
      <Popover.Portal>
        <Popover.Content side="bottom" align="end" sideOffset={4} collisionPadding={8} class="z-50 w-36 rounded-xl border border-border bg-popover p-1 text-popover-foreground shadow-lg outline-none">
          <button type="button" disabled={archiving !== null} onclick={() => archive(id)} class="flex w-full items-center gap-2 rounded-lg px-3 py-2 text-left text-sm hover:bg-accent disabled:opacity-50">
            <ArchiveIcon class="size-4" />{archiving === id ? "正在归档…" : "归档会话"}
          </button>
        </Popover.Content>
      </Popover.Portal>
    </Popover.Root>
  </div>
{/snippet}

<button
  type="button"
  tabindex="-1"
  class={["fixed inset-0 z-20 bg-black/30 transition-opacity duration-300 motion-reduce:transition-none md:hidden", !sidebar.drawer && "pointer-events-none opacity-0"]}
  aria-label="关闭会话列表"
  onclick={() => (sidebar.drawer = false)}></button>
<!-- 宽屏收起：占位宽度立即归零，面板浮在正文上滑出。 -->
<aside
  class={["fixed inset-y-0 left-0 z-30 w-72 shrink-0 transition-transform duration-300 ease-apple motion-reduce:transition-none md:static md:translate-x-0", !sidebar.drawer && "-translate-x-full", !sidebar.docked && "md:w-0"]}>
  <div class={["relative z-30 flex h-full w-72 flex-col border-r border-border bg-muted duration-300 ease-apple motion-reduce:transition-none md:transition-[transform,visibility]", !sidebar.docked && "md:invisible md:-translate-x-full"]}>
  <div class="flex h-12 shrink-0 items-center gap-2 px-3">
    <span class="flex-1 font-semibold">micnext</span>
    <button type="button" title="收起侧栏" aria-label="收起侧栏" class="rounded p-1.5 text-muted-foreground hover:bg-accent hover:text-foreground" onclick={() => sidebar.toggle()}>
      <Icon name="sidebar" />
    </button>
  </div>
  <nav aria-label="展示" class="relative mx-2 mb-2 grid grid-cols-2 rounded-lg bg-accent p-0.5 text-sm">
    <span class="absolute inset-y-0.5 left-0.5 w-[calc(50%-2px)] rounded-md bg-background shadow-sm transition-transform duration-200" class:translate-x-full={tracing}></span>
    {#each modes as m (m.label)}
      {@const current = m.tracing === tracing}
      <a
        href={m.href}
        aria-current={current ? "page" : undefined}
        onclick={(e) => (e.preventDefault(), current || go(m.href))}
        class={["relative flex items-center justify-center gap-1.5 rounded-md py-1.5 transition-colors", current ? "font-medium text-foreground" : "text-muted-foreground hover:bg-background/50 hover:text-foreground"]}>
        <Icon name={m.icon} />{m.label}
      </a>
    {/each}
  </nav>
  <div class="px-2 pb-2">
    {#if tracing}
      <div class="flex items-center gap-1 rounded-md border border-border bg-background pl-3 focus-within:border-ring">
        <Icon name="search" class="size-3.5 shrink-0 text-muted-foreground" />
        <input bind:value={search} placeholder="搜索会话" aria-label="搜索会话" class="h-9 min-w-0 flex-1 bg-transparent px-1 text-sm outline-none" />
        <select bind:value={channel} aria-label="筛选渠道" class="mr-1 max-w-20 bg-transparent text-xs text-muted-foreground outline-none">
          <option value="">全部</option>
          {#each [...new Set(traceSessions.items.map((s) => s.channel))] as c (c)}<option value={c}>{channelName(c)}</option>{/each}
        </select>
      </div>
    {:else}
      <button
        type="button"
        onclick={() => go("/")}
        class="flex w-full items-center gap-2 rounded-md border border-border bg-background px-3 py-2 text-sm hover:bg-accent">
        <Icon name="plus" />新会话
      </button>
    {/if}
  </div>
  <nav class="min-h-0 flex-1 overflow-y-auto px-2 pb-2">
    {#if tracing}
      {#if traceSessions.error}<p class="px-3 py-1 text-xs text-destructive">{traceSessions.error}</p>{/if}
      {#each traceGroups as [ch, items] (ch)}
        <p class="flex px-3 pt-3 pb-1 text-xs text-muted-foreground"><span class="flex-1">{channelName(ch)}</span>{items.length}</p>
        {#each items as s (s.session)}{@render traceNode(s)}{/each}
      {/each}
      {#if traceSessions.loaded && traceGroups.length === 0}<p class="px-3 pt-3 text-xs text-muted-foreground">没有匹配的会话</p>{/if}
      <p class="px-3 pt-4 text-xs text-muted-foreground">仅列出有已结束运行的会话</p>
    {:else}
    {#if archiveError}<p role="alert" class="px-3 py-2 text-xs text-destructive">归档失败：{archiveError}</p>{/if}
    {#each groups as group (group.label)}
      <p class="px-3 pt-3 pb-1 text-xs text-muted-foreground">{group.label}</p>
      {#each group.items as s (s.id)}
        {@render entry(s)}
      {/each}
    {/each}
    {#if webSessions.loaded && webSessions.items.length === 0}
      <p class="px-3 pt-3 text-xs text-muted-foreground">还没有会话</p>
    {/if}
    {@render list(webSessions)}

    <button
      type="button"
      onclick={toggleOthers}
      class="mt-4 flex w-full items-center gap-1 px-3 py-1 text-xs text-muted-foreground hover:text-foreground">
      <Icon name="chevron" class="size-3 transition-transform {othersOpen ? 'rotate-90' : ''}" />其它渠道
    </button>
    {#if othersOpen}
      {#each otherSessions as other (other.channel)}
        <p class="px-3 pt-2 pb-1 text-xs text-muted-foreground">{other.channel === "wechat" ? "微信" : other.channel}</p>
        {#each other.items as s (s.id)}
          {@render entry(s)}
        {/each}
        {#if other.loaded && other.items.length === 0}
          <p class="px-3 text-xs text-muted-foreground">无</p>
        {/if}
        {@render list(other)}
      {/each}
    {/if}
    {/if}
  </nav>
  <button type="button" onclick={() => go("/schedules")} class="mx-2 mb-2 flex items-center gap-2 rounded-md px-3 py-2 text-sm hover:bg-accent" class:bg-accent={router.current.kind === "schedules"}>
    <Icon name="clock" />定时任务
  </button>
  <div class="flex items-center gap-1 border-t border-border p-2">
    <button
      type="button"
      onclick={() => go("/settings")}
      class="flex flex-1 items-center gap-2 rounded-md px-2 py-1.5 text-sm text-muted-foreground hover:bg-accent hover:text-foreground"
      class:text-foreground={router.current.kind === "settings"}>
      <Icon name="settings" />设置
    </button>
    {#if updatable}
      <button
        type="button"
        title={updating ? "正在更新，查看进度" : "检查并更新"}
        aria-label={updating ? "正在更新，查看进度" : "检查并更新"}
        onclick={quickUpdate}
        class="rounded-md p-2 text-muted-foreground hover:bg-accent hover:text-foreground">
        <Icon name={updating ? "loader" : "refresh"} class={updating ? "size-4 animate-spin" : "size-4"} />
      </button>
    {/if}
    <button
      type="button"
      title="开发者诊断"
      aria-label="开发者诊断"
      onclick={() => go("/developer")}
      class="rounded-md p-2 text-muted-foreground hover:bg-accent hover:text-foreground"
      class:text-foreground={router.current.kind === "developer"}>
      <Icon name="terminal" />
    </button>
    <button
      type="button"
      title="主题：{THEME[theme.choice].label}（点击切换）"
      aria-label="主题：{THEME[theme.choice].label}（点击切换）"
      onclick={() => theme.cycle()}
      class="rounded-md p-2 text-muted-foreground hover:bg-accent hover:text-foreground">
      <Icon name={THEME[theme.choice].icon} />
    </button>
  </div>
  </div>
</aside>

<script lang="ts">
  import type { ChatRunStats, Message, ToolResultOutcome } from "../api/types";
  import { untrack } from "svelte";
  import { fly } from "svelte/transition";
  import { formatTime } from "../lib/format";
  import Icon from "../lib/Icon.svelte";
  import type { SessionView } from "../state/session.svelte";
  import DraftView from "./DraftView.svelte";
  import MessageItem from "./MessageItem.svelte";
  import RunStatsLine from "./RunStatsLine.svelte";

  let { view }: { view: SessionView } = $props();

  const results = $derived.by(() => {
    const m = new Map<string, ToolResultOutcome>();
    for (const { body } of view.messages) if (body.kind === "ToolResult") m.set(body.tool_call_id, body.outcome);
    return m;
  });
  const callIds = $derived.by(() => {
    const s = new Set<string>();
    for (const { body } of view.messages)
      if (body.kind === "Reply") for (const b of body.blocks) if ("ToolCall" in b) s.add(b.ToolCall.id);
    return s;
  });
  const running = $derived(view.executingRun !== null);
  const startedAt = $derived(view.messages[0]?.created_at);
  // `loaded`：组首条 id → 在这组之前刚加载进来的条数，交界落在被跳过或并入的消息上时顺延到下一组。
  const grouped = $derived.by(() => {
    const items: [Message, ...Message[]][] = [];
    const loaded = new Map<number, number>();
    let pending: number | undefined;
    for (const message of view.messages) {
      pending = view.loadedAt.get(message.id) ?? pending;
      if (message.body.kind === "HarnessNote") continue;
      const previous = items.at(-1);
      if (message.body.kind === "Notification" && previous?.[0].body.kind === "Notification") {
        previous.push(message);
      } else {
        items.push([message]);
        if (pending !== undefined) loaded.set(message.id, pending);
        pending = undefined;
      }
    }
    return { items, loaded };
  });
  // 统计行挂在包含 anchor 的组之后：首条 id ≤ anchor 的最后一组。
  const statsAfter = $derived.by(() => {
    const m = new Map<number, ChatRunStats[]>();
    const pending = [...view.runStats.values()].filter((s) => s.requests > 0).sort((a, b) => a.anchor_id - b.anchor_id);
    const items = grouped.items;
    items.forEach((group, i) => {
      const next = items[i + 1]?.[0].id ?? Infinity;
      const here = [];
      while (pending[0] && pending[0].anchor_id < next) {
        const s = pending.shift()!;
        if (s.anchor_id >= group[0].id) here.push(s);
      }
      if (here.length) m.set(group[0].id, here);
    });
    return m;
  });

  // 滚动容器 column-reverse：scrollTop 从底部起算（0 为底，向上为负）。贴底时内容变高仍停在底部，
  // 上方插入旧页、工具结果归并、图片加载都不移动视口，无需写 scrollTop——惯性滚动中写它会被 Safari 打断。
  // 只有不贴底时下方变高（实时消息）才把视口内首条消息放回原位。
  let scroller: HTMLDivElement;
  let content: HTMLDivElement;
  let sentinel: HTMLDivElement;
  let stuck = $state(true);
  let lastTop = 0;
  const STICK_PX = 48;
  let anchor: { el: Element; top: number } | null = null;
  // 校正触发的滚动事件异步到达，可能已晚于下一次 DOM 变化；此时重选会选到未校正的位置。
  let adjustedTop = 1;

  function pickAnchor() {
    const top = scroller.getBoundingClientRect().top;
    for (const el of content.children) {
      if (el === sentinel) continue;
      const rect = el.getBoundingClientRect();
      if (rect.bottom > top) return void (anchor = { el, top: rect.top - top });
    }
    anchor = null;
  }
  function onscroll() {
    const top = scroller.scrollTop;
    if (-top < STICK_PX) stuck = true;
    else if (top < lastTop) stuck = false;
    lastTop = top;
    if (!stuck && top !== adjustedTop) pickAnchor();
  }
  function toBottom() {
    scroller.scrollTop = 0;
  }
  $effect(() => {
    const ro = new ResizeObserver(() => {
      if (stuck) return;
      if (!anchor?.el.isConnected) return pickAnchor();
      const delta = anchor.el.getBoundingClientRect().top - scroller.getBoundingClientRect().top - anchor.top;
      if (Math.abs(delta) < 1) return;
      scroller.scrollTop += delta;
      adjustedTop = scroller.scrollTop;
    });
    ro.observe(content);
    return () => ro.disconnect();
  });

  // 预取通常发生在视口外：列表顶部不可见时才浮出提示，可见时由列表内提示负责，避免两处重复。
  const TOAST_MS = 2500;
  const pill =
    "[grid-area:1/1] inline-flex items-center gap-2 rounded-full border bg-background/90 px-4 py-2 text-sm shadow-lg backdrop-blur";
  const SLOW_MS = 300;
  let sentinelInView = $state(false);
  // 加载超过 SLOW_MS 才浮出，快的不闪。
  let slow = $state(false);
  $effect(() => {
    if (view.earlier !== "loading") return void (slow = false);
    const timer = setTimeout(() => (slow = true), SLOW_MS);
    return () => clearTimeout(timer);
  });
  // 本次加载期间是否看到过列表内的加载提示；看到过就不再弹完成提示。
  let sawInline = false;
  $effect(() => {
    if (view.earlier === "loading") sawInline = untrack(() => sentinelInView);
  });
  $effect(() => {
    if (view.earlier === "loading" && sentinelInView) sawInline = true;
  });
  let toast = $state<{ count: number; reachedStart: boolean } | null>(null);
  $effect(() => {
    const loaded = view.lastLoaded;
    if (!loaded || loaded.count === 0 || sawInline) return;
    toast = loaded;
    const timer = setTimeout(() => (toast = null), TOAST_MS);
    return () => clearTimeout(timer);
  });
  $effect(() => {
    const seen = new IntersectionObserver((entries) => {
      sentinelInView = entries.at(-1)?.isIntersecting ?? false;
    }, { root: scroller });
    seen.observe(sentinel);
    return () => seen.disconnect();
  });

  // 哨兵离顶部三屏内就取更早一页，用户划到顶前通常已接上。IntersectionObserver 只在相交变化时通知，
  // 就绪或一页取完后重新 observe，让它按最新布局再报一次。
  let io: IntersectionObserver;
  $effect(() => {
    io = new IntersectionObserver((entries) => {
      if (entries.at(-1)?.isIntersecting) void view.loadEarlier();
    }, { root: scroller, rootMargin: "300% 0px 0px 0px" });
    return () => io.disconnect();
  });
  $effect(() => {
    if (!view.ready || view.earlier !== "idle") return;
    io.unobserve(sentinel);
    io.observe(sentinel);
  });
</script>

<style>
  /* 交界提示：淡入后停留片刻，再退成一条淡线。 */
  .loaded-divider {
    animation: loaded-divider 4s ease forwards;
  }
  @keyframes loaded-divider {
    0% { opacity: 0; transform: translateY(4px); }
    8%, 70% { opacity: 1; transform: none; }
    100% { opacity: 0.5; }
  }
</style>

<div class="relative min-h-0 flex-1">
  <div bind:this={scroller} {onscroll} class="flex h-full flex-col-reverse overflow-y-auto [overflow-anchor:none]">
    <!-- mb-auto：内容不满一屏时靠上。 -->
    <div bind:this={content} class="mx-auto mb-auto w-full max-w-3xl space-y-4 px-4 py-6">
      <!-- 固定高度：上方变高会往视口外长，切换状态只换内容，提示才始终可见。 -->
      <div bind:this={sentinel} class="flex h-14 items-center justify-center">
        {#if view.earlier === "loading"}
          <div class="flex items-center gap-2 text-sm text-muted-foreground duration-300 animate-in fade-in">
            <Icon name="loader" class="size-5 animate-spin text-primary" />
            正在加载更早的消息
          </div>
        {:else if view.earlier === "none" && startedAt !== undefined}
          <div class="flex w-full items-center gap-3 text-xs text-muted-foreground duration-500 animate-in fade-in">
            <span class="h-px flex-1 bg-border"></span>
            <span class="rounded-full border border-border px-3 py-1">会话开始 · {formatTime(startedAt)}</span>
            <span class="h-px flex-1 bg-border"></span>
          </div>
        {:else if typeof view.earlier === "object"}
          <p class="text-sm text-destructive">
            加载失败：{view.earlier.error}
            <button type="button" class="ml-2 font-medium underline" onclick={() => view.loadEarlier()}>重试</button>
          </p>
        {/if}
      </div>
      {#each grouped.items as messages (messages[0].id)}
        {@const loaded = grouped.loaded.get(messages[0].id)}
        {#if loaded !== undefined}
          <!-- 只动透明度不动高度：交界常在视口内，改高度会推动上方内容。 -->
          <div class="loaded-divider flex items-center gap-3">
            <span class="h-px flex-1 bg-linear-to-r from-transparent to-primary/50"></span>
            <span class="inline-flex items-center gap-1 rounded-full bg-primary/10 px-3 py-1 text-xs font-medium text-primary">
              <Icon name="arrowUp" class="size-3.5" />以上为新加载的 {loaded} 条
            </span>
            <span class="h-px flex-1 bg-linear-to-l from-transparent to-primary/50"></span>
          </div>
        {/if}
        {#if messages[0].body.kind === "Notification"}
          <aside aria-label="系统提示" class="flex gap-2 text-muted-foreground">
            <Icon name="alert" class="mt-1.5 size-3.5 shrink-0" />
            <div class="space-y-1">
              {#each messages as message (message.id)}
                <MessageItem {view} {message} {results} {callIds} {running} />
              {/each}
            </div>
          </aside>
        {:else}
          <MessageItem {view} message={messages[0]} {results} {callIds} {running} />
        {/if}
        {#each statsAfter.get(messages[0].id) ?? [] as stats (stats.run_id)}<RunStatsLine {stats} />{/each}
      {/each}
      {#if view.draft}
        <div><DraftView draft={view.draft} /></div>
      {/if}
      {#if running && !view.draft}
        <div class="flex items-center gap-2 text-sm text-muted-foreground">
          <Icon name="loader" class="size-3.5 animate-spin" />处理中…
        </div>
      {/if}
      {#if view.statsError}<p class="text-sm text-destructive">统计加载失败：{view.statsError}</p>{/if}
      {#if view.contextError}<p class="text-sm text-destructive">上下文状态加载失败：{view.contextError}</p>{/if}
      {#if view.notice}
        <p class="flex items-center gap-2 text-sm text-warning"><Icon name="alert" class="size-4" />{view.notice}</p>
      {/if}
      {#if !view.ready && view.messages.length === 0 && !view.reconnecting}
        <p class="text-center text-sm text-muted-foreground">加载中…</p>
      {/if}
    </div>
  </div>
  <div class="pointer-events-none absolute inset-x-0 top-3 grid justify-items-center">
    {#if sentinelInView}
      <!-- 列表顶部可见，由列表内提示负责。 -->
    {:else if view.earlier === "loading" && slow}
      <div transition:fly={{ y: -12, duration: 200 }} class={[pill, "border-border text-foreground"]}>
        <Icon name="loader" class="size-4 animate-spin text-primary" />正在加载更早的消息
      </div>
    {:else if typeof view.earlier === "object"}
      <div transition:fly={{ y: -12, duration: 200 }} class={[pill, "pointer-events-auto border-destructive/40 text-destructive"]}>
        <Icon name="alert" class="size-4" />加载失败：{view.earlier.error}
        <button type="button" class="ml-1 font-medium underline" onclick={() => view.loadEarlier()}>重试</button>
      </div>
    {:else if toast}
      <div transition:fly={{ y: -12, duration: 200 }} class={[pill, "border-success/40 text-success"]}>
        <Icon name="check" class="size-4" />已加载 {toast.count} 条更早的消息{toast.reachedStart ? "，已到会话开头" : ""}
      </div>
    {/if}
  </div>
  {#if !stuck}
    <button
      type="button"
      class="absolute bottom-3 left-1/2 inline-flex -translate-x-1/2 items-center gap-1.5 rounded-full border border-border bg-background px-4 py-2 text-sm shadow-sm hover:bg-muted"
      onclick={toBottom}>
      <Icon name="arrowDown" class="size-4" />回到底部
    </button>
  {/if}
</div>

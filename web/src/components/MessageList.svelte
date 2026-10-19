<script lang="ts">
  import type { Message, MessageBody, ToolResultOutcome } from "../api/types";
  import { untrack } from "svelte";
  import { fly } from "svelte/transition";
  import { formatTime } from "../lib/format";
  import Icon from "../lib/Icon.svelte";
  import type { SessionView } from "../state/session.svelte";
  import DraftView from "./DraftView.svelte";
  import MessageItem from "./MessageItem.svelte";

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
  /** 不渲染任何内容的消息：已并入调用行的工具结果、只有空思考的回复。 */
  const hidden = (body: MessageBody) =>
    body.kind === "HarnessNote" ||
    (body.kind === "ToolResult" && callIds.has(body.tool_call_id)) ||
    (body.kind === "Reply" && body.blocks.every((b) => "Reasoning" in b && "Visible" in b.Reasoning && !b.Reasoning.Visible.text.trim()));
  /** 同属 agent 一侧的连续条目收紧间距。 */
  const AGENT_SIDE = new Set<MessageBody["kind"]>(["Reply", "ToolResult", "Completion"]);
  const startedAt = $derived(view.messages[0]?.created_at);
  // `loaded`：组首条 id → 在这组之前刚加载进来的条数，交界落在被跳过或并入的消息上时顺延到下一组。
  const grouped = $derived.by(() => {
    const items: [Message, ...Message[]][] = [];
    const loaded = new Map<number, number>();
    let pending: number | undefined;
    for (const message of view.messages) {
      pending = view.loadedAt.get(message.id) ?? pending;
      if (hidden(message.body)) continue;
      const previous = items.at(-1);
      if (message.body.kind === "Notification" && previous?.[0].body.kind === "Notification" &&
          (!view.draft || (message.id > view.draft.afterId) === (previous[0].id > view.draft.afterId))) {
        previous.push(message);
      } else {
        items.push([message]);
        if (pending !== undefined) loaded.set(message.id, pending);
        pending = undefined;
      }
    }
    return { items, loaded };
  });
  const draftIndex = $derived(view.draft ? grouped.items.findIndex((messages) => messages.some((m) => m.id > view.draft!.afterId)) : -1);
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
    <div bind:this={content} class="mx-auto mb-auto w-full max-w-chat space-y-4 px-4 py-6">
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
      {#each grouped.items as messages, i (messages[0].id)}
        {#if view.draft && i === draftIndex}
          <div><DraftView draft={view.draft} /></div>
        {/if}
        {@const loaded = grouped.loaded.get(messages[0].id)}
        {@const prev = grouped.items[i - 1]?.[0].body.kind}
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
          <aside aria-label="系统提示" class="flex flex-col items-center gap-1">
            {#each messages as message (message.id)}
              <MessageItem {view} {message} {results} {running} />
            {/each}
          </aside>
        {:else}
          <!-- 负外边距与上一条的 space-y 合并，实际间距收为 2。 -->
          <div class={loaded === undefined && prev !== undefined && AGENT_SIDE.has(prev) && AGENT_SIDE.has(messages[0].body.kind) ? "-mt-2" : ""}>
            <MessageItem {view} message={messages[0]} {results} {running} />
          </div>
        {/if}
      {/each}
      {#if view.draft && draftIndex === -1}
        <div><DraftView draft={view.draft} /></div>
      {/if}
      {#if running && !view.draft}
        <div class="flex items-center gap-2 text-sm text-muted-foreground">
          <Icon name="loader" class="size-3.5 animate-spin" />处理中…
        </div>
      {/if}
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

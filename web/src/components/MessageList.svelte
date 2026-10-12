<script lang="ts">
  import type { Message, ToolResultOutcome } from "../api/types";
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
  const groups = $derived.by(() => {
    const items: [Message, ...Message[]][] = [];
    for (const message of view.messages) {
      if (message.body.kind === "HarnessNote") continue;
      const previous = items.at(-1);
      if (message.body.kind === "Notification" && previous?.[0].body.kind === "Notification") {
        previous.push(message);
      } else {
        items.push([message]);
      }
    }
    return items;
  });

  // 在底部时跟随新内容；用户上翻后不打扰。滚动事件晚于渲染，内容变高会让程序滚动后的位置看似离底，
  // 只有 scrollTop 变小才算用户上翻。
  let scroller: HTMLDivElement;
  let content: HTMLDivElement;
  let sentinel: HTMLDivElement;
  let stuck = $state(true);
  let lastTop = 0;
  const STICK_PX = 48;
  // 不贴底时，视口内第一条消息及其相对视口顶部的位置；内容变高（插入旧页、工具结果归并、图片加载）后放回原位。
  let anchor: { el: Element; top: number } | null = null;
  // 校正触发的滚动事件异步到达，可能已晚于下一次 DOM 变化；此时重选会选到未校正的位置。
  let adjustedTop = -1;

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
    if (scroller.scrollHeight - top - scroller.clientHeight < STICK_PX) stuck = true;
    else if (top < lastTop) stuck = false;
    lastTop = top;
    if (!stuck && top !== adjustedTop) pickAnchor();
  }
  function toBottom() {
    scroller.scrollTop = scroller.scrollHeight;
  }
  $effect(() => {
    const ro = new ResizeObserver(() => {
      if (stuck) return toBottom();
      if (!anchor?.el.isConnected) return pickAnchor();
      scroller.scrollTop += anchor.el.getBoundingClientRect().top - scroller.getBoundingClientRect().top - anchor.top;
      adjustedTop = scroller.scrollTop;
    });
    ro.observe(content);
    return () => ro.disconnect();
  });

  // 哨兵离顶部一屏内就取更早一页。IntersectionObserver 只在相交变化时通知，
  // 就绪或一页取完后重新 observe，让它按最新布局再报一次。
  let io: IntersectionObserver;
  $effect(() => {
    io = new IntersectionObserver((entries) => {
      if (entries.at(-1)?.isIntersecting) void view.loadEarlier();
    }, { root: scroller, rootMargin: "100% 0px 0px 0px" });
    return () => io.disconnect();
  });
  $effect(() => {
    if (!view.ready || view.earlier !== "idle") return;
    io.unobserve(sentinel);
    io.observe(sentinel);
  });
</script>

<div class="relative min-h-0 flex-1">
  <div bind:this={scroller} {onscroll} class="h-full overflow-y-auto [overflow-anchor:none]">
    <div bind:this={content} class="mx-auto max-w-3xl space-y-4 px-4 py-6">
      <!-- 无提示时抵掉 space-y 给下一条的间距。 -->
      <div bind:this={sentinel} class={["text-center text-xs text-muted-foreground", view.earlier === "idle" || (view.earlier === "none" && view.messages.length === 0) ? "-mb-4" : ""]}>
        {#if view.earlier === "loading"}
          加载更早的消息…
        {:else if view.earlier === "none" && view.messages.length > 0}
          — 会话开始 —
        {:else if typeof view.earlier === "object"}
          <span class="text-destructive">加载失败：{view.earlier.error}</span>
          <button type="button" class="ml-2 underline hover:text-foreground" onclick={() => view.loadEarlier()}>重试</button>
        {/if}
      </div>
      {#each groups as messages (messages[0].id)}
        {#if messages[0].body.kind === "Notification"}
          <aside aria-label="系统提示" class="rounded-xl border border-border bg-muted px-4 py-3">
            <div class="mb-2 flex items-center gap-2 text-xs font-medium text-muted-foreground">
              <Icon name="alert" class="size-3.5 shrink-0" />
              系统提示
            </div>
            <div class="space-y-2 pl-5.5">
              {#each messages as message (message.id)}
                <MessageItem {view} {message} {results} {callIds} {running} />
              {/each}
            </div>
          </aside>
        {:else}
          <MessageItem {view} message={messages[0]} {results} {callIds} {running} />
        {/if}
      {/each}
      {#if view.draft}
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
  {#if !stuck}
    <button
      type="button"
      class="absolute bottom-3 left-1/2 inline-flex -translate-x-1/2 items-center gap-1 rounded-full border border-border bg-background px-3 py-1 text-xs shadow-sm hover:bg-muted"
      onclick={toBottom}>
      <Icon name="arrowDown" class="size-3.5" />回到底部
    </button>
  {/if}
</div>

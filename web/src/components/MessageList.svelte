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

  // 在底部时跟随新内容；用户上翻后不打扰。
  let scroller: HTMLDivElement;
  let content: HTMLDivElement;
  let stuck = $state(true);
  const STICK_PX = 48;

  function onscroll() {
    stuck = scroller.scrollHeight - scroller.scrollTop - scroller.clientHeight < STICK_PX;
  }
  function toBottom() {
    scroller.scrollTop = scroller.scrollHeight;
  }
  $effect(() => {
    const ro = new ResizeObserver(() => {
      if (stuck) toBottom();
    });
    ro.observe(content);
    return () => ro.disconnect();
  });
</script>

<div class="relative min-h-0 flex-1">
  <div bind:this={scroller} {onscroll} class="h-full overflow-y-auto">
    <div bind:this={content} class="mx-auto max-w-3xl space-y-4 px-4 py-6">
      {#each groups as messages (messages[0].id)}
        {#if messages[0].body.kind === "Notification"}
          <aside aria-label="系统提示" class="rounded-xl border border-line bg-panel px-4 py-3">
            <div class="mb-2 flex items-center gap-2 text-xs font-medium text-muted">
              <Icon name="alert" class="size-3.5 shrink-0" />
              系统提示
            </div>
            <div class="space-y-2 pl-5.5">
              {#each messages as message (message.id)}
                <MessageItem {message} {results} {callIds} {running} />
              {/each}
            </div>
          </aside>
        {:else}
          <MessageItem message={messages[0]} {results} {callIds} {running} />
        {/if}
      {/each}
      {#if view.draft}
        <div><DraftView draft={view.draft} /></div>
      {/if}
      {#if running && !view.draft}
        <div class="flex items-center gap-2 text-sm text-muted">
          <Icon name="loader" class="size-3.5 animate-spin" />处理中…
        </div>
      {/if}
      {#if view.notice}
        <p class="flex items-center gap-2 text-sm text-warn"><Icon name="alert" class="size-4" />{view.notice}</p>
      {/if}
      {#if !view.ready && view.messages.length === 0 && !view.reconnecting}
        <p class="text-center text-sm text-muted">加载中…</p>
      {/if}
    </div>
  </div>
  {#if !stuck}
    <button
      type="button"
      class="absolute bottom-3 left-1/2 inline-flex -translate-x-1/2 items-center gap-1 rounded-full border border-line bg-bg px-3 py-1 text-xs shadow-sm hover:bg-panel"
      onclick={toBottom}>
      <Icon name="arrowDown" class="size-3.5" />回到底部
    </button>
  {/if}
</div>

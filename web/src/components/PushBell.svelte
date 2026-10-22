<script lang="ts">
  import { untrack } from "svelte";
  import { Popover } from "bits-ui";
  import BellIcon from "@lucide/svelte/icons/bell";
  import XIcon from "@lucide/svelte/icons/x";
  import type { PushView } from "../api/pushes";
  import { formatAgo, formatTime } from "../lib/format";
  import Icon from "../lib/Icon.svelte";
  import { pushCenter } from "../state/pushes.svelte";
  import { router } from "../state/route.svelte";
  import DeliveryChips from "./DeliveryChips.svelte";

  $effect(() => {
    if (!pushCenter.open) { untrack(() => pushCenter.close()); return; }
    void pushCenter.items;
    void pushCenter.unread;
    untrack(() => void pushCenter.see());
  });

  /** 展开过的长推送。 */
  let expanded = $state<number[]>([]);
  const isLong = (text: string) => text.length > 160 || text.split("\n").length > 4;

  function open(p: PushView) {
    pushCenter.open = false;
    router.go(`/s/${p.source_session_id}`);
  }

  function ask(p: PushView) {
    pushCenter.askIn(p);
    open(p);
  }
</script>

<div class="relative shrink-0">
<Popover.Root bind:open={pushCenter.open}>
  <Popover.Trigger
    aria-label={pushCenter.unread > 0 ? `通知（${pushCenter.unread} 条未读）` : "通知"}
    title="通知"
    class="relative grid size-8 shrink-0 place-items-center rounded-md text-muted-foreground hover:bg-accent hover:text-foreground data-[state=open]:bg-accent">
    {#key pushCenter.arrivals}
      <BellIcon class={["size-4", pushCenter.arrivals > 0 && "bell-ring"]} />
      {#if pushCenter.unread > 0}
        <span class={["absolute top-0.5 right-0 h-4 min-w-4 rounded-full bg-destructive px-1 text-center text-[10px] leading-4 font-semibold text-white tabular-nums ring-2 ring-background", pushCenter.arrivals > 0 && "badge-pop"]}>
          {pushCenter.unread > 99 ? "99+" : pushCenter.unread}
        </span>
      {/if}
    {/key}
  </Popover.Trigger>
  <Popover.Portal>
    <Popover.Content
      side="bottom"
      align="end"
      sideOffset={6}
      collisionPadding={16}
      aria-label="通知"
      class="z-50 flex max-h-[min(36rem,calc(100vh-5rem))] w-[26rem] max-w-[calc(100vw-2rem)] flex-col overflow-hidden rounded-xl border border-border bg-popover text-popover-foreground shadow-lg outline-none">
      <div class="flex items-center gap-2 border-b px-4 py-3">
        <span class="text-sm font-semibold">通知</span>
        {#if pushCenter.unseen.size > 0}<span class="rounded-full bg-primary/10 px-1.5 text-xs font-medium text-primary tabular-nums">{pushCenter.unseen.size} 条新</span>{/if}
      </div>
      <div class="min-h-0 flex-1 overflow-y-auto">
        {#if pushCenter.error}
          <p role="alert" class="px-4 py-3 text-xs text-destructive">{pushCenter.error}</p>
        {/if}
        {#each pushCenter.items as p (p.id)}
          {@const fresh = pushCenter.unseen.has(p.id)}
          {@const long = isLong(p.text)}
          {@const full = expanded.includes(p.id)}
          <article class={["flex gap-3 border-b py-3 pr-4 pl-3 transition-colors last:border-b-0", fresh && "bg-primary/[0.04]"]}>
            <span class={["mt-1.5 size-2 shrink-0 rounded-full", fresh ? "bg-primary" : "bg-transparent"]} aria-label={fresh ? "新" : undefined}></span>
            <div class="min-w-0 flex-1">
              <div class="flex items-baseline gap-2 text-xs">
                <button type="button" class="min-w-0 truncate font-medium hover:underline" title="打开来源会话" onclick={() => open(p)}>{p.source_session_title ?? `会话 #${p.source_session_id}`}</button>
                <time class="ml-auto shrink-0 text-muted-foreground tabular-nums" title={formatTime(p.created_at)}>{formatAgo(p.created_at)}</time>
              </div>
              <p class={["mt-1 text-sm leading-relaxed break-words whitespace-pre-wrap", !fresh && "text-foreground/80", long && !full && "line-clamp-4"]}>{p.text}</p>
              <div class="mt-1.5 flex items-center gap-3 text-xs text-muted-foreground">
                {#if long}
                  <button type="button" class="hover:text-foreground" onclick={() => (expanded = full ? expanded.filter((id) => id !== p.id) : [...expanded, p.id])}>{full ? "收起" : "展开全文"}</button>
                {/if}
                <DeliveryChips deliveries={p.deliveries} />
                <button type="button" class="-mr-1.5 ml-auto inline-flex items-center gap-1 rounded-md px-1.5 py-0.5 hover:bg-accent hover:text-foreground" onclick={() => ask(p)}>
                  <Icon name="reply" class="size-3" />去会话里问
                </button>
              </div>
            </div>
          </article>
        {:else}
          {#if !pushCenter.error}
            <div class="flex flex-col items-center gap-2 px-4 py-10 text-sm text-muted-foreground">
              <BellIcon class="size-6 opacity-40" />还没有通知
            </div>
          {/if}
        {/each}
        {#if pushCenter.hasMore}
          <button type="button" disabled={pushCenter.loadingMore} class="w-full border-t px-4 py-2.5 text-xs text-muted-foreground hover:bg-accent hover:text-foreground disabled:opacity-50" onclick={() => pushCenter.loadMore()}>
            {pushCenter.loadingMore ? "加载中…" : "更早的通知"}
          </button>
        {/if}
      </div>
    </Popover.Content>
  </Popover.Portal>
</Popover.Root>
{#if pushCenter.fresh.length > 0 && !pushCenter.open}
  {@const latest = pushCenter.fresh[0]!}
  {@const count = pushCenter.fresh.length}
  <div role="status" class="push-banner absolute top-full right-0 z-40 mt-3 flex w-[24rem] max-w-[calc(100vw-1.5rem)] items-center overflow-hidden rounded-lg border border-border bg-popover text-popover-foreground shadow-lg">
    <button type="button" aria-label="打开通知" class="flex min-w-0 flex-1 items-center gap-2.5 px-3 py-3 text-left transition-colors hover:bg-accent focus-visible:-outline-offset-2 focus-visible:outline-2 focus-visible:outline-ring" onclick={() => pushCenter.show()}>
      <span class="size-1.5 shrink-0 rounded-full bg-primary"></span>
      <span class="shrink-0 text-xs font-medium">{count > 1 ? `${count} 条新通知` : "新通知"}</span>
      <span class="min-w-0 truncate text-xs text-muted-foreground">{latest.text}</span>
    </button>
    <button type="button" aria-label="收起通知提醒" title="收起" class="mr-1.5 grid size-7 shrink-0 place-items-center rounded-md text-muted-foreground transition-colors hover:bg-accent hover:text-foreground focus-visible:outline-2 focus-visible:outline-ring" onclick={() => pushCenter.dismiss()}><XIcon class="size-3.5" /></button>
  </div>
{/if}
</div>

<style>
  .push-banner { animation: push-drop 220ms ease-out; }
  :global(.bell-ring) { transform-origin: 50% 2px; animation: bell-ring 900ms ease-in-out; }
  .badge-pop { animation: badge-pop 400ms ease-out; }
  @keyframes push-drop {
    from { opacity: 0; transform: translateY(-6px); }
    to { opacity: 1; transform: translateY(0); }
  }
  @keyframes -global-bell-ring {
    0%, 100% { transform: rotate(0); }
    15% { transform: rotate(16deg); }
    30% { transform: rotate(-14deg); }
    45% { transform: rotate(10deg); }
    60% { transform: rotate(-6deg); }
    75% { transform: rotate(3deg); }
  }
  @keyframes badge-pop {
    0% { transform: scale(.6); }
    60% { transform: scale(1.25); }
    100% { transform: scale(1); }
  }
  @media (prefers-reduced-motion: reduce) {
    .push-banner, .badge-pop, :global(.bell-ring) { animation: none; }
  }
</style>

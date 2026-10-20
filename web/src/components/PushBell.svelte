<script lang="ts">
  import { Popover } from "bits-ui";
  import BellIcon from "@lucide/svelte/icons/bell";
  import XIcon from "@lucide/svelte/icons/x";
  import { channelLabel, type PushDelivery, type PushView } from "../api/pushes";
  import { formatTime } from "../lib/format";
  import Icon from "../lib/Icon.svelte";
  import { pushCenter } from "../state/pushes.svelte";
  import { router } from "../state/route.svelte";

  // 展开期间看到的未读（含展开时新到的）即标已读。
  $effect(() => {
    if (!pushCenter.open) return;
    pushCenter.dismiss();
    const ids = pushCenter.items.filter((p) => p.read_at === null).map((p) => p.id);
    void pushCenter.markRead(ids);
  });

  function ask(p: PushView) {
    pushCenter.askIn(p);
    pushCenter.open = false;
    router.go(`/s/${p.source_session_id}`);
  }

  function deliveryTitle(d: PushDelivery): string {
    const name = channelLabel(d.channel);
    switch (d.state.kind) {
      case "pending": return `${name}：投递中`;
      case "delivered": return `${name}：${formatTime(d.state.at)} 送达`;
      case "failed": return `${name}：未送达（${d.state.reason}）`;
    }
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
      class="z-50 flex max-h-[min(32rem,calc(100vh-5rem))] w-[24rem] max-w-[calc(100vw-2rem)] flex-col rounded-xl border border-border bg-popover text-popover-foreground shadow-lg outline-none">
      <div class="border-b px-4 py-2.5 text-sm font-medium">通知</div>
      <div class="min-h-0 flex-1 overflow-y-auto">
        {#if pushCenter.error}
          <p role="alert" class="px-4 py-3 text-xs text-destructive">{pushCenter.error}</p>
        {/if}
        {#each pushCenter.items as p (p.id)}
          <article class="space-y-1.5 border-b px-4 py-3 last:border-b-0">
            <div class="flex items-center gap-2 text-xs text-muted-foreground">
              <span class="min-w-0 flex-1 truncate">{p.source_schedule ? `定时任务 · ${p.source_schedule}` : (p.source_session_title ?? `会话 #${p.source_session_id}`)}</span>
              {#each p.deliveries as d (d.channel)}
                <span title={deliveryTitle(d)} class={["shrink-0", d.state.kind === "failed" && "text-destructive"]}>
                  {channelLabel(d.channel)}{#if d.state.kind === "failed"}<Icon name="alert" class="ml-0.5 inline size-3" />{/if}
                </span>
              {/each}
              <span class="shrink-0 tabular-nums">{formatTime(p.created_at)}</span>
            </div>
            <p class="text-sm break-words whitespace-pre-wrap">{p.text}</p>
            <button type="button" class="inline-flex items-center gap-1 rounded-md px-1.5 py-0.5 -ml-1.5 text-xs text-muted-foreground hover:bg-accent hover:text-foreground" onclick={() => ask(p)}>
              <Icon name="reply" class="size-3" />去会话里问
            </button>
          </article>
        {:else}
          {#if !pushCenter.error}<p class="px-4 py-6 text-center text-sm text-muted-foreground">还没有推送</p>{/if}
        {/each}
        {#if pushCenter.hasMore}
          <button type="button" disabled={pushCenter.loadingMore} class="w-full px-4 py-2 text-xs text-muted-foreground hover:bg-accent disabled:opacity-50" onclick={() => pushCenter.loadMore()}>
            {pushCenter.loadingMore ? "加载中…" : "更早的推送"}
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
    <button type="button" aria-label="打开通知看板" class="flex min-w-0 flex-1 items-center gap-2.5 px-3 py-3 text-left transition-colors hover:bg-accent focus-visible:-outline-offset-2 focus-visible:outline-2 focus-visible:outline-ring" onclick={() => pushCenter.show()}>
      <span class="size-1.5 shrink-0 rounded-full bg-primary"></span>
      <span class="shrink-0 text-xs font-medium">{count > 1 ? `${count} 条新推送` : "新推送"}</span>
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

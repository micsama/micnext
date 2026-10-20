<script lang="ts">
  import { Popover } from "bits-ui";
  import BellIcon from "@lucide/svelte/icons/bell";
  import { channelLabel, type PushDelivery, type PushView } from "../api/pushes";
  import { formatTime } from "../lib/format";
  import Icon from "../lib/Icon.svelte";
  import { pushCenter } from "../state/pushes.svelte";
  import { router } from "../state/route.svelte";

  let open = $state(false);

  const toWeb = (p: PushView) => p.deliveries.some((d) => d.channel === "web");

  // 展开期间看到的未读（含展开时新到的）即标已读。
  $effect(() => {
    if (!open) return;
    const ids = pushCenter.items.filter((p) => p.read_at === null && toWeb(p)).map((p) => p.id);
    void pushCenter.markRead(ids);
  });

  function ask(p: PushView) {
    pushCenter.askIn(p);
    open = false;
    router.go(`/s/${p.source_session_id}`);
  }

  function deliveryTitle(d: PushDelivery): string {
    const name = channelLabel(d.channel);
    switch (d.state.kind) {
      case "shown": return `${name}：已显示`;
      case "pending": return `${name}：投递中`;
      case "delivered": return `${name}：${formatTime(d.state.at)} 送达`;
      case "failed": return `${name}：未送达（${d.state.reason}）`;
    }
  }
</script>

<Popover.Root bind:open>
  <Popover.Trigger
    aria-label={pushCenter.unread > 0 ? `通知（${pushCenter.unread} 条未读）` : "通知"}
    title="通知"
    class="relative grid size-8 shrink-0 place-items-center rounded-md text-muted-foreground hover:bg-accent hover:text-foreground data-[state=open]:bg-accent">
    <BellIcon class="size-4" />
    {#if pushCenter.unread > 0}
      <span class="absolute top-1.5 right-1.5 size-2 rounded-full bg-destructive ring-2 ring-background"></span>
    {/if}
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
              <span class="min-w-0 flex-1 truncate">{p.source_session_title ?? `会话 #${p.source_session_id}`}</span>
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

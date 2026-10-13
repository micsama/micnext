<script lang="ts">
  import { onMount } from "svelte";
  import { Popover } from "bits-ui";
  import EllipsisIcon from "@lucide/svelte/icons/ellipsis";
  import ArchiveIcon from "@lucide/svelte/icons/archive";
  import { archiveSession } from "../api/client";
  import type { SessionItem } from "../api/types";
  import type { Attachment } from "svelte/attachments";
  import { groupSessions } from "../lib/format";
  import Icon, { type IconName } from "../lib/Icon.svelte";
  import { router } from "../state/route.svelte";
  import { otherSessions, webSessions, type SessionList } from "../state/sessions.svelte";
  import { theme, type ThemeChoice } from "../state/theme.svelte";
  import { updater } from "../state/update.svelte";

  let { open, onclose, activeSession }: { open: boolean; onclose: () => void; activeSession: SessionItem | null } = $props();

  const groups = $derived(groupSessions(webSessions.items));
  const activeId = $derived(router.current.kind === "session" ? router.current.id : null);
  let othersOpen = $state(false);
  let archiving = $state<number | null>(null);
  let archiveError = $state<string | null>(null);

  async function archive(id: number) {
    archiving = id;
    archiveError = null;
    try {
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
    onclose();
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

{#if open}
  <button type="button" class="fixed inset-0 z-20 bg-black/30 md:hidden" aria-label="关闭会话列表" onclick={onclose}
  ></button>
{/if}
<aside
  class="fixed inset-y-0 left-0 z-30 flex w-72 flex-col border-r border-border bg-muted transition-transform md:static md:translate-x-0"
  class:-translate-x-full={!open}>
  <div class="flex h-12 shrink-0 items-center gap-2 px-3">
    <span class="flex-1 font-semibold">micnext</span>
    <button type="button" class="rounded p-1.5 hover:bg-accent md:hidden" aria-label="关闭" onclick={onclose}>
      <Icon name="x" />
    </button>
  </div>
  <div class="px-2 pb-2">
    <button
      type="button"
      onclick={() => go("/")}
      class="flex w-full items-center gap-2 rounded-md border border-border bg-background px-3 py-2 text-sm hover:bg-accent">
      <Icon name="plus" />新会话
    </button>
  </div>
  <nav class="min-h-0 flex-1 overflow-y-auto px-2 pb-2">
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
  </nav>
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
</aside>

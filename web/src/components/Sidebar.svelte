<script lang="ts">
  import type { Attachment } from "svelte/attachments";
  import { groupSessions } from "../lib/format";
  import Icon, { type IconName } from "../lib/Icon.svelte";
  import { router } from "../state/route.svelte";
  import { otherSessions, webSessions, type SessionList } from "../state/sessions.svelte";
  import { theme, type ThemeChoice } from "../state/theme.svelte";

  let { open, onclose }: { open: boolean; onclose: () => void } = $props();

  const groups = $derived(groupSessions(webSessions.items));
  const activeId = $derived(router.current.kind === "session" ? router.current.id : null);
  let othersOpen = $state(false);

  const THEME: Record<ThemeChoice, { icon: IconName; label: string }> = {
    system: { icon: "monitor", label: "跟随系统" },
    light: { icon: "sun", label: "浅色" },
    dark: { icon: "moon", label: "深色" },
  };

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

{#snippet entry(id: number, preview: string | null)}
  <a
    href="/s/{id}"
    onclick={(e) => (e.preventDefault(), go(`/s/${id}`))}
    class="block truncate rounded-md px-3 py-1.5 text-sm hover:bg-accent"
    class:bg-accent={activeId === id}
    class:font-medium={activeId === id}>{preview ?? `会话 ${id}`}</a>
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
    {#each groups as group (group.label)}
      <p class="px-3 pt-3 pb-1 text-xs text-muted-foreground">{group.label}</p>
      {#each group.items as s (s.id)}
        {@render entry(s.id, s.preview)}
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
          {@render entry(s.id, s.preview)}
        {/each}
        {#if other.loaded && other.items.length === 0}
          <p class="px-3 text-xs text-muted-foreground">无</p>
        {/if}
        {@render list(other)}
      {/each}
    {/if}
  </nav>
  <div class="flex flex-wrap items-center justify-between border-t border-border p-2">
    <button
      type="button"
      onclick={() => go("/settings")}
      class="flex items-center gap-2 rounded-md px-2 py-1.5 text-xs text-muted-foreground hover:bg-accent hover:text-foreground"
      class:text-foreground={router.current.kind === "settings"}>
      <Icon name="settings" />设置
    </button>
    <button
      type="button"
      onclick={() => go("/developer")}
      class="flex items-center gap-2 rounded-md px-2 py-1.5 text-xs text-muted-foreground hover:bg-accent hover:text-foreground"
      class:text-foreground={router.current.kind === "developer"}>
      <Icon name="terminal" />开发者诊断
    </button>
    <button
      type="button"
      onclick={() => theme.cycle()}
      class="flex items-center gap-2 rounded-md px-2 py-1.5 text-xs text-muted-foreground hover:bg-accent hover:text-foreground">
      <Icon name={THEME[theme.choice].icon} />主题：{THEME[theme.choice].label}
    </button>
  </div>
</aside>

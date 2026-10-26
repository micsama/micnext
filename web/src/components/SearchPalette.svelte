<script lang="ts">
  import * as Dialog from "$lib/components/ui/dialog";
  import type { Attachment } from "svelte/attachments";
  import type { SessionHit, SessionItem } from "../api/types";
  import { formatAgo, sessionSourceLabel } from "../lib/format";
  import Icon from "../lib/Icon.svelte";
  import { router } from "../state/route.svelte";
  import { searchPalette as search } from "../state/search.svelte";
  import { sidebar } from "../state/sidebar.svelte";

  /** 键盘可选的一行：会话本身或其中一条命中。 */
  type Entry = { key: string; session: number; message: number | null };

  const terms = $derived([...new Set(search.query.toLowerCase().split(/\s+/).filter(Boolean))]);
  const entries = $derived(
    search.items.flatMap((h): Entry[] => [
      { key: `s${h.session.id}`, session: h.session.id, message: null },
      ...h.hits.map((m) => ({ key: `m${m.message_id}`, session: h.session.id, message: m.message_id })),
    ]),
  );
  let active = $state(0);
  $effect(() => {
    void search.query;
    active = 0;
  });
  const activeKey = $derived(entries[Math.min(active, entries.length - 1)]?.key);

  /** 按检索词切段，命中段高亮；英文不分大小写，与服务端一致。 */
  function segments(text: string): { text: string; hit: boolean }[] {
    if (terms.length === 0) return [{ text, hit: false }];
    const lower = text.toLowerCase();
    const out: { text: string; hit: boolean }[] = [];
    let i = 0;
    let plain = 0;
    while (i < text.length) {
      const term = terms.find((t) => lower.startsWith(t, i));
      if (!term) { i++; continue; }
      if (i > plain) out.push({ text: text.slice(plain, i), hit: false });
      out.push({ text: text.slice(i, i + term.length), hit: true });
      i += term.length;
      plain = i;
    }
    if (plain < text.length) out.push({ text: text.slice(plain), hit: false });
    return out;
  }

  const title = (s: SessionItem) => s.title ?? s.preview ?? `会话 ${s.id}`;

  function open(entry: Entry | undefined) {
    if (!entry) return;
    search.open = false;
    sidebar.drawer = false;
    router.go(entry.message === null ? `/s/${entry.session}` : `/s/${entry.session}?m=${entry.message}`);
  }

  function onkeydown(e: KeyboardEvent) {
    if (e.isComposing) return;
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      if (entries.length === 0) return;
      active = (Math.min(active, entries.length - 1) + (e.key === "ArrowDown" ? 1 : entries.length - 1)) % entries.length;
      document.getElementById(`search-${entries[active]!.key}`)?.scrollIntoView({ block: "nearest" });
    } else if (e.key === "Enter") {
      e.preventDefault();
      open(entries[Math.min(active, entries.length - 1)]);
    }
  }

  let scroller = $state<HTMLElement | null>(null);
  const loadMoreWhenSeen: Attachment<HTMLElement> = (el) => {
    const io = new IntersectionObserver((list) => {
      if (list.some((e) => e.isIntersecting)) search.loadMore();
    }, { root: scroller, rootMargin: "0px 0px 120px 0px" });
    io.observe(el);
    return () => io.disconnect();
  };

  const row = "flex w-full items-center gap-2 rounded-lg text-left transition-colors";
</script>

{#snippet marked(text: string)}
  {#each segments(text) as part, i (i)}
    {#if part.hit}<mark class="rounded-[3px] bg-primary/15 px-px font-medium text-foreground">{part.text}</mark>{:else}{part.text}{/if}
  {/each}
{/snippet}

{#snippet badge(text: string)}
  <span class="shrink-0 rounded-full border border-border px-1.5 text-[10px] leading-4 text-muted-foreground">{text}</span>
{/snippet}

{#snippet group(h: SessionHit)}
  {@const s = h.session}
  {@const key = `s${s.id}`}
  <section class="py-1">
    <button
      type="button"
      id="search-{key}"
      role="option"
      aria-selected={activeKey === key}
      onclick={() => open({ key, session: s.id, message: null })}
      onmousemove={() => (active = entries.findIndex((e) => e.key === key))}
      class={[row, "px-3 py-2", activeKey === key ? "bg-accent text-foreground" : "text-foreground/90"]}>
      <Icon name="message" class="size-4 shrink-0 text-muted-foreground" />
      <span class="min-w-0 flex-1 truncate text-sm font-medium">{#if h.title_hit}{@render marked(title(s))}{:else}{title(s)}{/if}</span>
      {#if s.source.kind === "channel" && s.source.channel !== "web"}{@render badge(sessionSourceLabel(s.source))}{/if}
      {#if s.archived_at !== null}{@render badge("已归档")}{/if}
      <time class="shrink-0 text-xs text-muted-foreground tabular-nums">{formatAgo(s.last_activity_at)}</time>
    </button>
    {#if h.hits.length > 0}
      <div class="ml-5 border-l border-border pl-2">
        {#each h.hits as m (m.message_id)}
          {@const mkey = `m${m.message_id}`}
          <button
            type="button"
            id="search-{mkey}"
            role="option"
            aria-selected={activeKey === mkey}
            onclick={() => open({ key: mkey, session: s.id, message: m.message_id })}
            onmousemove={() => (active = entries.findIndex((e) => e.key === mkey))}
            class={[row, "items-start px-2.5 py-1.5", activeKey === mkey ? "bg-accent text-foreground" : "text-muted-foreground"]}>
            <span class={["mt-0.5 flex size-4 shrink-0 items-center justify-center rounded-full", m.from_user ? "bg-primary/10 text-primary" : "bg-muted-foreground/10"]} title={m.from_user ? "你说的" : "回复"}>
              <Icon name={m.from_user ? "user" : "bot"} class="size-2.5" />
            </span>
            <span class="line-clamp-2 min-w-0 flex-1 text-[13px] leading-5 break-words">{@render marked(m.snippet)}</span>
            <time class="mt-0.5 shrink-0 text-[11px] text-muted-foreground/80 tabular-nums">{formatAgo(m.created_at)}</time>
          </button>
        {/each}
        {#if h.hit_count > h.hits.length}
          <p class="px-2.5 pt-0.5 pb-1 pl-8.5 text-[11px] text-muted-foreground/80">还有 {h.hit_count - h.hits.length} 条匹配，打开会话查看</p>
        {/if}
      </div>
    {/if}
  </section>
{/snippet}

{#snippet kbd(text: string)}
  <kbd class="inline-flex h-4.5 min-w-4.5 items-center justify-center rounded border border-border bg-muted px-1 font-sans text-[10px] text-muted-foreground">{text}</kbd>
{/snippet}

<Dialog.Root bind:open={search.open}>
  <Dialog.Content
    showCloseButton={false}
    class="top-[12vh] flex max-h-[76vh] translate-y-0 flex-col gap-0 overflow-hidden rounded-2xl p-0 shadow-2xl sm:max-w-2xl">
    <Dialog.Title class="sr-only">搜索会话</Dialog.Title>
    <div class="flex h-14 shrink-0 items-center gap-3 border-b border-border px-4">
      <Icon name={search.loading ? "loader" : "search"} class={search.loading ? "size-5 shrink-0 animate-spin text-primary" : "size-5 shrink-0 text-muted-foreground"} />
      <input
        value={search.query}
        oninput={(e) => search.input(e.currentTarget.value)}
        {onkeydown}
        placeholder="搜索会话标题、你说的话和回复…"
        aria-label="搜索会话"
        role="combobox"
        aria-expanded={entries.length > 0}
        aria-controls="search-results"
        aria-activedescendant={activeKey ? `search-${activeKey}` : undefined}
        autocomplete="off"
        spellcheck="false"
        class="h-full min-w-0 flex-1 bg-transparent text-base outline-none placeholder:text-muted-foreground/70" />
      {#if search.query}
        <button type="button" aria-label="清空" title="清空" onclick={() => search.input("")} class="rounded-md p-1 text-muted-foreground hover:bg-accent hover:text-foreground">
          <Icon name="x" class="size-4" />
        </button>
      {/if}
    </div>

    <div bind:this={scroller} id="search-results" role="listbox" aria-label="搜索结果" class="min-h-0 flex-1 overflow-y-auto px-2 py-1">
      {#if !search.query.trim()}
        <div class="flex flex-col items-center gap-2 px-6 py-12 text-center text-sm text-muted-foreground">
          <span class="flex size-10 items-center justify-center rounded-full bg-muted"><Icon name="search" class="size-5" /></span>
          <p class="text-foreground/80">在所有会话里找聊过的内容</p>
          <p class="text-xs">空格分隔多个词时，需出现在同一条消息里；近期的会话排在前面</p>
        </div>
      {:else if search.error}
        <p class="flex items-center gap-2 px-3 py-6 text-sm text-destructive"><Icon name="alert" class="size-4" />{search.error}</p>
      {:else if search.done && search.items.length === 0}
        <div class="flex flex-col items-center gap-2 px-6 py-12 text-center text-sm text-muted-foreground">
          <p>没有找到包含「<span class="text-foreground">{search.query.trim()}</span>」的会话</p>
          <p class="text-xs">试试更短的词，或者减少词数</p>
        </div>
      {:else}
        {#each search.items as h (h.session.id)}{@render group(h)}{/each}
        {#if search.next}
          <div {@attach loadMoreWhenSeen} class="flex items-center justify-center gap-2 py-3 text-xs text-muted-foreground">
            {#if search.loading}<Icon name="loader" class="size-3.5 animate-spin" />正在搜索更早的会话{/if}
          </div>
        {:else if search.done && !search.loading}
          <p class="py-3 text-center text-[11px] text-muted-foreground/70">已搜完全部会话</p>
        {/if}
      {/if}
    </div>

    <div class="flex shrink-0 items-center gap-4 border-t border-border bg-muted/40 px-4 py-2 text-[11px] text-muted-foreground">
      <span class="flex items-center gap-1">{@render kbd("↑")}{@render kbd("↓")}选择</span>
      <span class="flex items-center gap-1">{@render kbd("↵")}打开</span>
      <span class="flex items-center gap-1">{@render kbd("esc")}关闭</span>
      {#if search.items.length > 0}<span class="ml-auto tabular-nums">{search.items.length}{search.next ? "+" : ""} 个会话</span>{/if}
    </div>
  </Dialog.Content>
</Dialog.Root>

<script lang="ts">
  import MenuButton from "./MenuButton.svelte";
  import PushBell from "./PushBell.svelte";
  import type { Snippet } from "svelte";

  let { title, detail, actions, detailActions, onrename }: { title: string; detail?: string; actions?: Snippet; detailActions?: Snippet; onrename?: (title: string) => void } = $props();

  // 点标题原地编辑：Enter 或失焦保存，Esc 放弃；空白或未改动不保存。
  let editing = $state(false);
  let draft = $state("");

  function begin() {
    draft = title;
    editing = true;
  }

  function finish(save: boolean) {
    if (!editing) return;
    editing = false;
    const next = draft.trim();
    if (save && next && next !== title) onrename?.(next);
  }

  function onkeydown(e: KeyboardEvent) {
    if (e.isComposing) return;
    if (e.key === "Enter") { e.preventDefault(); finish(true); }
    else if (e.key === "Escape") { e.preventDefault(); finish(false); }
  }
</script>

<header class="flex h-12 shrink-0 items-center gap-2 border-b border-border px-3">
  <MenuButton />
  <div class="min-w-0 flex-1">
    <div class="flex items-center gap-1.5">
      {#if editing}
        <input
          aria-label="会话标题"
          maxlength={40}
          bind:value={draft}
          {onkeydown}
          onblur={() => finish(true)}
          {@attach (el: HTMLInputElement) => { el.focus(); el.select(); }}
          class="min-w-0 flex-1 rounded border border-border bg-background px-1.5 py-0.5 text-sm font-medium outline-none focus:border-primary" />
      {:else if onrename}
        <button type="button" title="点击修改标题" onclick={begin} class="min-w-0 cursor-text truncate rounded px-0.5 text-left text-sm font-medium hover:bg-accent">{title}</button>
      {:else}
        <span class="truncate text-sm font-medium">{title}</span>
      {/if}
      {#if !editing}{@render actions?.()}{/if}
    </div>
    {#if detail}
      <div class="flex min-w-0 items-center gap-2 text-xs text-muted-foreground max-sm:hidden"><span class="truncate font-mono">{detail}</span>{@render detailActions?.()}</div>
    {/if}
  </div>
  <!-- 窄屏只留标题一行，详情行的入口移到右侧。 -->
  {#if detailActions}<div class="flex shrink-0 items-center gap-2 text-xs sm:hidden">{@render detailActions()}</div>{/if}
  <PushBell />
</header>

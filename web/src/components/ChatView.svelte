<script lang="ts">
  import { ApiError, sendMessage } from "../api/client";
  import { router } from "../state/route.svelte";
  import type { SessionView } from "../state/session.svelte";
  import { findSession, webSessions } from "../state/sessions.svelte";
  import Composer from "./Composer.svelte";
  import Header from "./Header.svelte";
  import MessageList from "./MessageList.svelte";

  let { view, onmenu }: { view: SessionView; onmenu: () => void } = $props();

  const item = $derived(findSession(view.id));
  let forbidden = $state(false);
  const readonly = $derived(forbidden || (item !== undefined && item.channel !== "web"));

  const title = $derived(item?.preview ?? `会话 ${view.id}`);
  const detail = $derived(item ? `${item.workdir} · ${item.channel}` : undefined);
  $effect(() => {
    document.title = `${title} · micnext`;
  });

  async function send(text: string) {
    try {
      await sendMessage(view.id, text);
      void webSessions.refresh();
    } catch (e) {
      if (e instanceof ApiError && e.status === 403) forbidden = true;
      throw e;
    }
  }
</script>

<Header {title} {detail} {onmenu} />
{#if view.fatal}
  <div class="grid flex-1 place-items-center p-6 text-center">
    <div class="space-y-3">
      <p>{view.fatal}</p>
      <button type="button" class="text-sm text-accent underline" onclick={() => router.go("/")}>去新会话</button>
    </div>
  </div>
{:else}
  {#if view.reconnecting}
    <p class="bg-panel py-1 text-center text-xs text-warn">连接已断开，正在重连…</p>
  {/if}
  <MessageList {view} />
  {#if readonly}
    <p class="mx-auto w-full max-w-3xl px-4 pb-4 text-center text-sm text-muted">
      该会话来自 {item?.channel ?? "其它"} 渠道，只能在这里查看
    </p>
  {:else}
    <Composer onsend={send} autofocus />
  {/if}
{/if}

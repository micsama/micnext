<script lang="ts">
  import { sendMessage, setSessionPersona } from "../api/client";
  import { router } from "../state/route.svelte";
  import type { SessionView } from "../state/session.svelte";
  import { webSessions } from "../state/sessions.svelte";
  import { settingsStore } from "../state/settings.svelte";
  import Composer from "./Composer.svelte";
  import Header from "./Header.svelte";
  import MessageList from "./MessageList.svelte";
  import PersonaPicker from "./PersonaPicker.svelte";

  let { view, onmenu }: { view: SessionView; onmenu: () => void } = $props();

  const item = $derived(view.info);

  const title = $derived(item?.preview ?? `会话 ${view.id}`);
  const detail = $derived(item ? `${item.workdir} · ${item.channel}` : undefined);
  $effect(() => {
    document.title = `${title} · micnext`;
  });

  async function send(text: string) {
    await sendMessage(view.id, text);
    void webSessions.refresh();
  }

  const persona = $derived(item ? settingsStore.resolve(item.persona_id) : null);
  let pickError = $state<string | null>(null);
  /** 本轮执行中改过选择：新人设从下一轮生效。 */
  let pickedDuringRun = $state(false);
  $effect(() => {
    if (view.executingRun === null) pickedDuringRun = false;
  });

  async function pick(id: number) {
    const info = view.info!;
    const before = info.persona_id;
    info.persona_id = id;
    pickError = null;
    try {
      await setSessionPersona(view.id, id);
      if (view.executingRun !== null) pickedDuringRun = true;
    } catch (e) {
      info.persona_id = before;
      pickError = (e as Error).message;
    }
  }

  const notice = $derived(
    pickError ??
      (persona?.replaced ? "原人设已删除，下一轮改用默认人设" : pickedDuringRun ? "人设已切换，下一轮生效" : null),
  );
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
  {#if item?.writable}
    <Composer onsend={send} autofocus {notice}>
      {#snippet controls()}
        <PersonaPicker value={persona!.id} onpick={pick} />
      {/snippet}
    </Composer>
  {:else if item}
    <p class="mx-auto w-full max-w-3xl px-4 pb-4 text-center text-sm text-muted">
      该会话来自 {item.channel} 渠道，只能在这里查看
    </p>
  {/if}
{/if}

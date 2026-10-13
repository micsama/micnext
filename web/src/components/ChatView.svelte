<script lang="ts">
  import type { InputPart, ReasoningEffort } from "../api/types";
  import { sendMessage, setSessionModel, setSessionPersona } from "../api/client";
  import { router } from "../state/route.svelte";
  import type { SessionView } from "../state/session.svelte";
  import { webSessions } from "../state/sessions.svelte";
  import { settingsStore } from "../state/settings.svelte";
  import Composer from "./Composer.svelte";
  import Header from "./Header.svelte";
  import MessageList from "./MessageList.svelte";
  import ModelPicker from "./ModelPicker.svelte";
  import PersonaPicker from "./PersonaPicker.svelte";

  let { view, onmenu }: { view: SessionView; onmenu: () => void } = $props();

  const item = $derived(view.info);

  const title = $derived(item?.preview ?? `会话 ${view.id}`);
  const detail = $derived(item ? `${item.workdir} · ${item.channel}` : undefined);
  $effect(() => {
    document.title = `${title} · micnext`;
  });

  async function send(parts: InputPart[]) {
    await sendMessage(view.id, parts);
    void webSessions.refresh();
  }

  const persona = $derived(item ? settingsStore.resolve(item.persona_id) : null);
  let pickError = $state<string | null>(null);
  /** 本轮执行中改过人设或模型：从下一轮生效。 */
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

  /** null = 下次取默认模型；指向已删除条目时为 null 且提示重选。 */
  const modelDeleted = $derived(
    !!item && item.model_id !== null && settingsStore.loaded && !settingsStore.model(item.model_id),
  );
  const modelValue = $derived(
    !item || modelDeleted ? null : (item.model_id ?? settingsStore.defaultModelId),
  );

  let modelSaving = $state(false);

  async function pickModel(id: number, effort: ReasoningEffort | null = null) {
    if (modelSaving) return;
    const info = view.info!;
    const before = { model: info.model_id, effort: info.reasoning_effort };
    info.model_id = id;
    info.reasoning_effort = effort;
    modelSaving = true;
    pickError = null;
    try {
      await setSessionModel(view.id, id, effort);
      if (view.executingRun !== null) pickedDuringRun = true;
    } catch (e) {
      info.model_id = before.model;
      info.reasoning_effort = before.effort;
      pickError = (e as Error).message;
    } finally {
      modelSaving = false;
    }
  }

  const channelName = (c: string) => (c === "wechat" ? "微信" : c);

  const modelNotice = $derived(pickError ?? (modelDeleted ? "原模型已删除，请重新选择" : null));
  const pickedNotice = $derived(pickedDuringRun ? "已切换，下一轮生效" : null);
  const readonlyNotice = $derived(modelNotice ?? pickedNotice);
  const notice = $derived(
    modelNotice ?? (persona?.replaced ? "原人设已删除，下一轮改用默认人设" : null) ?? pickedNotice,
  );
</script>

<Header {title} {detail} {onmenu} />
{#if view.fatal}
  <div class="grid flex-1 place-items-center p-6 text-center">
    <div class="space-y-3">
      <p>{view.fatal}</p>
      <button type="button" class="text-sm text-primary underline" onclick={() => router.go("/")}>去新会话</button>
    </div>
  </div>
{:else}
  {#if view.reconnecting}
    <p class="bg-muted py-1 text-center text-xs text-warning">连接已断开，正在重连…</p>
  {/if}
  <MessageList {view} />
  {#if item?.writable}
    <Composer onsend={send} autofocus {notice} blocked={modelSaving}>
      {#snippet leading()}
        <PersonaPicker value={persona!.id} onpick={pick} />
      {/snippet}
      {#snippet trailing()}
        <ModelPicker value={modelValue} onpick={(id) => pickModel(id)} reasoningEffort={item.reasoning_effort} onreasoning={(effort) => pickModel(modelValue!, effort)} disabled={modelSaving} />
      {/snippet}
    </Composer>
  {:else if item}
    <div class="mx-auto w-full max-w-3xl space-y-1 px-4 pb-4">
      {#if readonlyNotice}<p class="text-sm text-warning">{readonlyNotice}</p>{/if}
      <div class="flex items-center justify-between gap-3">
        <p class="text-sm text-muted-foreground">消息请在{channelName(item.channel)}发送，这里可切换模型与思考强度</p>
        <ModelPicker value={modelValue} onpick={(id) => pickModel(id)} reasoningEffort={item.reasoning_effort} onreasoning={(effort) => pickModel(modelValue!, effort)} disabled={modelSaving} />
      </div>
    </div>
  {/if}
{/if}

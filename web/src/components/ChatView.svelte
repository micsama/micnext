<script lang="ts">
  import { sessionSourceLabel } from "../lib/format";
  import type { InputPart, ReasoningEffort } from "../api/types";
  import { sendMessage, setSessionModel, setSessionPersona, setSessionPlaintext } from "../api/client";
  import { router } from "../state/route.svelte";
  import type { SessionView } from "../state/session.svelte";
  import { settingsStore } from "../state/settings.svelte";
  import Composer from "./Composer.svelte";
  import Header from "./Header.svelte";
  import MessageList from "./MessageList.svelte";
  import ModelPicker from "./ModelPicker.svelte";
  import ContextMeter from "./ContextMeter.svelte";
  import PersonaPicker from "./PersonaPicker.svelte";
  import Switch from "./Switch.svelte";
  import Icon from "../lib/Icon.svelte";
  import SessionTags from "./SessionTags.svelte";
  import JobsDock from "./JobsDock.svelte";
  import { provideJobRefs } from "./JobRef.svelte";

  let { view }: { view: SessionView } = $props();
  provideJobRefs(() => view);

  const item = $derived(view.info);

  const scheduled = $derived(item?.source.kind === "schedule");
  const title = $derived(scheduled ? `定时任务 · ${item?.schedule?.name ?? item?.title ?? "已删除任务"}` : item?.title ?? item?.preview ?? `会话 ${view.id}`);
  const titleBusy = $derived(view.requestingTitle || !!item?.title_generating);
  const titleError = $derived(view.titleActionError ?? view.titleSyncError ?? (titleBusy ? null : item?.title_error));
  const detail = $derived(item ? `${item.workdir} · ${sessionSourceLabel(item.source)}` : undefined);
  $effect(() => {
    document.title = `${title} · micnext`;
  });

  async function send(parts: InputPart[]) {
    await sendMessage(view.id, parts);
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

  async function togglePlaintext() {
    const info = view.info!;
    info.plaintext = !info.plaintext;
    pickError = null;
    try {
      await setSessionPlaintext(view.id, info.plaintext);
    } catch (e) {
      info.plaintext = !info.plaintext;
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



  const modelNotice = $derived(pickError ?? (modelDeleted ? "原模型已删除，请重新选择" : null));
  const pickedNotice = $derived(pickedDuringRun ? "已切换，下一轮生效" : null);
  const readonlyNotice = $derived(modelNotice ?? pickedNotice);
  const notice = $derived(
    modelNotice ?? (!scheduled && persona?.replaced ? "原人设已删除，下一轮改用默认人设" : null) ?? pickedNotice,
  );
</script>

{#snippet modelControls()}
  {#if view.context?.occupancy}<ContextMeter session={view.id} occupancy={view.context.occupancy} />{/if}
  <ModelPicker value={modelValue} onpick={(id) => pickModel(id)} reasoningEffort={item!.reasoning_effort} onreasoning={(effort) => pickModel(modelValue!, effort)} disabled={modelSaving} />
{/snippet}
{#snippet jobsDock()}<JobsDock {view} writable={!!item?.writable && item.archived_at === null} />{/snippet}

<Header {title} {detail}>
  {#snippet detailActions()}
    {#if scheduled}<button type="button" class="shrink-0 text-primary hover:underline" onclick={() => router.go("/schedules")}>定时任务</button>{/if}
    <button type="button" class="inline-flex shrink-0 items-center gap-1 rounded px-1.5 text-primary hover:bg-accent" onclick={() => router.go(`/s/${view.id}/trace`)} title="查看当前会话的 Trace"><Icon name="sparkles" class="size-3" />Trace</button>
  {/snippet}
  {#snippet actions()}
    {#if !scheduled}
    <button type="button" aria-label="重新生成标题" title={titleBusy ? "正在生成标题" : "重新生成标题"} disabled={titleBusy || !view.ready || !!view.fatal || item?.archived_at != null} onclick={() => view.regenerateTitle()} class="flex size-6 shrink-0 items-center justify-center rounded-full text-muted-foreground transition-colors hover:bg-accent hover:text-foreground disabled:opacity-40">
      <Icon name={titleBusy ? "loader" : "refresh"} class="size-3.5 {titleBusy ? 'animate-spin' : ''}" />
    </button>
    {#if item}<SessionTags {item} />{/if}
    {/if}
  {/snippet}
</Header>
{#if titleError}<p role="alert" class="border-b border-border px-4 py-2 text-xs text-destructive">标题生成：{titleError}</p>{/if}
<div class="flex min-h-0 flex-1 flex-col">
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
  {#if item?.writable && item.archived_at === null}
    <Composer onsend={send} autofocus bind:reply={view.reply} {notice} dock={view.jobs.length > 0 ? jobsDock : undefined} blocked={modelSaving} danger={item?.plaintext} onstop={view.executingRun === null ? undefined : () => view.stop()} stopping={view.stopping !== null} prediction={view.prediction?.text ?? null} trailing={modelControls}>
      {#snippet leading()}
        {#if !scheduled}<PersonaPicker value={persona!.id} onpick={pick} />{/if}
        <label
          class="inline-flex h-9 shrink-0 items-center gap-1.5 rounded-full px-2 {item?.plaintext ? 'text-destructive' : 'text-success'}"
          title={item?.plaintext
            ? "凭据脱敏已临时关闭：开启后的新消息里的凭据原样发给模型，之前的内容仍脱敏；关掉恢复全部脱敏。主密钥与模型 key 始终遮住"
            : "凭据脱敏中：发给模型前遮住消息里的 key、token、密码等；打开开关可临时让新消息不脱敏"}>
          <Icon name={item?.plaintext ? "lock-open" : "lock"} class="size-4" />
          <Switch checked={!!item?.plaintext} label="临时关闭凭据脱敏" class={item?.plaintext ? "bg-destructive" : "bg-success"} onclick={togglePlaintext} />
        </label>
      {/snippet}
    </Composer>
  {:else if item}
    {@const archived = item.archived_at !== null}
    <Composer
      onsend={send}
      readonly={archived ? "会话已归档，历史记录仍可查看" : `消息请在${sessionSourceLabel(item.source)}发送，这里可切换模型与思考强度`}
      notice={archived ? null : readonlyNotice}
      dock={view.jobs.length > 0 ? jobsDock : undefined}
      trailing={archived ? undefined : modelControls} />
  {/if}
{/if}
</div>

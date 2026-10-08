<script lang="ts">
  import { createSession } from "../api/client";
  import { router } from "../state/route.svelte";
  import { webSessions } from "../state/sessions.svelte";
  import { settingsStore } from "../state/settings.svelte";
  import Composer from "./Composer.svelte";
  import Header from "./Header.svelte";
  import ModelPicker from "./ModelPicker.svelte";
  import PersonaPicker from "./PersonaPicker.svelte";

  let { onmenu }: { onmenu: () => void } = $props();

  $effect(() => {
    document.title = "新会话 · micnext";
  });

  let picked = $state<number | null>(null);
  const persona = $derived.by(() => {
    const id = picked ?? settingsStore.settings?.default_persona_id ?? null;
    return id === null ? null : settingsStore.resolve(id);
  });

  let pickedModel = $state<number | null>(null);
  /** 选中的模型已被删除时回到默认模型。 */
  const modelId = $derived.by(() => {
    if (pickedModel !== null && settingsStore.model(pickedModel)) return pickedModel;
    return settingsStore.defaultModelId;
  });

  async function send(text: string) {
    const created = await createSession(text, persona!.id, modelId!);
    void webSessions.refresh();
    router.go(`/s/${created.session_id}`);
  }
</script>

<Header title="新会话" {onmenu} />
<div class="grid flex-1 place-items-center px-4">
  <p class="text-2xl font-medium text-muted">有什么要做的？</p>
</div>
<Composer
  onsend={send}
  placeholder="发送第一条消息开始新会话"
  autofocus
  blocked={persona === null || modelId === null}
  notice={settingsStore.error ??
    (settingsStore.loaded && modelId === null
      ? "还没有可用的模型：先到 设置 → 模型 添加一个"
      : persona?.replaced
        ? "原选的人设已删除，已改用默认人设"
        : null)}>
  {#snippet controls()}
    {#if persona !== null}
      <PersonaPicker value={persona.id} onpick={(id) => (picked = id)} />
    {/if}
    {#if modelId !== null}
      <ModelPicker value={modelId} onpick={(id) => (pickedModel = id)} />
    {/if}
  {/snippet}
</Composer>

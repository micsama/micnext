<script lang="ts">
  import { getTitleModel, putTitleModel } from "../../api/client";
  import type { TitleModelSelection } from "../../api/types";
  import { settingsStore } from "../../state/settings.svelte";
  import ModelPicker from "../ModelPicker.svelte";
  import Group from "./Group.svelte";
  import SaveStatus, { type SaveState } from "./SaveStatus.svelte";

  let status = $state<SaveState | undefined>();
  const selection = $derived(settingsStore.titleModel);
  const busy = $derived(status?.kind === "saving");
  const deleted = $derived(selection?.model_id != null && !settingsStore.model(selection.model_id));

  async function save(next: TitleModelSelection) {
    if (busy) return;
    const before = selection;
    settingsStore.titleModel = next;
    status = { kind: "saving" };
    try {
      await putTitleModel(next);
      settingsStore.titleModel = await getTitleModel();
      status = { kind: "saved" };
    } catch (error) {
      settingsStore.titleModel = before;
      status = { kind: "error", message: (error as Error).message };
    }
  }
</script>

<Group title="标题生成" footer="首个文字问题自动取标题，也可以在标题旁重新生成。选具体模型后可单独调整思考强度。">
  <div class="space-y-2 px-4 py-3">
    <div class="flex flex-wrap items-center justify-between gap-3">
      <div class="flex items-center gap-2"><span class="text-sm">生成模型</span><SaveStatus state={status} /></div>
      {#if selection}
        <ModelPicker value={selection.model_id} reasoningEffort={selection.reasoning_effort} disabled={busy} ondefault={() => save({ model_id: null, reasoning_effort: null })} onpick={(id) => save({ model_id: id, reasoning_effort: null })} onreasoning={(effort) => save({ model_id: selection.model_id, reasoning_effort: effort })} />
      {/if}
    </div>
    {#if status?.kind === "error"}<p role="alert" class="text-xs text-destructive">{status.message}</p>{/if}
    {#if deleted}<p class="text-xs text-destructive">标题模型已删除，请重新选择</p>{/if}
  </div>
</Group>

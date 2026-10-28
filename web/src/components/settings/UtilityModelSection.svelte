<script lang="ts">
  import { getUtilityModel, putUtilityModel } from "../../api/client";
  import type { UtilityModelSelection } from "../../api/types";
  import { settingsStore } from "../../state/settings.svelte";
  import ModelPicker from "../ModelPicker.svelte";
  import Group from "./Group.svelte";
  import SaveStatus, { type SaveState } from "./SaveStatus.svelte";

  let status = $state<SaveState | undefined>();
  const selection = $derived(settingsStore.utilityModel);
  const busy = $derived(status?.kind === "saving");
  const deleted = $derived(selection?.model_id != null && !settingsStore.model(selection.model_id));

  function pick(id: number) {
    const kept = selection?.reasoning_effort ?? null;
    const supported = kept !== null && settingsStore.model(id)?.reasoning?.choices.includes(kept);
    save({ model_id: id, reasoning_effort: supported ? kept : null });
  }

  async function save(next: UtilityModelSelection) {
    if (busy) return;
    const before = selection;
    settingsStore.utilityModel = next;
    status = { kind: "saving" };
    try {
      await putUtilityModel(next);
      settingsStore.utilityModel = await getUtilityModel();
      status = { kind: "saved" };
    } catch (error) {
      settingsStore.utilityModel = before;
      status = { kind: "error", message: (error as Error).message };
    }
  }
</script>

<Group title="杂活模型" footer="负责会话标题与标签生成、SQL 写入把关。首个文字问题自动取标题，也可以在标题旁重新生成。选具体模型后可单独调整思考强度。">
  <div class="space-y-2 px-4 py-3">
    <div class="flex flex-wrap items-center justify-between gap-3">
      <div class="flex items-center gap-2"><span class="text-sm">模型</span><SaveStatus state={status} /></div>
      {#if selection}
        <ModelPicker value={selection.model_id} reasoningEffort={selection.reasoning_effort} disabled={busy} ondefault={() => save({ model_id: null, reasoning_effort: null })} onpick={pick} onreasoning={(effort) => save({ model_id: selection.model_id, reasoning_effort: effort })} />
      {/if}
    </div>
    {#if status?.kind === "error"}<p role="alert" class="text-xs text-destructive">{status.message}</p>{/if}
    {#if deleted}<p class="text-xs text-destructive">杂活模型已删除，请重新选择</p>{/if}
  </div>
</Group>

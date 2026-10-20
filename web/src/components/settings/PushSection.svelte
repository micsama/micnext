<script lang="ts">
  import CheckIcon from "@lucide/svelte/icons/check";
  import { getPushSettings, putPushSettings } from "../../api/client";
  import { channelLabel, type PushSettings } from "../../api/pushes";
  import { cn } from "$lib/utils";
  import Group from "./Group.svelte";
  import SaveStatus, { type SaveState } from "./SaveStatus.svelte";

  let settings = $state<PushSettings | null>(null);
  let status = $state<SaveState | undefined>();
  const busy = $derived(status?.kind === "saving");

  $effect(() => {
    getPushSettings().then(
      (s) => (settings = s),
      (e: Error) => (status = { kind: "error", message: e.message }),
    );
  });

  async function toggle(channel: string) {
    if (!settings || busy) return;
    const chosen = settings.chosen.includes(channel)
      ? settings.chosen.filter((c) => c !== channel)
      : settings.channels.filter((c) => c === channel || settings!.chosen.includes(c));
    status = { kind: "saving" };
    try {
      await putPushSettings(chosen);
      settings = await getPushSettings();
      status = { kind: "saved" };
    } catch (e) {
      status = { kind: "error", message: (e as Error).message };
    }
  }
</script>

<Group title="推送" footer="agent 主动通知你（如长任务完成提醒）时，推送总会进 Web 通知中心；未指定渠道时，同时发到这里选的 IM。">
  {#snippet actions()}<SaveStatus state={status} />{/snippet}
  <div class="space-y-2 px-4 py-3.5">
    <div class="flex flex-wrap items-center justify-between gap-3">
      <span class="text-sm">默认推送渠道</span>
      {#if settings}
        <div class="flex flex-wrap gap-1.5">
          {#each settings.channels as c (c)}
            {@const on = settings.chosen.includes(c)}
            <button
              type="button"
              aria-pressed={on}
              disabled={busy}
              class={cn(
                "inline-flex items-center gap-1 rounded-full border px-3 py-1 text-xs transition-colors disabled:opacity-50",
                on ? "border-primary bg-primary/10 text-foreground" : "text-muted-foreground hover:text-foreground",
              )}
              onclick={() => toggle(c)}
            >
              {#if on}<CheckIcon class="size-3" />{/if}{channelLabel(c)}
            </button>
          {/each}
        </div>
      {/if}
    </div>
    {#if settings && settings.chosen.length === 0}
      <p class="text-xs text-muted-foreground">未选择：发到所有已连接的 IM。{settings.effective.length > 0 ? `当前发往 ${settings.effective.map(channelLabel).join("、")}。` : "当前没有已连接的 IM，只进 Web 通知中心。"}</p>
    {/if}
    {#if status?.kind === "error"}<p role="alert" class="text-xs text-destructive">{status.message}</p>{/if}
  </div>
</Group>

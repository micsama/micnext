<script lang="ts">
  import CheckIcon from "@lucide/svelte/icons/check";
  import { getPushSettings, putPushSettings } from "../../api/client";
  import { channelLabel, type PushSettings } from "../../api/pushes";
  import { cn } from "$lib/utils";
  import { currentSubscription, disableWebPush, enableWebPush, webPushSupported } from "$lib/webPush";
  import Switch from "../Switch.svelte";
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

  /** 本设备的系统通知订阅；`undefined` = 尚未读出。 */
  let device = $state<PushSubscription | null | undefined>(webPushSupported ? undefined : null);
  let deviceBusy = $state(false);
  let deviceError = $state<string | null>(null);

  $effect(() => {
    if (!webPushSupported) return;
    currentSubscription().then(
      (s) => (device = s),
      (e: Error) => ((device = null), (deviceError = e.message)),
    );
  });

  async function toggleDevice() {
    if (device === undefined || deviceBusy) return;
    deviceBusy = true;
    deviceError = null;
    try {
      if (device) await disableWebPush(device);
      else await enableWebPush();
      device = await currentSubscription();
    } catch (e) {
      deviceError = (e as Error).message;
      device = await currentSubscription().catch(() => null);
    } finally {
      deviceBusy = false;
    }
  }

  const channelHint = $derived.by(() => {
    if (!settings || settings.chosen.length > 0) return "推送同时发到选中的 IM";
    if (settings.effective.length === 0) return "未选择；当前没有已连接的 IM";
    return `未选择，发到所有已连接的 IM（${settings.effective.map(channelLabel).join("、")}）`;
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

<Group title="推送" footer="agent 主动通知你（如长任务完成提醒）时，推送总会进 Web 通知中心。">
  {#snippet actions()}<SaveStatus state={status} />{/snippet}
  <div class="space-y-2.5 px-4 py-3.5">
    {@render label("默认推送渠道", channelHint)}
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
    {#if status?.kind === "error"}<p role="alert" class="text-xs text-destructive">{status.message}</p>{/if}
  </div>

  <div class="space-y-1 px-4 py-3.5">
    <div class="flex items-center justify-between gap-4">
      {@render label(
        "本设备系统通知",
        webPushSupported ? "推送同时在这台设备弹出系统通知" : "iPhone 需先在 Safari 分享菜单「添加到主屏幕」，再从主屏幕打开",
      )}
      <Switch
        checked={!!device}
        label="本设备系统通知"
        disabled={!webPushSupported || device === undefined || deviceBusy}
        onclick={toggleDevice}
      />
    </div>
    {#if deviceError}<p role="alert" class="text-xs text-destructive">{deviceError}</p>{/if}
  </div>
</Group>

{#snippet label(text: string, hint: string)}
  <div class="min-w-0 flex-1">
    <span class="text-sm">{text}</span>
    <p class="mt-0.5 text-xs text-muted-foreground">{hint}</p>
  </div>
{/snippet}

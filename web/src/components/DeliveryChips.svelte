<script lang="ts">
  import { Popover } from "bits-ui";
  import { channelLabel, type PushDelivery } from "../api/pushes";
  import { formatTime } from "../lib/format";
  import Icon from "../lib/Icon.svelte";

  let { deliveries }: { deliveries: PushDelivery[] } = $props();

  function title(d: PushDelivery): string {
    const name = channelLabel(d.channel);
    switch (d.state.kind) {
      case "pending": return `${name}：投递中`;
      case "delivered": return `${name}：${formatTime(d.state.at)} 送达`;
      case "failed": return `${name}：未送达（${d.state.reason}）`;
    }
  }
</script>

<!-- 失败原因点开看：悬停在手机上不可用。 -->
{#each deliveries as d (d.channel)}
  {#if d.state.kind === "failed"}
    <Popover.Root>
      <Popover.Trigger title={title(d)} class="inline-flex items-center gap-1 rounded text-destructive hover:underline">
        <Icon name="alert" class="size-3" />{channelLabel(d.channel)}
      </Popover.Trigger>
      <Popover.Portal>
        <Popover.Content side="bottom" align="start" sideOffset={4} collisionPadding={8} class="z-50 max-w-64 rounded-xl border border-border bg-popover px-3 py-2 text-xs text-popover-foreground shadow-lg outline-none">
          {d.state.reason}
        </Popover.Content>
      </Popover.Portal>
    </Popover.Root>
  {:else}
    <span title={title(d)} class="inline-flex items-center gap-1">
      <Icon name={d.state.kind === "delivered" ? "check" : "loader"} class={d.state.kind === "pending" ? "size-3 animate-spin" : "size-3"} />{channelLabel(d.channel)}
    </span>
  {/if}
{/each}

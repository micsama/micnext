<script lang="ts">
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

{#each deliveries as d (d.channel)}
  <span title={title(d)} class={["inline-flex items-center gap-1", d.state.kind === "failed" && "text-destructive"]}>
    <Icon name={d.state.kind === "delivered" ? "check" : d.state.kind === "failed" ? "alert" : "loader"} class={d.state.kind === "pending" ? "size-3 animate-spin" : "size-3"} />{channelLabel(d.channel)}
  </span>
{/each}

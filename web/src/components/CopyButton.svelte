<script lang="ts">
  import { copyText } from "../lib/clipboard";
  import Icon from "../lib/Icon.svelte";

  let { text }: { text: string } = $props();
  let copied = $state(false);

  async function copy() {
    await copyText(text);
    copied = true;
    setTimeout(() => (copied = false), 1500);
  }
</script>

<button
  type="button"
  class="inline-flex items-center gap-1 rounded px-1.5 py-0.5 text-xs text-muted-foreground hover:bg-accent hover:text-foreground"
  onclick={copy}>
  <Icon name={copied ? "check" : "copy"} class="size-3.5" />{copied ? "已复制" : "复制"}
</button>

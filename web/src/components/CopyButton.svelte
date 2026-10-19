<script lang="ts">
  import { copyText } from "../lib/clipboard";
  import Icon from "../lib/Icon.svelte";

  let { text, prominent = false }: { text: string; prominent?: boolean } = $props();
  let copied = $state(false);

  async function copy() {
    await copyText(text);
    copied = true;
    setTimeout(() => (copied = false), 1500);
  }
</script>

<button
  type="button"
  class={["inline-flex items-center gap-1 text-xs", prominent ? "rounded-full px-2 py-0.5 font-medium text-foreground hover:bg-muted" : "rounded px-1.5 py-0.5 text-muted-foreground hover:bg-accent hover:text-foreground"]}
  onclick={copy}>
  <Icon name={copied ? "check" : "copy"} class="size-3.5" />{copied ? "已复制" : "复制"}
</button>

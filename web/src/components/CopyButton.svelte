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
  class={["inline-flex items-center gap-1 text-xs", prominent ? "rounded-full bg-muted px-2.5 py-1 font-medium text-foreground hover:bg-accent" : "rounded-md border border-border bg-background px-1.5 py-0.5 text-muted-foreground shadow-xs hover:bg-accent hover:text-foreground"]}
  onclick={copy}>
  <Icon name={copied ? "check" : "copy"} class="size-3.5" />{copied ? "已复制" : "复制"}
</button>

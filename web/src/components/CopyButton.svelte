<script lang="ts">
  import { copyText } from "../lib/clipboard";
  import Icon from "../lib/Icon.svelte";

  let { text, variant = "default" }: { text: string; variant?: "default" | "prominent" | "icon" } = $props();
  let copied = $state(false);

  async function copy() {
    await copyText(text);
    copied = true;
    setTimeout(() => (copied = false), 1500);
  }

  const style = {
    default: "rounded-md border border-border bg-background px-1.5 py-0.5 text-muted-foreground shadow-xs hover:bg-accent hover:text-foreground",
    prominent: "rounded-full bg-muted px-2.5 py-1 font-medium text-foreground hover:bg-accent",
    icon: "rounded-md p-1.5 text-muted-foreground hover:bg-muted hover:text-foreground",
  };
</script>

<button type="button" class={["inline-flex items-center gap-1 text-xs transition-colors", style[variant]]} title={copied ? "已复制" : "复制"} aria-label="复制" onclick={copy}>
  <Icon name={copied ? "check" : "copy"} class="size-3.5" />{#if variant !== "icon"}{copied ? "已复制" : "复制"}{/if}
</button>

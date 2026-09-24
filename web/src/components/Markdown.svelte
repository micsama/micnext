<script lang="ts">
  import { copyText } from "../lib/clipboard";
  import { renderMarkdown } from "../lib/markdown";

  let { source }: { source: string } = $props();
  const html = $derived(renderMarkdown(source));

  async function onclick(e: MouseEvent) {
    const btn = (e.target as Element).closest(".copy-code");
    const code = btn?.closest(".code-block")?.querySelector("pre code");
    if (!btn || !code) return;
    await copyText(code.textContent ?? "");
    btn.textContent = "已复制";
    setTimeout(() => (btn.textContent = "复制"), 1500);
  }
</script>

<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
<div class="prose" {onclick}>{@html html}</div>

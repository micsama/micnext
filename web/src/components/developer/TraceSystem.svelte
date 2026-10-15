<script lang="ts">
  import type { IconName } from "../../lib/Icon.svelte";
  import Markdown from "../Markdown.svelte";
  import CopyButton from "../CopyButton.svelte";
  import TraceCard from "./TraceCard.svelte";
  import { isLong } from "./trace-presentation";

  let { text }: { text: string } = $props();
  let raw = $state(false);

  // 顺序与 mic-core `system_prompt` 一致：instructions → persona → context。
  const LABEL: Record<string, [string, IconName, string]> = {
    instructions: ["内置指令", "file", "固定，不可修改"],
    persona: ["人设与偏好", "user", "设置页可改"],
    context: ["环境与上下文", "folder", "工作目录、渠道、工具提示与压缩摘要"],
  };
  const blocks = $derived([...text.matchAll(/<(\w+)>\n([\s\S]*?)\n<\/\1>/g)].map((m) => ({ tag: m[1]!, body: m[2]! })));
</script>

<div class="pv">
  <div class="pv-bar">
    <span class="seg"><button type="button" class:on={!raw} onclick={() => raw = false}>分块</button><button type="button" class:on={raw} onclick={() => raw = true}>原文</button></span>
    <CopyButton {text} />
  </div>
  {#if raw || blocks.length === 0}<pre>{text}</pre>
  {:else}
    {#each blocks as b (b.tag)}
      {@const [title, icon, note] = LABEL[b.tag] ?? [b.tag, "text", ""]}
      <TraceCard {icon} {title} meta={[note, `${b.body.length} 字`].filter(Boolean).join(" · ")} long={isLong(b.body)} copy={b.body}><Markdown source={b.body} /></TraceCard>
    {/each}
  {/if}
</div>

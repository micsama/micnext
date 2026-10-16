<script lang="ts">
  import type { IconName } from "../../lib/Icon.svelte";
  import type { PromptBlock, PromptPart, SystemPromptView } from "../../api/trace";
  import { CONTEXT_LABEL, contextColor } from "../../lib/context-kind";
  import Markdown from "../Markdown.svelte";
  import CopyButton from "../CopyButton.svelte";
  import TraceCard from "./TraceCard.svelte";
  import { isLong } from "./trace-presentation";

  let { view }: { view: SystemPromptView } = $props();
  let raw = $state(false);

  const BLOCK: Record<PromptBlock["kind"], [string, IconName, string]> = {
    instructions: ["内置指令", "file", "固定，不可修改"],
    persona: ["人设与偏好", "user", "设置页可改"],
    context: ["环境与上下文", "folder", "工作目录、渠道、工具提示与压缩摘要"],
  };
  const body = (parts: PromptPart[]) => parts.map((p) => p.text).join("\n\n");
  /** context 块内相邻同类段合成一节。 */
  function sections(parts: PromptPart[]) {
    const out: { kind: PromptPart["kind"]; parts: PromptPart[] }[] = [];
    for (const p of parts) {
      const last = out.at(-1);
      if (last?.kind === p.kind) last.parts.push(p);
      else out.push({ kind: p.kind, parts: [p] });
    }
    return out;
  }
</script>

<div class="pv">
  <div class="pv-bar">
    <span class="seg"><button type="button" class:on={!raw} onclick={() => raw = false}>分块</button><button type="button" class:on={raw} onclick={() => raw = true}>原文</button></span>
    <CopyButton text={view.text} />
  </div>
  {#if raw}<pre>{view.text}</pre>
  {:else}
    {#each view.blocks as b (b.kind)}
      {@const [title, icon, note] = BLOCK[b.kind]}
      {@const text = body(b.parts)}
      <TraceCard {icon} {title} meta={`${note} · ${text.length} 字`} long={isLong(text)} copy={text}>
        {#if b.kind === "context"}
          {#each sections(b.parts) as s, i (i)}
            <div class="sys-sec">
              <span class="sys-sec-title"><i style:background={contextColor(s.kind)}></i>{CONTEXT_LABEL[s.kind]}</span>
              <Markdown source={body(s.parts)} />
            </div>
          {/each}
        {:else}<Markdown source={text} />{/if}
      </TraceCard>
    {/each}
  {/if}
</div>

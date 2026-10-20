<script lang="ts">
  import type { IconName } from "../../lib/Icon.svelte";
  import type { PromptBlock, PromptPart, SystemPromptView, ToolSpec } from "../../api/trace";
  import { CONTEXT_LABEL, contextColor } from "../../lib/context-kind";
  import Markdown from "../Markdown.svelte";
  import TraceCard from "./TraceCard.svelte";
  import { isLong, jsonHtml } from "./trace-presentation";

  let { view, tab }: { view: SystemPromptView; tab: "blocks" | "raw" | "tools" } = $props();

  const BLOCK: Record<PromptBlock["kind"], [string, IconName, string]> = {
    instructions: ["内置指令", "file", "固定，不可修改"],
    persona: ["人设与偏好", "user", "设置页可改"],
    context: ["环境与上下文", "folder", "工作目录、渠道与压缩摘要"],
  };
  /** 原文里的块标签行，单独着色。 */
  const tags = $derived(new Set(view.blocks.flatMap((b) => [`<${b.kind}>`, `</${b.kind}>`])));
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
  type Param = { name: string; type: string; required: boolean; description: string };
  /** schema 顶层属性；类型取 `type`，联合类型写 `a | b`。 */
  function params(t: ToolSpec): Param[] {
    const props = (t.parameters.properties ?? {}) as Record<string, Record<string, unknown>>;
    const required = new Set((t.parameters.required ?? []) as string[]);
    const typeOf = (p: Record<string, unknown>): string => {
      if (Array.isArray(p.type)) return p.type.join(" | ");
      if (typeof p.type === "string") return p.type === "array" && p.items ? `${typeOf(p.items as Record<string, unknown>)}[]` : p.type;
      const union = (p.anyOf ?? p.oneOf) as Record<string, unknown>[] | undefined;
      if (union) return union.map(typeOf).join(" | ");
      if (Array.isArray(p.enum)) return p.enum.map((v) => JSON.stringify(v)).join(" | ");
      return "any";
    };
    return Object.entries(props).map(([name, p]) => ({
      name, type: typeOf(p), required: required.has(name), description: typeof p.description === "string" ? p.description : "",
    }));
  }
</script>

<div class="cards">
  {#if tab === "tools"}
    {#each view.tools as t (t.name)}
      {@const ps = params(t)}
      {@const schema = JSON.stringify(t.parameters, null, 2)}
      <TraceCard icon="wrench" title={t.name} mono meta="{ps.length} 个参数 · {t.description.length + schema.length} 字" copy={JSON.stringify(t, null, 2)}>
        <p class="tool-desc">{t.description}</p>
        {#if ps.length > 0}
          <ul class="tool-params">
            {#each ps as p (p.name)}
              <li><code>{p.name}</code><span class="tool-type">{p.type}{p.required ? "" : "，可选"}</span>{#if p.description}<span class="tool-pdesc">{p.description}</span>{/if}</li>
            {/each}
          </ul>
        {/if}
        <details class="tool-schema"><summary>原始 schema</summary><pre class="hl">{@html jsonHtml(schema)}</pre></details>
      </TraceCard>
    {:else}<p class="hint">该会话没有工具</p>{/each}
  {:else if tab === "raw"}
    <TraceCard icon="file" title="原文" meta="{view.text.length} 字" copy={view.text}>
      <pre>{#each view.text.split("\n") as line, i (i)}{#if tags.has(line)}<span class="sys-tag">{line}</span>{:else}{line}{/if}{"\n"}{/each}</pre>
    </TraceCard>
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

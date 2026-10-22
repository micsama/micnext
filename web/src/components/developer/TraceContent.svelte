<script lang="ts">
  import type { ContentPart, ExecOutcome, Message } from "../../api/types";
  import type { Usage } from "../../api/trace";
  import { outcomeText, partsText } from "../outcome";
  import { isLong, jsonHtml } from "./trace-presentation";
  import Markdown from "../Markdown.svelte";
  import HistoryImage from "../HistoryImage.svelte";
  import TraceCard from "./TraceCard.svelte";

  let { message, usage = null }: { message: Message; usage?: Usage | null } = $props();
  const body = $derived(message.body);
  const args = (v: unknown) => JSON.stringify(v, null, 2);
  const chars = (s: string) => `${s.length} 字`;
  const plain = (parts: ContentPart[]) => parts.map((p) => ("Text" in p ? p.Text.text : "")).filter(Boolean).join("\n");

  const reply = $derived(body.kind === "Reply" ? body.blocks : []);
  const count = (f: (b: (typeof reply)[number]) => boolean) => reply.filter(f).length;
  const calls = $derived(count((b) => "ToolCall" in b));
  // token 只在能唯一对应时标：推理取上游推理数；正文仅在无工具调用且只有一段时取输出减推理。
  const thinkTok = $derived(usage?.reasoning_tokens != null && count((b) => "Reasoning" in b) === 1 ? usage.reasoning_tokens : null);
  const textTok = $derived(usage && count((b) => "Text" in b) === 1 && calls === 0 ? usage.output_tokens - (usage.reasoning_tokens ?? 0) : null);
  const tok = (n: number | null) => (n === null ? "" : ` · ${n} tok`);

  const text = $derived.by(() => {
    switch (body.kind) {
      case "UserInput": return partsText(body.parts);
      case "HarnessNote": case "Notification": case "JobNotice": case "Trigger": return body.text;
      case "ToolResult": return "Terminal" in body.outcome ? outcomeText(body.outcome.Terminal) : `已派发：${body.outcome.Dispatched.exec_id}`;
      case "Completion": return outcomeText(body.outcome);
      case "Boundary": return body.boundary === "UserClear" ? "清空上下文" : body.boundary.Compaction.summary;
      case "SideOutput": return body.text;
      case "Command": return body.command === "Clear" ? "/clear" : "Compact" in body.command ? `/compact ${body.command.Compact.text ?? ""}` : `撤回压缩 #${body.command.RevokeCompaction.boundary_id}`;
      case "Reply": return body.blocks.map((b) => "Text" in b ? b.Text.text : "Reasoning" in b ? "Visible" in b.Reasoning ? b.Reasoning.Visible.text : "[隐藏推理]" : `${b.ToolCall.name}\n${args(b.ToolCall.args)}`).join("\n\n");
    }
  });
</script>

{#snippet media(items: ContentPart[])}
  {@const extra = items.filter((p) => !("Text" in p))}
  {#if extra.length}<div class="thumbs">{#each extra as p, i (i)}{#if "Image" in p}<HistoryImage sessionId={message.session_id} imageId={p.Image.id} />{:else if "File" in p}<span class="cap" title={p.File.path}>[文件：{p.File.path.split(/[\\/]/).at(-1)}]</span>{/if}{/each}</div>{/if}
{/snippet}

{#snippet result(title: string, outcome: ExecOutcome)}
  {#if "Completed" in outcome}
    {@const t = plain(outcome.Completed.output)}
    <TraceCard icon="terminal" {title} tone="tool" meta={chars(t)} long={isLong(t)} copy={t || undefined}>
      {#if t}<pre class="hl">{@html jsonHtml(t)}</pre>{:else if outcome.Completed.output.length === 0}<p class="muted">（无输出）</p>{/if}
      {@render media(outcome.Completed.output)}
    </TraceCard>
  {:else}
    {@const msg = "Failed" in outcome ? outcome.Failed.message : outcome.Cancelled.message}
    <TraceCard icon="error" {title} tone="bad" meta={"Failed" in outcome ? "执行报错" : "已取消"} long={isLong(msg)} copy={msg}><pre>{msg}</pre></TraceCard>
  {/if}
{/snippet}

<div class="pv">
  {#if body.kind === "Reply"}
    {#each body.blocks as block, i (i)}
      {#if "Text" in block}
        <TraceCard icon="text" title={block.Text.phase === "commentary" ? "过程说明" : "正文"} tone="ai" meta={chars(block.Text.text) + tok(textTok)} long={isLong(block.Text.text)} copy={block.Text.text}><Markdown source={block.Text.text} /></TraceCard>
      {:else if "Reasoning" in block}
        {@const visible = "Visible" in block.Reasoning ? block.Reasoning.Visible.text : null}
        <TraceCard icon="bulb" title="推理" tone="think" fold="closed" meta={(visible === null ? "已加密" : chars(visible)) + tok(thinkTok)} long={visible !== null && isLong(visible)} copy={visible ?? undefined}>
          {#if visible !== null}<Markdown source={visible} />{:else}<p class="muted">上游只返回了加密推理</p>{/if}
        </TraceCard>
      {:else}
        {@const a = args(block.ToolCall.args)}
        <TraceCard icon="wrench" title={block.ToolCall.name} mono tone="tool" meta={block.ToolCall.id} long={isLong(a)} copy={a}><pre class="hl">{@html jsonHtml(a)}</pre></TraceCard>
      {/if}
    {/each}
  {:else if body.kind === "UserInput"}
    {@const t = plain(body.parts)}
    <TraceCard icon="user" title="用户输入" tone="user" meta={chars(t)} long={isLong(t)} copy={t || undefined}>{#if t}<Markdown source={t} />{/if}{@render media(body.parts)}</TraceCard>
  {:else if body.kind === "Trigger"}
    <TraceCard icon="clock" title="定时任务指令" tone="user" meta={chars(text)} long={isLong(text)} copy={text}><Markdown source={text} /></TraceCard>
  {:else if body.kind === "ToolResult"}
    {#if "Terminal" in body.outcome}{@render result(`结果 · ${body.tool_name}`, body.outcome.Terminal)}
    {:else}<TraceCard icon="unknown" title="结果 · {body.tool_name}" meta="已派发"><p>执行编号 <span class="mono">{body.outcome.Dispatched.exec_id}</span>，等待异步完成</p></TraceCard>{/if}
  {:else if body.kind === "Completion"}
    {@render result(`完成 · ${body.tool_name}`, body.outcome)}
  {:else if body.kind === "JobNotice"}
    <TraceCard icon="info" title="后台通知 · {body.tool_name}" meta={chars(text)} long={isLong(text)} copy={text}><pre>{text}</pre></TraceCard>
  {:else if body.kind === "Boundary"}
    <TraceCard icon="layers" title={body.boundary === "UserClear" ? "清空上下文" : "压缩摘要"} tone="compact" meta={chars(text)} long={isLong(text)} copy={text}><Markdown source={text} /></TraceCard>
  {:else if body.kind === "Command"}
    <TraceCard icon="terminal" title="指令" copy={text}><pre>{text}</pre></TraceCard>
  {:else}
    <TraceCard icon="info" title={body.kind === "Notification" ? `通知 · ${body.source}` : "系统备注"} meta={chars(text)} long={isLong(text)} copy={text}><Markdown source={text} /></TraceCard>
  {/if}
</div>

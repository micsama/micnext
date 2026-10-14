<script lang="ts">
  import type { MessageBody } from "../../api/types";
  import { outcomeText, partsText } from "../outcome";
  import Markdown from "../Markdown.svelte";
  import CopyButton from "../CopyButton.svelte";
  let { body }: { body: MessageBody } = $props();
  const text = $derived.by(() => {
    switch (body.kind) {
      case "UserInput": return partsText(body.parts);
      case "HarnessNote": case "Notification": return body.text;
      case "ToolResult": return "Terminal" in body.outcome ? outcomeText(body.outcome.Terminal) : `已派发：${body.outcome.Dispatched.exec_id}`;
      case "Completion": return outcomeText(body.outcome);
      case "Boundary": return body.boundary === "UserClear" ? "清空上下文" : body.boundary.Compaction.summary;
      case "Command": return body.command === "Clear" ? "/clear" : "Compact" in body.command ? `/compact ${body.command.Compact.text ?? ""}` : `撤回压缩 #${body.command.RevokeCompaction.boundary_id}`;
      case "Reply": return body.blocks.map((b) => "Text" in b ? b.Text.text : "Reasoning" in b ? "Visible" in b.Reasoning ? b.Reasoning.Visible.text : "[隐藏推理]" : `${b.ToolCall.name}\n${JSON.stringify(b.ToolCall.args, null, 2)}`).join("\n\n");
    }
  });
</script>
<div class="mb-1 flex justify-end"><CopyButton {text} /></div>
{#if body.kind === "Reply"}
  {#each body.blocks as block, i (i)}
    {#if "Text" in block}<Markdown source={block.Text.text} />
    {:else if "Reasoning" in block}
      <details class="my-2 rounded border p-2"><summary class="cursor-pointer text-xs text-muted-foreground">推理</summary>
        {#if "Visible" in block.Reasoning}<Markdown source={block.Reasoning.Visible.text} />{:else}<p>隐藏推理</p>{/if}
      </details>
    {:else}
      <p class="mt-2 text-xs text-muted-foreground">工具 · {block.ToolCall.name}</p>
      <pre class="overflow-auto text-xs whitespace-pre-wrap">{JSON.stringify(block.ToolCall.args, null, 2)}</pre>
    {/if}
  {/each}
{:else if body.kind === "ToolResult" || body.kind === "Completion"}
  <pre class="text-xs break-words whitespace-pre-wrap">{text}</pre>
{:else}
  <Markdown source={text} />
{/if}

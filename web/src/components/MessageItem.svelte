<script lang="ts">
  import type { Message, ToolResultOutcome } from "../api/types";
  import { formatTime } from "../lib/format";
  import Icon from "../lib/Icon.svelte";
  import HistoryImage from "./HistoryImage.svelte";
  import { outcomeText } from "./outcome";
  import ReplyView from "./ReplyView.svelte";
  import ToolRow from "./ToolRow.svelte";

  let {
    message,
    results,
    callIds,
    running,
  }: {
    message: Message;
    results: Map<string, ToolResultOutcome>;
    callIds: Set<string>;
    running: boolean;
  } = $props();

  const body = $derived(message.body);
  const time = $derived(formatTime(message.created_at));
  const basename = (path: string) => path.split(/[\\/]/).at(-1) ?? path;
</script>

{#if body.kind === "UserInput"}
  <div class="group relative flex flex-col items-end">
    <div class="max-w-[85%] rounded-2xl bg-raised px-4 py-2 whitespace-pre-wrap break-words">
      {#each body.parts as part, i (i)}
        {#if "Text" in part}
          {part.Text.text}
        {:else if "Image" in part}
          <HistoryImage sessionId={message.session_id} imageId={part.Image.id} />
        {:else}
          <span class="my-0.5 inline-flex items-center gap-1 rounded bg-bg px-1.5 py-0.5 text-xs text-muted">
            <Icon name="file" class="size-3" />{basename(part.File.path)}
          </span>
        {/if}
      {/each}
    </div>
    <span class="pointer-events-none absolute top-full right-1 text-xs text-muted opacity-0 transition-opacity group-hover:opacity-100">{time}</span>
  </div>
{:else if body.kind === "Reply"}
  <div class="group relative">
    <ReplyView blocks={body.blocks} {results} {running} />
    <span class="pointer-events-none absolute top-full left-0 text-xs text-muted opacity-0 transition-opacity group-hover:opacity-100">{time} · {body.model}</span>
  </div>
{:else if body.kind === "ToolResult"}
  {#if !callIds.has(body.tool_call_id)}
    <ToolRow name={body.tool_name} result={body.outcome} />
  {/if}
{:else if body.kind === "Completion"}
  <details class="rounded-lg border border-line text-sm">
    <summary class="cursor-pointer px-3 py-1.5 text-muted select-none">后台任务完成：{body.tool_name}</summary>
    <pre class="max-h-80 overflow-auto border-t border-line px-3 py-2 font-mono text-xs whitespace-pre-wrap">{outcomeText(
        body.outcome,
      )}</pre>
  </details>
{:else if body.kind === "Notification"}
  <p class="text-center text-xs text-muted" title="{body.source} · {time}">{body.text}</p>
{:else if body.kind === "Boundary"}
  <div class="flex items-center gap-3 text-xs text-muted" title={time}>
    <div class="h-px flex-1 bg-line"></div>
    {body.boundary === "UserClear" ? "上下文已清空" : "以上内容已压缩"}
    <div class="h-px flex-1 bg-line"></div>
  </div>
{/if}

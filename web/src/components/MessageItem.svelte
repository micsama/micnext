<script lang="ts">
  import type { Message, ToolResultOutcome } from "../api/types";
  import { formatTime } from "../lib/format";
  import Icon from "../lib/Icon.svelte";
  import HistoryImage from "./HistoryImage.svelte";
  import { outcomeText } from "./outcome";
  import type { SessionView } from "../state/session.svelte";
  import ContextBoundaryView from "./ContextBoundaryView.svelte";
  import ReplyView from "./ReplyView.svelte";
  import ToolRow from "./ToolRow.svelte";
  import JobRef from "./JobRef.svelte";

  let {
    message,
    view,
    results,
    callIds,
    running,
  }: {
    message: Message;
    view: SessionView;
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
    <div class="max-w-[85%] rounded-2xl bg-accent px-4 py-2 whitespace-pre-wrap break-words">
      {#each body.parts as part, i (i)}
        {#if "Text" in part}
          {part.Text.text}
        {:else if "Image" in part}
          <HistoryImage sessionId={message.session_id} imageId={part.Image.id} />
        {:else}
          <span class="my-0.5 inline-flex items-center gap-1 rounded bg-background px-1.5 py-0.5 text-xs text-muted-foreground">
            <Icon name="file" class="size-3" />{basename(part.File.path)}
          </span>
        {/if}
      {/each}
    </div>
    <span class="pointer-events-none absolute top-full right-1 text-xs text-muted-foreground opacity-0 transition-opacity group-hover:opacity-100">{time}</span>
  </div>
{:else if body.kind === "Command"}
  <p class="text-sm text-muted-foreground" title={time}>
    {#if body.command === "Clear"}/clear
    {:else if "Compact" in body.command}/compact{body.command.Compact.text ? ` ${body.command.Compact.text}` : ""}
    {:else}撤回压缩{/if}
  </p>
{:else if body.kind === "Reply"}
  <ReplyView blocks={body.blocks} {results} {running} meta="{time} · {body.model}" stats={body.stats} />
{:else if body.kind === "ToolResult"}
  {#if !callIds.has(body.tool_call_id)}
    <ToolRow name={body.tool_name} result={body.outcome} />
  {/if}
{:else if body.kind === "Completion"}
  <details class="rounded-lg border border-border text-sm">
    <summary class="cursor-pointer px-3 py-1.5 text-muted-foreground select-none">后台任务完成：{body.tool_name} <JobRef id={body.exec_id} /></summary>
    <pre class="max-h-80 overflow-auto border-t border-border px-3 py-2 font-mono text-xs whitespace-pre-wrap">{outcomeText(
        body.outcome,
      )}</pre>
  </details>
{:else if body.kind === "Notification"}
  <p class="w-fit max-w-full rounded-lg bg-muted/70 px-2.5 py-1 text-xs leading-5 whitespace-pre-wrap break-words text-muted-foreground" title="{body.source} · {time}">{body.text}</p>
{:else if body.kind === "Boundary"}
  {#if body.boundary === "UserClear"}
    <div class="flex items-center gap-3 text-xs text-muted-foreground" title={time}><div class="h-px flex-1 bg-border"></div>上下文已清空<div class="h-px flex-1 bg-border"></div></div>
  {:else}
    <ContextBoundaryView id={message.id} result={body.boundary.Compaction} {view} />
  {/if}
{/if}

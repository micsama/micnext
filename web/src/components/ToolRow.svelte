<script lang="ts">
  import type { ExecOutcome, ToolResultOutcome } from "../api/types";
  import { argsSummary, prettyArgs } from "../lib/format";
  import Icon from "../lib/Icon.svelte";
  import { outcomeText } from "./outcome";

  let {
    name,
    args,
    result,
    running = false,
  }: { name: string; args?: unknown; result?: ToolResultOutcome; running?: boolean } = $props();

  const terminal: ExecOutcome | undefined = $derived(result && "Terminal" in result ? result.Terminal : undefined);
  const status = $derived.by(() => {
    if (!result) return running ? "running" : "none";
    if (!terminal) return "dispatched";
    return "Completed" in terminal ? "ok" : "fail";
  });
  const summary = $derived(args === undefined ? "" : argsSummary(args));
</script>

<details class="group/tool my-1 rounded-lg border border-line text-sm">
  <summary class="flex cursor-pointer list-none items-center gap-2 px-3 py-1.5 select-none">
    <Icon name="wrench" class="size-3.5 shrink-0 text-muted" />
    <span class="shrink-0 font-medium">{name}</span>
    <span class="min-w-0 flex-1 truncate font-mono text-xs text-muted">{summary}</span>
    {#if status === "running"}
      <Icon name="loader" class="size-3.5 shrink-0 animate-spin text-muted" />
    {:else if status === "ok"}
      <Icon name="check" class="size-3.5 shrink-0 text-green-600" />
    {:else if status === "fail"}
      <Icon name="x" class="size-3.5 shrink-0 text-danger" />
    {:else if status === "dispatched"}
      <span class="shrink-0 text-xs text-muted">后台执行中</span>
    {/if}
    <Icon name="chevron" class="size-3.5 shrink-0 text-muted transition-transform group-open/tool:rotate-90" />
  </summary>
  <div class="space-y-2 border-t border-line px-3 py-2">
    {#if args !== undefined}
      <pre class="max-h-60 overflow-auto rounded bg-code p-2 font-mono text-xs whitespace-pre-wrap">{prettyArgs(args)}</pre>
    {/if}
    {#if terminal}
      <pre
        class="max-h-80 overflow-auto rounded bg-code p-2 font-mono text-xs whitespace-pre-wrap"
        class:text-danger={status === "fail"}>{outcomeText(terminal)}</pre>
    {:else if result && "Dispatched" in result}
      <p class="text-xs text-muted">已转入后台（{result.Dispatched.exec_id}），完成后另行通知</p>
    {:else}
      <p class="text-xs text-muted">{running ? "执行中…" : "没有结果"}</p>
    {/if}
  </div>
</details>

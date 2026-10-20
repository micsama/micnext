<script lang="ts" module>
  /** 工具行与后台回报行共用的列布局：图标 · 定宽工具名 · 描述 · 参数 · 状态，逐行对齐。 */
  export const STEP_ROW = "flex h-8 cursor-pointer list-none items-center gap-2 px-2.5 select-none [&::-webkit-details-marker]:hidden";
  export const STEP_NAME = "w-20 shrink-0 truncate font-medium text-muted-foreground";
</script>

<script lang="ts">
  import type { ExecOutcome, ToolResultOutcome } from "../api/types";
  import { argsPreview, prettyArgs } from "../lib/format";
  import Icon, { type IconName } from "../lib/Icon.svelte";
  import JobRef from "./JobRef.svelte";
  import { outcomeText } from "./outcome";

  let {
    name,
    args,
    result,
    running = false,
  }: { name: string; args?: unknown; result?: ToolResultOutcome; running?: boolean } = $props();

  const ICONS: Record<string, IconName> = {
    bash: "terminal",
    read: "file",
    write: "file",
    edit: "file",
    grep: "search",
    glob: "search",
    web_fetch: "external",
    agent: "bot",
    jobs: "layers",
    job_output: "layers",
    job_stop: "layers",
  };

  const terminal: ExecOutcome | undefined = $derived(result && "Terminal" in result ? result.Terminal : undefined);
  const status = $derived.by(() => {
    if (!result) return running ? "running" : "none";
    if (!terminal) return "dispatched";
    return "Completed" in terminal ? "ok" : "fail";
  });
  const preview = $derived(argsPreview(args));
</script>

<details class="group/tool my-1 rounded-lg bg-muted/60 text-xs open:bg-transparent open:ring-1 open:ring-border">
  <summary class="{STEP_ROW} rounded-lg hover:bg-muted">
    <Icon name={ICONS[name] ?? "wrench"} class="size-3.5 shrink-0 text-muted-foreground" />
    <span class={STEP_NAME}>{name}</span>
    {#if preview.description}<span class="max-w-[60%] shrink-0 truncate">{preview.description}</span>{/if}
    <span class="min-w-0 flex-1 truncate font-mono text-muted-foreground/80">{preview.detail}</span>
    {#if status === "running"}
      <Icon name="loader" class="size-3.5 shrink-0 animate-spin text-muted-foreground" />
    {:else if status === "ok"}
      <Icon name="check" class="size-3.5 shrink-0 text-success" />
    {:else if status === "fail"}
      <Icon name="x" class="size-3.5 shrink-0 text-destructive" />
    {:else if result && "Dispatched" in result}
      <JobRef id={result.Dispatched.exec_id} />
    {/if}
  </summary>
  <div class="space-y-2 border-t border-border px-2.5 py-2">
    {#if args !== undefined}
      <pre class="max-h-60 overflow-auto rounded bg-code p-2 font-mono whitespace-pre-wrap">{prettyArgs(args)}</pre>
    {/if}
    {#if terminal}
      <pre
        class="max-h-80 overflow-auto rounded bg-code p-2 font-mono whitespace-pre-wrap"
        class:text-destructive={status === "fail"}>{outcomeText(terminal)}</pre>
    {:else if result && "Dispatched" in result}
      <p class="text-muted-foreground">已转入后台（{result.Dispatched.exec_id}），完成后另行通知</p>
    {:else}
      <p class="text-muted-foreground">{running ? "执行中…" : "没有结果"}</p>
    {/if}
  </div>
</details>

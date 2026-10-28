<script lang="ts">
  import type { ContentPart, ExecOutcome, FileRef, Message, ToolResultOutcome } from "../api/types";
  import { fetchFile } from "../api/client";
  import { formatTime } from "../lib/format";
  import Icon from "../lib/Icon.svelte";
  import FileCard from "./FileCard.svelte";
  import HistoryImage from "./HistoryImage.svelte";
  import { outcomeText } from "./outcome";
  import type { SessionView } from "../state/session.svelte";
  import ContextBoundaryView from "./ContextBoundaryView.svelte";
  import TrimBoundaryView from "./TrimBoundaryView.svelte";
  import CopyButton from "./CopyButton.svelte";
  import ReplyView from "./ReplyView.svelte";
  import ToolRow, { STEP_NAME, STEP_ROW } from "./ToolRow.svelte";
  import JobRef from "./JobRef.svelte";

  let {
    message,
    view,
    results,
    running,
  }: {
    message: Message;
    view: SessionView;
    results: Map<string, ToolResultOutcome>;
    running: boolean;
  } = $props();

  const body = $derived(message.body);
  const time = $derived(formatTime(message.created_at));
  const queued = $derived(view.isQueued(message.id));
  const basename = (path: string) => path.split(/[\\/]/).at(-1) ?? path;
  const fileLoad = (index: number) => () => fetchFile(`/sessions/${message.session_id}/messages/${message.id}/files/${index}`);
  const VERDICT = { Completed: "完成", Failed: "失败", Cancelled: "取消", TimedOut: "超时" } as const;
  const outcomeKind = (o: ExecOutcome) => ("Completed" in o ? "Completed" : "TimedOut" in o ? "TimedOut" : "Failed" in o ? "Failed" : "Cancelled");
</script>

{#snippet fileCards(files: FileRef[], cls: string)}
  <div class={["flex flex-wrap gap-2", cls]}>
    {#each files as file, i (i)}
      <FileCard name={basename(file.path)} size={file.size_bytes} load={fileLoad(i)} />
    {/each}
  </div>
{/snippet}

{#snippet queuedTag()}
  <span class="inline-flex items-center gap-1 text-xs text-muted-foreground"><Icon name="clock" class="size-3" />排队中</span>
{/snippet}

{#if body.kind === "UserInput"}
  {@const attachments = body.parts.filter((p) => "Image" in p || "File" in p)}
  {@const skills = body.parts.flatMap((p) => ("Skill" in p ? [p.Skill] : []))}
  {@const fileIndex = (part: ContentPart) => body.parts.filter((p) => "File" in p).findIndex((p) => p === part)}
  {@const text = body.parts.flatMap((p) => ("Text" in p ? [p.Text.text] : [])).join("")}
  <div class="group relative flex flex-col items-end gap-1.5">
    {#if attachments.length > 0}
      <div class={["flex max-w-[85%] flex-wrap items-end justify-end gap-2", queued && "opacity-60"]}>
        {#each attachments as part, i (i)}
          {#if "Image" in part}
            <HistoryImage sessionId={message.session_id} imageId={part.Image.id} />
          {:else if "File" in part}
            <FileCard name={basename(part.File.path)} size={part.File.size_bytes} title={part.File.path} load={fileLoad(fileIndex(part))} />
          {/if}
        {/each}
      </div>
    {/if}
    {#each skills as skill, i (i)}
      <details class={["group/skill w-full max-w-[85%] rounded-lg border border-border text-xs", queued && "opacity-60"]}>
        <summary class="{STEP_ROW} justify-end rounded-lg text-muted-foreground hover:bg-muted">
          <Icon name="zap" class="size-3.5 shrink-0" />已加载 skill <span class="font-mono text-foreground">{skill.name}</span><Icon name="chevron" class="size-3 transition-transform group-open/skill:rotate-90" />
        </summary>
        <pre class="max-h-80 overflow-auto border-t border-border px-2.5 py-2 font-mono whitespace-pre-wrap break-words">{skill.text}</pre>
      </details>
    {/each}
    {#if text}
      <!-- 复制放在气泡左侧：下方只留时间，悬停不压住下一条。 -->
      <div class="relative max-w-[85%]">
        <div class={["rounded-2xl bg-accent px-4 py-2 whitespace-pre-wrap break-words", queued && "opacity-60"]}>{text}</div>
        <div class="absolute right-full bottom-0.5 pr-1 opacity-0 transition-opacity group-hover:opacity-100 focus-within:opacity-100"><CopyButton {text} variant="icon" /></div>
      </div>
    {/if}
    {#if queued}{@render queuedTag()}{/if}
    <span class="pointer-events-none absolute top-full right-1 text-xs text-muted-foreground opacity-0 transition-opacity group-hover:opacity-100">{time}</span>
  </div>
{:else if body.kind === "Trigger"}
  <!-- 每次触发的指令相同：默认只露分隔条，点开看指令。 -->
  <details class="group">
    <summary class="flex cursor-pointer list-none items-center gap-3 text-xs text-muted-foreground [&::-webkit-details-marker]:hidden">
      <span class="h-px flex-1 bg-border"></span>
      <span class="inline-flex items-center gap-1.5 rounded-full border border-border bg-background px-2.5 py-1 transition-colors group-hover:text-foreground">
        <Icon name="clock" class="size-3" />定时触发 · {time}<Icon name="chevron" class="size-3 transition-transform group-open:rotate-90" />
      </span>
      <span class="h-px flex-1 bg-border"></span>
    </summary>
    <p class="mx-auto mt-2 max-w-[85%] rounded-lg bg-muted/50 px-3 py-2 text-sm whitespace-pre-wrap break-words text-muted-foreground">{body.text}</p>
  </details>
{:else if body.kind === "Command"}
  <p class={["flex items-center gap-2 text-sm text-muted-foreground", queued && "opacity-60"]} title={time}>
    <span>
      {#if body.command === "Clear"}/clear
      {:else if "Compact" in body.command}/compact{body.command.Compact.text ? ` ${body.command.Compact.text}` : ""}
      {:else}撤回压缩{/if}
    </span>
    {#if queued}{@render queuedTag()}{/if}
  </p>
{:else if body.kind === "Reply"}
  <ReplyView {view} replyId={message.id} blocks={body.blocks} {results} {running} {time} model={body.model} stats={body.stats} />
{:else if body.kind === "ToolResult"}
  <ToolRow name={body.tool_name} result={body.outcome} />
{:else if body.kind === "Completion"}
  <!-- 后台回报不是本轮调用：虚线框与工具胶囊区分。 -->
  {@const job = view.jobs.find((j) => String(j.id) === body.exec_id)}
  <details class="rounded-lg border border-dashed border-border text-xs">
    <summary class="{STEP_ROW} rounded-lg hover:bg-muted">
      <Icon name="layers" class="size-3.5 shrink-0 text-muted-foreground" />
      <span class={STEP_NAME}>{body.tool_name}</span>
      <span class="shrink-0 text-muted-foreground">后台{VERDICT[outcomeKind(body.outcome)]}</span>
      <span class="min-w-0 flex-1 truncate">{job?.description ?? ""}</span>
      <JobRef id={body.exec_id} />
    </summary>
    <pre class="max-h-80 overflow-auto border-t border-dashed border-border px-2.5 py-2 font-mono whitespace-pre-wrap" class:text-destructive={!("Completed" in body.outcome)}>{outcomeText(
        body.outcome,
      )}</pre>
  </details>
{:else if body.kind === "JobNotice"}
  {@const job = view.jobs.find((j) => String(j.id) === body.exec_id)}
  <details class="rounded-lg border border-dashed border-border text-xs">
    <summary class="{STEP_ROW} rounded-lg hover:bg-muted">
      <Icon name="layers" class="size-3.5 shrink-0 text-muted-foreground" />
      <span class={STEP_NAME}>{body.tool_name}</span>
      <span class="shrink-0 text-muted-foreground">后台通知</span>
      <span class="min-w-0 flex-1 truncate">{job?.description ?? ""}</span>
      <JobRef id={body.exec_id} />
    </summary>
    <pre class="max-h-80 overflow-auto border-t border-dashed border-border px-2.5 py-2 font-mono whitespace-pre-wrap">{body.text}</pre>
  </details>
{:else if body.kind === "Notification"}
  <p class="w-fit max-w-full rounded-lg bg-muted/70 px-2.5 py-1 text-xs leading-5 whitespace-pre-wrap break-words text-muted-foreground" title="{body.source} · {time}">{body.text}</p>
  {#if body.files.length > 0}{@render fileCards(body.files, "justify-center")}{/if}
{:else if body.kind === "Attachment"}
  {@render fileCards(body.files, "")}
{:else if body.kind === "Boundary"}
  {#if body.boundary === "UserClear"}
    <div class="flex items-center gap-3 text-xs text-muted-foreground" title={time}><div class="h-px flex-1 bg-border"></div>上下文已清空<div class="h-px flex-1 bg-border"></div></div>
  {:else if "Trim" in body.boundary}
    <TrimBoundaryView id={message.id} trim={body.boundary.Trim} {view} />
  {:else}
    <ContextBoundaryView id={message.id} result={body.boundary.Compaction} {view} />
  {/if}
{/if}

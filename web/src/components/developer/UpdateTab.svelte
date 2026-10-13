<script lang="ts">
  import { onMount } from "svelte";
  import type { UpdateStage } from "../../api/developer";
  import Icon from "../../lib/Icon.svelte";
  import { updater } from "../../state/update.svelte";

  const STAGES: Record<UpdateStage, string> = {
    checking: "检查工作区",
    pulling: "拉取代码",
    building: "构建",
    restarting: "重启",
  };

  let outputEl = $state<HTMLElement | null>(null);
  const status = $derived(updater.status);
  const running = $derived(updater.running);
  const reconnecting = $derived(updater.reconnecting);
  const verdict = $derived(updater.verdict);
  const error = $derived(updater.error);
  const output = $derived(status !== null && "output" in status ? status.output : "");
  const short = (sha: string) => sha.slice(0, 7);
  const mb = (bytes: number) => `${(bytes / 1024 / 1024).toFixed(1)} MB`;
  /** 新版大小，旧版已知时附相对变化。 */
  function sizeText(size: number | null, from: number | null): string {
    if (size === null) return "";
    if (from === null || from === 0) return ` · ${mb(size)}`;
    const pct = ((size - from) / from) * 100;
    const delta = Math.abs(pct) < 0.05 ? "±0%" : `${pct > 0 ? "+" : "−"}${Math.abs(pct).toFixed(1)}%`;
    return ` · ${mb(size)}（${delta}）`;
  }

  onMount(() => void updater.refresh());

  $effect(() => {
    void output;
    if (outputEl) outputEl.scrollTop = outputEl.scrollHeight;
  });
</script>

<div class="flex min-h-0 flex-1 flex-col gap-3 p-4 text-sm">
  {#if status?.state === "unavailable"}
    <p class="text-muted-foreground">
      未启用一键更新。在配置文件 <code class="font-mono">[gateway]</code> 里加上
      <code class="font-mono">update_repo = "&lt;部署仓库绝对路径&gt;"</code> 后重启。
    </p>
  {:else if status}
    <div class="flex flex-wrap items-center gap-3">
      <button
        type="button"
        disabled={running || reconnecting}
        class="inline-flex items-center gap-1 rounded-md bg-primary px-3 py-1 text-primary-foreground disabled:opacity-50"
        onclick={() => updater.start()}>
        {#if running || reconnecting}<Icon name="loader" class="size-3.5 animate-spin" />{/if}检查并更新
      </button>
      <span class="text-muted-foreground">
        {#if reconnecting}
          正在重启，等待服务恢复…
        {:else if status.state === "checking"}
          {STAGES.checking}…
        {:else if status.state === "pulling"}
          {STAGES.pulling}（第 {status.attempt} 次）· 当前 {short(status.from)}
        {:else if status.state === "building"}
          {STAGES.building}：{short(status.from)} → {short(status.to)}
        {:else if status.state === "restarting"}
          {STAGES.restarting}：{short(status.from)} → {short(status.to)}{sizeText(status.to_size, status.from_size)}
        {:else if status.state === "up_to_date"}
          已是最新（{short(status.commit)}），未重启。
        {:else if status.state === "idle" && !verdict}
          当前运行 {status.running ? short(status.running) : "未知版本（非 build.sh 构建）"}{sizeText(status.size, null)}
        {/if}
      </span>
    </div>
    {#if verdict && status.state === "idle" && !reconnecting}
      {#if verdict.running === verdict.target}
        <p class="text-emerald-600 dark:text-emerald-400">
          已更新并重启，当前运行 {short(verdict.target)}{sizeText(verdict.size, verdict.fromSize)}。
        </p>
      {:else}
        <p class="text-destructive">
          已重启，但当前运行 {verdict.running ? short(verdict.running) : "未知版本"}，不是目标 {short(verdict.target)}。
        </p>
      {/if}
    {/if}
    {#if status.state === "failed" && !reconnecting}
      <div class="text-destructive">
        <p>{STAGES[status.stage]}失败：{status.error}</p>
        {#if status.from}
          <p class="text-xs">更新前提交 <code class="font-mono">{status.from}</code>，旧服务仍在运行。</p>
        {/if}
      </div>
    {/if}
  {/if}
  {#if error}
    <p class="text-destructive">{error}</p>
  {/if}
  {#if output}
    <pre
      bind:this={outputEl}
      class="min-h-0 flex-1 overflow-auto rounded-md border bg-muted p-3 font-mono text-xs break-all whitespace-pre-wrap">{output}</pre>
  {/if}
</div>

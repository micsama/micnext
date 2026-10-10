<script lang="ts">
  import { onMount } from "svelte";
  import { ApiError, getUpdateStatus, startUpdate } from "../../api/client";
  import type { UpdateStage, UpdateStatus } from "../../api/developer";
  import Icon from "../../lib/Icon.svelte";
  import { confirm } from "../../state/confirm.svelte";

  const POLL_MS = 1000;
  const STAGES: Record<UpdateStage, string> = {
    checking: "检查工作区",
    pulling: "拉取代码",
    building: "构建",
    restarting: "重启",
  };

  let status = $state.raw<UpdateStatus | null>(null);
  let error = $state<string | null>(null);
  /** 已请求重启，等待服务重新可连。 */
  let reconnecting = $state(false);
  let recovered = $state(false);
  let outputEl = $state<HTMLElement | null>(null);

  const running = $derived(
    status !== null && ["checking", "pulling", "building", "restarting"].includes(status.state),
  );
  const output = $derived(status !== null && "output" in status ? status.output : "");
  const short = (sha: string) => sha.slice(0, 7);

  async function refresh() {
    try {
      const next = await getUpdateStatus();
      if (reconnecting) {
        reconnecting = false;
        recovered = true;
      }
      status = next;
      error = null;
    } catch (e) {
      if (status?.state === "restarting" && e instanceof ApiError && e.status === 0) {
        reconnecting = true;
      } else {
        error = (e as Error).message;
      }
    }
  }

  onMount(() => {
    void refresh();
    const timer = setInterval(() => {
      if (running || reconnecting) void refresh();
    }, POLL_MS);
    return () => clearInterval(timer);
  });

  $effect(() => {
    void output;
    if (outputEl) outputEl.scrollTop = outputEl.scrollHeight;
  });

  async function update() {
    const ok = await confirm.ask({
      title: "拉取并更新？",
      description: "有新提交时会构建并重启服务，正在进行的对话会被中断。页面关闭不影响更新。",
      action: "更新",
    });
    if (!ok) return;
    recovered = false;
    try {
      await startUpdate();
    } catch (e) {
      error = (e as Error).message;
    }
    await refresh();
  }
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
        onclick={update}>
        {#if running || reconnecting}<Icon name="loader" class="size-3.5 animate-spin" />{/if}检查并更新
      </button>
      <span class="text-muted-foreground">
        {#if reconnecting}
          正在重启，等待服务恢复…
        {:else if recovered}
          服务已恢复。
        {:else if status.state === "checking"}
          {STAGES.checking}…
        {:else if status.state === "pulling"}
          {STAGES.pulling}（第 {status.attempt} 次）· 当前 {short(status.from)}
        {:else if status.state === "building"}
          {STAGES.building}：{short(status.from)} → {short(status.to)}
        {:else if status.state === "restarting"}
          {STAGES.restarting}：{short(status.from)} → {short(status.to)}
        {:else if status.state === "up_to_date"}
          已是最新（{short(status.commit)}），未重启。
        {/if}
      </span>
    </div>
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

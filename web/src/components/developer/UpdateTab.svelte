<script lang="ts">
  import { onMount } from "svelte";
  import { ApiError, getUpdateStatus, startUpdate } from "../../api/client";
  import { ProtocolError } from "../../api/decode";
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
  /** 重启后核对：新进程报告的版本是否为目标提交。 */
  let verdict = $state<{ target: string; running: string | null } | null>(null);

  // 目标提交按浏览器记住：随机 token 重启后需重新登录，页面会重新挂载。
  const TARGET_KEY = "micnext.developer.updateTarget";
  function loadTarget(): string | null {
    try {
      return localStorage.getItem(TARGET_KEY);
    } catch {
      return null;
    }
  }
  function saveTarget(v: string | null) {
    try {
      if (v === null) localStorage.removeItem(TARGET_KEY);
      else localStorage.setItem(TARGET_KEY, v);
    } catch {
      // NOTE: 存储不可用时仅本页有效。
    }
  }
  let target = loadTarget();
  let outputEl = $state<HTMLElement | null>(null);

  const running = $derived(
    status !== null && ["checking", "pulling", "building", "restarting"].includes(status.state),
  );
  const output = $derived(status !== null && "output" in status ? status.output : "");
  const short = (sha: string) => sha.slice(0, 7);

  /** 本页是否已从服务取到过状态；首次之后再见到重启结果，说明页面代码是旧版。 */
  let loaded = false;

  async function refresh() {
    try {
      const next = await getUpdateStatus();
      reconnecting = false;
      if (next.state === "building" || next.state === "restarting") {
        if (target !== next.to) saveTarget((target = next.to));
      } else if (next.state === "idle" && target !== null) {
        // 服务已换新版，重载取新界面，核对由重载后的页面按已存目标完成。
        if (loaded) return location.reload();
        verdict = { target, running: next.running };
        saveTarget((target = null));
      } else if (next.state !== "unavailable" && target !== null) {
        saveTarget((target = null));
      }
      status = next;
      error = null;
      loaded = true;
    } catch (e) {
      if (loaded && target !== null && e instanceof ProtocolError) {
        location.reload();
      } else if (status?.state === "restarting" && e instanceof ApiError && e.status === 0) {
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
    verdict = null;
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
        {:else if status.state === "idle" && !verdict}
          当前运行 {status.running ? short(status.running) : "未知版本（非 build.sh 构建）"}
        {/if}
      </span>
    </div>
    {#if verdict && status.state === "idle" && !reconnecting}
      {#if verdict.running === verdict.target}
        <p class="text-emerald-600 dark:text-emerald-400">已更新并重启，当前运行 {short(verdict.target)}。</p>
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

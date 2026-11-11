<script lang="ts">
  import { Button } from "$lib/components/ui/button";
  import { Input } from "$lib/components/ui/input";
  import { login } from "../api/client";

  let username = $state("");
  let password = $state("");
  let busy = $state(false);
  let error = $state("");

  $effect(() => {
    document.title = "登录 · micnext";
  });

  async function submit(e: SubmitEvent) {
    e.preventDefault();
    busy = true;
    error = "";
    try {
      await login(username.trim(), password);
      // 重新加载，各页面状态从头取一遍；地址不变即回到原来要打开的页面。
      location.reload();
    } catch (err) {
      error = (err as Error).message;
      busy = false;
    }
  }
</script>

<div class="grid h-full place-items-center p-6">
  <form class="w-full max-w-xs space-y-3" onsubmit={submit}>
    <p class="mb-5 text-center text-lg font-medium">登录 micnext</p>
    <Input bind:value={username} placeholder="用户名" autocomplete="username" autocapitalize="off" spellcheck={false} required />
    <Input bind:value={password} type="password" placeholder="密码" autocomplete="current-password" required />
    {#if error}
      <p class="text-sm text-destructive">{error}</p>
    {/if}
    <Button type="submit" class="w-full" disabled={busy}>{busy ? "登录中…" : "登录"}</Button>
  </form>
</div>

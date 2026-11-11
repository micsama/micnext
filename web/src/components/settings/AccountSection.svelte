<script lang="ts">
  import { Button } from "$lib/components/ui/button";
  import { Input } from "$lib/components/ui/input";
  import { changePassword } from "../../api/client";
  import Group from "./Group.svelte";

  let current = $state("");
  let next = $state("");
  let busy = $state(false);
  let error = $state("");
  let done = $state(false);

  async function submit(e: SubmitEvent) {
    e.preventDefault();
    busy = true;
    error = "";
    done = false;
    try {
      await changePassword(current, next);
      current = "";
      next = "";
      done = true;
    } catch (err) {
      error = (err as Error).message;
    } finally {
      busy = false;
    }
  }
</script>

<Group title="修改密码" footer="用户名 dzmfg。修改后其他设备需要重新登录，本设备保持登录。">
  <form class="space-y-3 px-4 py-3" onsubmit={submit}>
    <input type="text" autocomplete="username" value="dzmfg" hidden readonly />
    <Input bind:value={current} type="password" placeholder="当前密码" autocomplete="current-password" required />
    <Input bind:value={next} type="password" placeholder="新密码，至少 8 位" autocomplete="new-password" minlength={8} required />
    {#if error}
      <p class="text-sm text-destructive">{error}</p>
    {:else if done}
      <p class="text-sm text-muted-foreground">密码已修改</p>
    {/if}
    <Button type="submit" size="sm" disabled={busy}>{busy ? "保存中…" : "修改密码"}</Button>
  </form>
</Group>

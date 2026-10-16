<script lang="ts">
  import { Button } from "$lib/components/ui/button";
  import { Textarea } from "$lib/components/ui/textarea";
  import { ApiError, exportSettings, importSettings } from "../../api/client";
  import { confirm } from "../../state/confirm.svelte";
  import { settingsStore } from "../../state/settings.svelte";
  import Group from "./Group.svelte";

  let input = $state("");
  let exported = $state("");
  let busy = $state(false);
  let message = $state("");
  let error = $state("");

  async function exportBackup() {
    busy = true;
    error = "";
    message = "";
    exported = "";
    try {
      const text = await exportSettings();
      try {
        await navigator.clipboard.writeText(text);
        message = "设置备份已复制到剪贴板";
      } catch {
        exported = text;
        message = "请复制下方的备份文本";
      }
    } catch (e) {
      error = (e as Error).message;
    } finally {
      busy = false;
    }
  }

  async function importBackup() {
    busy = true;
    try {
      if (!(await confirm.ask({
        title: "覆盖当前设置？",
        description: "将用备份覆盖当前的服务商、模型、人设、通用设置与 Channel 账号，确定吗？",
        action: "覆盖并导入",
      }))) return;
      error = "";
      message = "";
      await importSettings(input.trim());
      input = "";
      exported = "";
      message = "设置已导入";
      await settingsStore.refresh();
    } catch (e) {
      error = (e as Error).message;
      if (e instanceof ApiError && e.status === 500 && e.message.startsWith("设置已导入；")) {
        await settingsStore.refresh();
      }
    } finally {
      busy = false;
    }
  }
</script>

<div class="space-y-6">
  <Group title="导出设置" footer="包含服务商密钥、模型、人设、对话偏好与当前微信账号，不包含聊天记录。备份已加密；换机器恢复需同时拷贝 master.key。">
    <div class="space-y-3 p-5">
      <Button variant="outline" disabled={busy} onclick={exportBackup}>导出设置</Button>
      {#if exported}
        <label class="block space-y-2 text-sm">
          <span>备份文本（请手动复制）</span>
          <Textarea readonly value={exported} class="min-h-32 break-all font-mono text-xs" onclick={(e) => e.currentTarget.select()} />
        </label>
      {/if}
    </div>
  </Group>
  <Group title="导入设置" footer="只能导入由同一 master.key 加密的备份。导入会覆盖当前设置，已有聊天记录保留。">
    <div class="space-y-3 p-5">
      <label class="block space-y-2 text-sm">
        <span>粘贴备份文本</span>
        <Textarea bind:value={input} disabled={busy} placeholder="micnext-settings:…" class="min-h-32 break-all font-mono text-xs" />
      </label>
      <Button disabled={busy || !input.trim()} onclick={importBackup}>{busy ? "处理中…" : "导入设置"}</Button>
    </div>
  </Group>
  {#if message}<p role="status" class="text-sm text-muted-foreground">{message}</p>{/if}
  {#if error}<p role="alert" class="text-sm text-destructive">{error}</p>{/if}
</div>

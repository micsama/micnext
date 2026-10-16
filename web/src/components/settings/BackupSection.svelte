<script lang="ts">
  import CheckIcon from "@lucide/svelte/icons/check";
  import DownloadIcon from "@lucide/svelte/icons/download";
  import KeyRoundIcon from "@lucide/svelte/icons/key-round";
  import UploadIcon from "@lucide/svelte/icons/upload";
  import { Button } from "$lib/components/ui/button";
  import { Textarea } from "$lib/components/ui/textarea";
  import { ApiError, exportSettings, importSettings } from "../../api/client";
  import { confirm } from "../../state/confirm.svelte";
  import { settingsStore } from "../../state/settings.svelte";
  import Group from "./Group.svelte";

  let input = $state("");
  let exported = $state("");
  let busy = $state(false);
  let copied = $state(false);
  let exportError = $state("");
  let imported = $state(false);
  let importError = $state("");
  let copiedTimer: ReturnType<typeof setTimeout>;

  async function exportBackup() {
    busy = true;
    exportError = "";
    exported = "";
    try {
      const text = await exportSettings();
      try {
        await navigator.clipboard.writeText(text);
        copied = true;
        clearTimeout(copiedTimer);
        copiedTimer = setTimeout(() => (copied = false), 2000);
      } catch {
        exported = text;
      }
    } catch (e) {
      exportError = (e as Error).message;
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
      importError = "";
      imported = false;
      await importSettings(input.trim());
      input = "";
      exported = "";
      imported = true;
      await settingsStore.refresh();
    } catch (e) {
      importError = (e as Error).message;
      if (e instanceof ApiError && e.status === 500 && e.message.startsWith("设置已导入；")) {
        await settingsStore.refresh();
      }
    } finally {
      busy = false;
    }
  }
</script>

{#snippet head(Icon: typeof DownloadIcon, title: string, desc: string)}
  <div class="flex size-9 shrink-0 items-center justify-center rounded-lg bg-primary/10 text-primary">
    <Icon class="size-4" />
  </div>
  <div class="min-w-0 flex-1">
    <p class="text-sm font-medium">{title}</p>
    <p class="mt-0.5 text-xs text-muted-foreground">{desc}</p>
  </div>
{/snippet}

<div class="space-y-8">
  <div class="flex items-start gap-3 rounded-xl bg-muted/60 px-4 py-3 text-xs leading-relaxed text-muted-foreground">
    <KeyRoundIcon class="mt-0.5 size-4 shrink-0" />
    <p>备份经 <code class="font-mono text-foreground">master.key</code> 加密，只能在持有同一密钥的机器上导入。换机器时请一并拷贝 <code class="font-mono text-foreground">master.key</code>。</p>
  </div>

  <Group title="导出">
    <div class="flex items-center gap-3 px-4 py-4">
      {@render head(DownloadIcon, "导出设置", "服务商密钥、模型、人设、对话偏好与微信账号，不含聊天记录")}
      <Button variant="outline" size="sm" class="shrink-0" disabled={busy} onclick={exportBackup}>
        {#if copied}<CheckIcon class="size-3.5" />已复制{:else}复制备份{/if}
      </Button>
    </div>
    {#if exported}
      <div class="space-y-2 px-4 py-3">
        <p class="text-xs text-muted-foreground">无法访问剪贴板，请手动复制下方文本</p>
        <Textarea readonly value={exported} class="min-h-28 break-all font-mono text-xs" onclick={(e) => e.currentTarget.select()} />
      </div>
    {/if}
    {#if exportError}<p role="alert" class="px-4 py-3 text-xs text-destructive">{exportError}</p>{/if}
  </Group>

  <Group title="导入">
    <div class="flex items-center gap-3 px-4 py-4">
      {@render head(UploadIcon, "导入设置", "覆盖当前设置，已有聊天记录保留")}
    </div>
    <div class="space-y-3 px-4 py-3">
      <Textarea
        bind:value={input}
        disabled={busy}
        placeholder="粘贴以 micnext-settings: 开头的备份文本"
        class="min-h-28 break-all font-mono text-xs"
        oninput={() => (imported = false)}
      />
      <div class="flex items-center justify-end gap-3">
        {#if importError}
          <p role="alert" class="mr-auto text-xs text-destructive">{importError}</p>
        {:else if imported}
          <p role="status" class="mr-auto flex items-center gap-1 text-xs text-muted-foreground">
            <CheckIcon class="size-3.5 text-primary" />设置已导入
          </p>
        {/if}
        <Button size="sm" disabled={busy || !input.trim()} onclick={importBackup}>{busy ? "处理中…" : "导入设置"}</Button>
      </div>
    </div>
  </Group>
</div>

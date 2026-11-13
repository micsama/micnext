<script lang="ts">
  import { fileIconUrl } from "../lib/file-icon.svelte";
  import { formatBytes } from "../lib/format";
  import Icon from "../lib/Icon.svelte";
  import { theme } from "../state/theme.svelte";
  import ChatImage from "./ChatImage.svelte";
  import FilePreview from "./FilePreview.svelte";

  /** 消息或推送引用的文件；`path` 为 `/api` 下的地址。图片直接显示，HTML 页内预览，PDF 新标签页打开，其余交给浏览器下载。 */
  let { name, size, title = name, path }: { name: string; size: number; title?: string; path: string } = $props();

  const href = $derived(`/api${path}`);
  const ext = $derived(name.split(".").at(-1)?.toLowerCase() ?? "");
  const kind = $derived(
    ["png", "jpg", "jpeg", "gif", "webp", "svg"].includes(ext) ? "image" : ["html", "htm"].includes(ext) ? "html" : ext === "pdf" ? "pdf" : "other",
  );
  /** 图片显示失败（如文件已清理）后退回文件卡片。 */
  let imageFailed = $state(false);
  let previewing = $state(false);

  function download() {
    const a = document.createElement("a");
    a.href = href;
    a.download = name;
    a.click();
  }
</script>

{#if kind === "image" && !imageFailed}
  <ChatImage src={href} {name} onfail={() => (imageFailed = true)} ondownload={download} />
{:else}
  <div class="group/file relative flex h-16 w-44 items-center gap-2 rounded-lg bg-muted px-3 text-xs" {title}>
    <img src={fileIconUrl(name, theme.dark)} alt="" class="size-6 shrink-0" />
    <div class="min-w-0">
      <p class="truncate">{name}</p>
      <p class="text-muted-foreground tabular-nums">{imageFailed ? "无法显示" : formatBytes(size)}</p>
    </div>
    {#if kind === "html"}
      <button type="button" aria-label="打开 {name}" class="absolute inset-0 rounded-lg hover:bg-foreground/5" onclick={() => (previewing = true)}></button>
    {:else if kind === "pdf"}
      <a {href} target="_blank" rel="noopener" aria-label="打开 {name}" class="absolute inset-0 rounded-lg hover:bg-foreground/5"></a>
    {:else}
      <a {href} download={name} aria-label="下载 {name}" class="absolute inset-0 rounded-lg hover:bg-foreground/5"></a>
    {/if}
    {#if kind === "html" || kind === "pdf"}
      <a {href} download={name} aria-label="下载" title="下载" class="absolute top-1 right-1 grid size-6 place-items-center rounded-md bg-muted text-muted-foreground opacity-0 transition-opacity group-hover/file:opacity-100 hover:text-foreground focus-visible:opacity-100"><Icon name="download" class="size-3.5" /></a>
    {/if}
  </div>
{/if}

{#if kind === "html" && previewing}
  <FilePreview bind:open={previewing} {name} url={href} kind="html" ondownload={download} />
{/if}

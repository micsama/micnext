<script lang="ts">
  import { onDestroy } from "svelte";
  import { fetchFile } from "../api/client";
  import { fileIconUrl } from "../lib/file-icon.svelte";
  import { formatBytes } from "../lib/format";
  import Icon from "../lib/Icon.svelte";
  import { theme } from "../state/theme.svelte";
  import ChatImage from "./ChatImage.svelte";
  import FilePreview from "./FilePreview.svelte";

  /** 消息或推送引用的文件；`path` 为 `/api` 下的地址。图片交给浏览器按需显示，其余点开时带进度下载。 */
  let { name, size, title = name, path }: { name: string; size: number; title?: string; path: string } = $props();

  const ext = $derived(name.split(".").at(-1)?.toLowerCase() ?? "");
  const kind = $derived(
    ["png", "jpg", "jpeg", "gif", "webp", "svg"].includes(ext) ? "image" : ["html", "htm"].includes(ext) ? "html" : ext === "pdf" ? "pdf" : "other",
  );
  /** 图片显示失败（如文件已清理）后退回文件卡片，点开下载给出原因。 */
  let imageFailed = $state(false);
  let url = $state<string | null>(null);
  let error = $state<string | null>(null);
  let previewing = $state(false);
  /** 下载中的已读字节数；同一时间只有一次下载。 */
  let received = $state<number | null>(null);
  let destroyed = false;

  /** 组件已卸载时返回 null，不留下无人撤销的 Blob URL。 */
  async function blobUrl(): Promise<string | null> {
    if (url) return url;
    received = 0;
    try {
      const blob = await fetchFile(path, (n) => (received = n));
      if (destroyed) return null;
      return (url = URL.createObjectURL(blob));
    } finally {
      received = null;
    }
  }

  async function run(action: (url: string) => void) {
    if (received !== null) return;
    error = null;
    try {
      const href = await blobUrl();
      if (href) action(href);
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    }
  }

  const status = $derived.by(() => {
    if (received === null) return error ?? formatBytes(size);
    return size > 0 ? `下载中 ${Math.min(99, Math.floor((received / size) * 100))}%` : `下载中 ${formatBytes(received)}`;
  });

  function download() {
    void run((href) => {
      const a = document.createElement("a");
      a.href = href;
      a.download = name;
      a.click();
    });
  }

  function open() {
    if (kind === "image" || kind === "other") return download();
    if (kind === "html") return void run(() => (previewing = true));
    if (received !== null) return;
    // 先同步开窗，等下载完再导航，避开弹窗拦截。
    const tab = window.open("about:blank");
    void run((href) => tab && (tab.location.href = href)).then(() => (error || destroyed) && tab?.close());
  }

  onDestroy(() => {
    destroyed = true;
    if (url) URL.revokeObjectURL(url);
  });
</script>

{#if kind === "image" && !imageFailed}
  <ChatImage src="/api{path}" {name} onfail={() => (imageFailed = true)} ondownload={download} />
{:else}
  <div class="group/file relative flex h-16 w-44 items-center gap-2 rounded-lg bg-muted px-3 text-xs" {title}>
    <img src={fileIconUrl(name, theme.dark)} alt="" class="size-6 shrink-0" />
    <div class="min-w-0">
      <p class="truncate">{name}</p>
      <p class={error ? "truncate text-destructive" : "text-muted-foreground tabular-nums"}>{status}</p>
    </div>
    <button type="button" aria-label="打开 {name}" aria-busy={received !== null} class="absolute inset-0 rounded-lg hover:bg-foreground/5 aria-busy:cursor-progress" onclick={open}></button>
    {#if kind !== "other" && kind !== "image"}
      <button type="button" aria-label="下载" title="下载" class="absolute top-1 right-1 grid size-6 place-items-center rounded-md bg-muted text-muted-foreground opacity-0 transition-opacity group-hover/file:opacity-100 hover:text-foreground focus-visible:opacity-100" onclick={download}><Icon name="download" class="size-3.5" /></button>
    {/if}
  </div>
{/if}

{#if url && kind === "html"}
  <FilePreview bind:open={previewing} {name} {url} {kind} ondownload={download} />
{/if}

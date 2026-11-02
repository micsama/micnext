<script lang="ts">
  import { onDestroy, untrack } from "svelte";
  import { fileIconUrl } from "../lib/file-icon.svelte";
  import { formatBytes } from "../lib/format";
  import Icon from "../lib/Icon.svelte";
  import { theme } from "../state/theme.svelte";
  import FilePreview from "./FilePreview.svelte";

  /** `load` 缺省为只展示（如待发送的附件）；有则可预览与下载，`onProgress` 收到已读字节数。 */
  let { name, size, title = name, load }: { name: string; size: number; title?: string; load?: (onProgress: (received: number) => void) => Promise<Blob> } = $props();

  const ext = $derived(name.split(".").at(-1)?.toLowerCase() ?? "");
  const kind = $derived(
    ["png", "jpg", "jpeg", "gif", "webp", "svg"].includes(ext) ? "image" : ["html", "htm"].includes(ext) ? "html" : ext === "pdf" ? "pdf" : "other",
  );
  let url = $state<string | null>(null);
  let error = $state<string | null>(null);
  let previewing = $state(false);
  /** 下载中的已读字节数；同一时间只有一次下载。 */
  let received = $state<number | null>(null);

  async function blobUrl(): Promise<string> {
    if (url) return url;
    received = 0;
    try {
      url = URL.createObjectURL(await load!((n) => (received = n)));
    } finally {
      received = null;
    }
    return url;
  }

  async function run(action: (url: string) => void) {
    if (received !== null) return;
    error = null;
    try {
      action(await blobUrl());
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
    if (kind === "image" || kind === "html") return void run(() => (previewing = true));
    if (kind === "other") return download();
    if (received !== null) return;
    // 先同步开窗，等下载完再导航，避开弹窗拦截。
    const tab = window.open("about:blank");
    void run((href) => tab && (tab.location.href = href)).then(() => error && tab?.close());
  }

  $effect(() => {
    if (load && kind === "image") untrack(() => void run(() => {}));
  });
  onDestroy(() => url && URL.revokeObjectURL(url));
</script>

{#if load && kind === "image" && url}
  <button type="button" class="block overflow-hidden rounded-lg" {title} onclick={open}>
    <img src={url} alt={name} class="block max-h-64 max-w-full" />
  </button>
{:else}
  <div class="group/file relative flex h-16 w-44 items-center gap-2 rounded-lg bg-muted px-3 text-xs" {title}>
    <img src={fileIconUrl(name, theme.dark)} alt="" class="size-6 shrink-0" />
    <div class="min-w-0">
      <p class="truncate">{name}</p>
      <p class={error ? "truncate text-destructive" : "text-muted-foreground tabular-nums"}>{status}</p>
    </div>
    {#if load}
      <button type="button" aria-label="打开 {name}" aria-busy={received !== null} class="absolute inset-0 rounded-lg hover:bg-foreground/5 aria-busy:cursor-progress" onclick={open}></button>
      {#if kind !== "other"}
        <button type="button" aria-label="下载" title="下载" class="absolute top-1 right-1 grid size-6 place-items-center rounded-md bg-muted text-muted-foreground opacity-0 transition-opacity group-hover/file:opacity-100 hover:text-foreground focus-visible:opacity-100" onclick={download}><Icon name="download" class="size-3.5" /></button>
      {/if}
    {/if}
  </div>
{/if}

{#if url && (kind === "image" || kind === "html")}
  <FilePreview bind:open={previewing} {name} {url} {kind} ondownload={download} />
{/if}

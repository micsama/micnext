<script lang="ts">
  import { onDestroy, untrack } from "svelte";
  import * as Dialog from "$lib/components/ui/dialog";
  import { fileIconUrl } from "../lib/file-icon.svelte";
  import { formatBytes } from "../lib/format";
  import Icon from "../lib/Icon.svelte";
  import { theme } from "../state/theme.svelte";

  /** `load` 缺省为只展示（如待发送的附件）；有则可预览与下载。 */
  let { name, size, title = name, load }: { name: string; size: number; title?: string; load?: () => Promise<Blob> } = $props();

  const ext = $derived(name.split(".").at(-1)?.toLowerCase() ?? "");
  const kind = $derived(
    ["png", "jpg", "jpeg", "gif", "webp", "svg"].includes(ext) ? "image" : ["html", "htm"].includes(ext) ? "html" : ext === "pdf" ? "pdf" : "other",
  );
  let url = $state<string | null>(null);
  let error = $state<string | null>(null);
  let previewing = $state(false);

  async function blobUrl(): Promise<string> {
    if (url) return url;
    url = URL.createObjectURL(await load!());
    return url;
  }

  async function run(action: (url: string) => void) {
    error = null;
    try {
      action(await blobUrl());
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    }
  }

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
      <p class={error ? "truncate text-destructive" : "text-muted-foreground"}>{error ?? formatBytes(size)}</p>
    </div>
    {#if load}
      <button type="button" aria-label="打开 {name}" class="absolute inset-0 rounded-lg hover:bg-foreground/5" onclick={open}></button>
      {#if kind !== "other"}
        <button type="button" aria-label="下载" title="下载" class="absolute top-1 right-1 grid size-6 place-items-center rounded-md bg-muted text-muted-foreground opacity-0 transition-opacity group-hover/file:opacity-100 hover:text-foreground focus-visible:opacity-100" onclick={download}><Icon name="download" class="size-3.5" /></button>
      {/if}
    {/if}
  </div>
{/if}

{#if url && (kind === "image" || kind === "html")}
  <Dialog.Root bind:open={previewing}>
    <Dialog.Content showCloseButton={false} class="flex h-[90vh] w-[min(96vw,80rem)] max-w-none flex-col gap-0 overflow-hidden p-0 sm:max-w-none">
      <div class="flex items-center gap-2 border-b px-4 py-2">
        <Dialog.Title class="min-w-0 flex-1 truncate text-sm font-medium">{name}</Dialog.Title>
        <button type="button" aria-label="下载" title="下载" class="grid size-7 place-items-center rounded-md text-muted-foreground hover:bg-accent hover:text-foreground" onclick={download}><Icon name="download" class="size-4" /></button>
        <Dialog.Close aria-label="关闭" title="关闭" class="grid size-7 place-items-center rounded-md text-muted-foreground hover:bg-accent hover:text-foreground"><Icon name="x" class="size-4" /></Dialog.Close>
      </div>
      {#if kind === "html"}
        <!-- 不给 allow-same-origin：页面脚本可运行，但拿不到本站登录态。 -->
        <iframe src={url} title={name} sandbox="allow-scripts" class="min-h-0 flex-1 bg-white"></iframe>
      {:else}
        <div class="grid min-h-0 flex-1 place-items-center overflow-auto bg-muted/40 p-4"><img src={url} alt={name} class="max-h-full max-w-full" /></div>
      {/if}
    </Dialog.Content>
  </Dialog.Root>
{/if}

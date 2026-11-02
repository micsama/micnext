<script lang="ts">
  import * as Dialog from "$lib/components/ui/dialog";
  import Icon from "../lib/Icon.svelte";

  /** 图片或 HTML 的全屏预览；有 `ondownload` 时标题栏带下载。 */
  let { open = $bindable(false), name, url, kind, ondownload }: { open?: boolean; name: string; url: string; kind: "image" | "html"; ondownload?: () => void } = $props();
</script>

<Dialog.Root bind:open>
  <Dialog.Content showCloseButton={false} class="flex h-[90vh] w-[min(96vw,80rem)] max-w-none flex-col gap-0 overflow-hidden p-0 sm:max-w-none">
    <div class="flex items-center gap-2 border-b px-4 py-2">
      <Dialog.Title class="min-w-0 flex-1 truncate text-sm font-medium">{name}</Dialog.Title>
      {#if ondownload}
        <button type="button" aria-label="下载" title="下载" class="grid size-7 place-items-center rounded-md text-muted-foreground hover:bg-accent hover:text-foreground" onclick={ondownload}><Icon name="download" class="size-4" /></button>
      {/if}
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

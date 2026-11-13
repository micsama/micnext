<script lang="ts">
  import FilePreview from "./FilePreview.svelte";

  /** 消息里的图片：显示 `src`，点开放大才加载 `full`；懒加载与缓存交给浏览器。`onfail` 缺省时失败显示占位。 */
  let { src, full = src, name = "图片", onfail, ondownload }: { src: string; full?: string; name?: string; onfail?: () => void; ondownload?: () => void } = $props();
  let loaded = $state(false);
  let failed = $state(false);
  let previewing = $state(false);

  $effect.pre(() => {
    void src;
    loaded = false;
    failed = false;
  });

  function fail() {
    failed = true;
    onfail?.();
  }
</script>

{#if failed}
  {#if !onfail}<span class="grid h-16 w-32 place-items-center rounded-lg bg-muted text-xs text-muted-foreground">图片加载失败</span>{/if}
{:else}
  <button type="button" aria-label="放大{name}" title={name} class={["block cursor-zoom-in overflow-hidden rounded-lg", !loaded && "h-24 w-32 animate-pulse bg-muted"]} onclick={() => (previewing = true)}>
    <img {src} alt={name} loading="lazy" decoding="async" class={["block max-h-64 max-w-full", !loaded && "opacity-0"]} onload={() => (loaded = true)} onerror={fail} />
  </button>
  {#if previewing}
    <FilePreview bind:open={previewing} {name} url={full} kind="image" {ondownload} />
  {/if}
{/if}

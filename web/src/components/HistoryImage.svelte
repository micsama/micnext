<script lang="ts">
  import FilePreview from "./FilePreview.svelte";

  /** 会话内图片：列表里显示缩略图，点开放大才加载原图；懒加载与缓存交给浏览器。 */
  let { sessionId, imageId }: { sessionId: number; imageId: number } = $props();
  const url = $derived(`/api/sessions/${sessionId}/images/${imageId}`);
  let loaded = $state(false);
  let failed = $state(false);
  let previewing = $state(false);

  $effect.pre(() => {
    void url;
    loaded = false;
    failed = false;
  });
</script>

{#if failed}
  <span class="grid h-16 w-32 place-items-center rounded-lg bg-muted text-xs text-muted-foreground">图片加载失败</span>
{:else}
  <button type="button" aria-label="放大图片" title="放大图片" class={["block cursor-zoom-in overflow-hidden rounded-lg", !loaded && "h-24 w-32 animate-pulse bg-muted"]} onclick={() => (previewing = true)}>
    <img src="{url}/thumb" alt="图片" loading="lazy" decoding="async" class={["block max-h-64 max-w-full", !loaded && "opacity-0"]} onload={() => (loaded = true)} onerror={() => (failed = true)} />
  </button>
  {#if previewing}
    <FilePreview bind:open={previewing} name="图片" {url} kind="image" />
  {/if}
{/if}

<script lang="ts">
  import { fetchImage } from "../api/client";

  let { sessionId, imageId }: { sessionId: number; imageId: number } = $props();

  let url = $state<string | null>(null);
  let failed = $state(false);

  $effect(() => {
    const id = imageId;
    const session = sessionId;
    let objectUrl: string | null = null;
    let stale = false;
    failed = false;
    fetchImage(session, id).then(
      (blob) => {
        if (stale) return;
        objectUrl = URL.createObjectURL(blob);
        url = objectUrl;
      },
      () => {
        if (!stale) failed = true;
      },
    );
    return () => {
      stale = true;
      url = null;
      if (objectUrl) URL.revokeObjectURL(objectUrl);
    };
  });
</script>

{#if url}
  <img src={url} alt="图片" class="my-1 block max-h-64 max-w-full rounded-lg" />
{:else if failed}
  <span class="my-0.5 inline-block rounded bg-background px-1.5 py-0.5 text-xs text-muted-foreground">图片加载失败</span>
{:else}
  <span class="my-1 block h-24 w-32 animate-pulse rounded-lg bg-background"></span>
{/if}

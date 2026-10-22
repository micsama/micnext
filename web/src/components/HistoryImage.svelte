<script module lang="ts">
  import { fetchImage } from "../api/client";
  import { auth } from "../state/auth.svelte";

  const IMAGE_IDLE_MS = 5 * 60_000;
  type ImageRequest = {
    users: number;
    url: string | null;
    ready: Promise<string>;
    timer: ReturnType<typeof setTimeout> | null;
  };
  const requests = new Map<string, ImageRequest>();

  function acquire(key: string): ImageRequest {
    const existing = requests.get(key);
    if (existing) {
      existing.users++;
      if (existing.timer !== null) clearTimeout(existing.timer);
      existing.timer = null;
      return existing;
    }
    const [, session, image] = JSON.parse(key) as [string | null, number, number];
    const entry: ImageRequest = {
      users: 1,
      url: null,
      timer: null,
      ready: fetchImage(session, image).then((blob) => {
        const url = URL.createObjectURL(blob);
        entry.url = url;
        if (requests.get(key) !== entry) URL.revokeObjectURL(url);
        return url;
      }).catch((error) => {
        if (requests.get(key) === entry) requests.delete(key);
        throw error;
      }),
    };
    requests.set(key, entry);
    return entry;
  }

  function release(key: string, entry: ImageRequest): void {
    entry.users--;
    if (entry.users !== 0) return;
    entry.timer = setTimeout(() => {
      if (requests.get(key) === entry) requests.delete(key);
      if (entry.url) URL.revokeObjectURL(entry.url);
      entry.timer = null;
    }, IMAGE_IDLE_MS);
  }
</script>

<script lang="ts">
  import { untrack } from "svelte";

  let { sessionId, imageId }: { sessionId: number; imageId: number } = $props();
  const key = $derived(JSON.stringify([auth.token, sessionId, imageId]));
  let container: HTMLDivElement;
  let url = $state<string | null>(null);
  let failed = $state(false);

  $effect(() => {
    const identity = key;
    const element = container;
    let entry: ImageRequest | null = null;
    let stale = false;
    url = null;
    failed = false;
    const observer = new IntersectionObserver((entries) => {
      if (stale || entry || !entries.some((entry) => entry.isIntersecting)) return;
      observer.disconnect();
      entry = untrack(() => acquire(identity));
      entry.ready.then(
        (value) => { if (!stale) url = value; },
        () => { if (!stale) failed = true; },
      );
    }, { rootMargin: "200px" });
    observer.observe(element);
    return () => {
      stale = true;
      observer.disconnect();
      if (entry) release(identity, entry);
    };
  });
</script>

<div bind:this={container}>
  {#if url}
    <img src={url} alt="图片" class="block max-h-64 max-w-full rounded-lg" />
  {:else if failed}
    <span class="grid h-16 w-32 place-items-center rounded-lg bg-muted text-xs text-muted-foreground">图片加载失败</span>
  {:else}
    <span class="block h-24 w-32 animate-pulse rounded-lg bg-muted"></span>
  {/if}
</div>

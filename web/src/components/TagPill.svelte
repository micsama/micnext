<script lang="ts" module>
  // 颜色不入库：按 id 轮取一组淡色，同一标签处处同色。
  const TONES = [
    "bg-sky-500/12 text-sky-700 dark:text-sky-300",
    "bg-emerald-500/12 text-emerald-700 dark:text-emerald-300",
    "bg-amber-500/15 text-amber-700 dark:text-amber-300",
    "bg-rose-500/12 text-rose-700 dark:text-rose-300",
    "bg-violet-500/12 text-violet-700 dark:text-violet-300",
    "bg-teal-500/12 text-teal-700 dark:text-teal-300",
  ];
  export const tone = (id: number) => TONES[id % TONES.length];
</script>

<script lang="ts">
  import type { Snippet } from "svelte";

  let { id, name, active = false, small = false, onclick, children }: { id: number; name: string; active?: boolean; small?: boolean; onclick?: () => void; children?: Snippet } = $props();
  const look = $derived(["inline-flex shrink-0 items-center gap-0.5 rounded-full whitespace-nowrap", small ? "px-1 text-[10px] leading-[13px]" : "px-1.5 text-[11px] leading-[18px]", tone(id), active && "ring-1 ring-current"]);
</script>

{#if onclick}
  <button type="button" aria-pressed={active} {onclick} class={[look, "hover:brightness-95 dark:hover:brightness-125"]}>{name}{@render children?.()}</button>
{:else}
  <span class={look}>{name}{@render children?.()}</span>
{/if}

<script lang="ts">
  import BrainIcon from "@lucide/svelte/icons/brain";
  import DramaIcon from "@lucide/svelte/icons/drama";
  import SparklesIcon from "@lucide/svelte/icons/sparkles";
  import type { Component } from "svelte";
  import { cn } from "$lib/utils";
  import type { Persona } from "../api/types";

  /** `tile` 带浅色底块，用于列表与标题；否则只是一枚着色图标。 */
  let { persona, tile = false, class: cls = "" }: { persona?: Persona; tile?: boolean; class?: string } = $props();

  // 内置人设按固定 id 配图；大肥鱼取 DeepSeek 的鲸鱼形象与品牌蓝。
  const looks: Record<number, { icon: Component | "whale"; color: string }> = {
    1: { icon: SparklesIcon, color: "#8b5cf6" },
    2: { icon: "whale", color: "#4d6bfe" },
    3: { icon: BrainIcon, color: "#ec4899" },
  };
  const look = $derived(
    (persona?.builtin && looks[persona.id]) || { icon: DramaIcon, color: "var(--muted-foreground)" },
  );
</script>

{#snippet glyph(size: string)}
  {#if look.icon === "whale"}
    <svg class={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
      <path d="M2.5 13c0 4 3.5 7 8.5 7 4.5 0 7.5-2.5 8-6.5" />
      <path d="M2.5 13c1.5-2 4-3 7-3h3c2.5 0 4.5 1.2 6.5 3.5" />
      <path d="M19 13.5 20 10" />
      <path d="M20 10c-.3-1.5-1.2-2.5-2.6-3" />
      <path d="M20 10c.4-1.5 1.2-2.5 2.6-3" />
      <path d="M8.5 7V4.5" />
      <path d="M8.5 4.5C8 3.6 7.1 3 6 3" />
      <path d="M8.5 4.5C9 3.6 9.9 3 11 3" />
      <circle cx="6.5" cy="14.5" r=".7" fill="currentColor" />
    </svg>
  {:else}
    <look.icon class={size} aria-hidden="true" />
  {/if}
{/snippet}

{#if tile}
  <span
    class={cn("flex size-9 shrink-0 items-center justify-center rounded-lg", cls)}
    style="color: {look.color}; background: color-mix(in oklab, {look.color} 14%, transparent)"
  >
    {@render glyph("size-[1.15rem]")}
  </span>
{:else}
  <span class={cn("inline-flex shrink-0", cls)} style="color: {look.color}">{@render glyph("size-4")}</span>
{/if}

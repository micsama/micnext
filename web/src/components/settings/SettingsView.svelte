<script lang="ts">
  import BoxesIcon from "@lucide/svelte/icons/boxes";
  import SlidersIcon from "@lucide/svelte/icons/sliders-horizontal";
  import SparklesIcon from "@lucide/svelte/icons/sparkles";
  import type { Component } from "svelte";
  import { cn } from "$lib/utils";
  import { settingsStore } from "../../state/settings.svelte";
  import Header from "../Header.svelte";
  import EndpointsSection from "./EndpointsSection.svelte";
  import PersonasSection from "./PersonasSection.svelte";
  import PrefsSection from "./PrefsSection.svelte";

  let { onmenu }: { onmenu: () => void } = $props();

  $effect(() => {
    document.title = "设置 · micnext";
  });

  type Tab = "prefs" | "personas" | "models";
  const tabs: { id: Tab; label: string; icon: Component }[] = [
    { id: "prefs", label: "对话偏好", icon: SlidersIcon },
    { id: "personas", label: "人设", icon: SparklesIcon },
    { id: "models", label: "服务商与模型", icon: BoxesIcon },
  ];
  let tab = $state<Tab>("prefs");
  const current = $derived(tabs.find((t) => t.id === tab)!);

  // 弹窗遮挡站内导航，只需拦截刷新与关闭页面。
  let personasDirty = $state(false);
  let endpointsDirty = $state(false);
  $effect(() => {
    if (!personasDirty && !endpointsDirty) return;
    const onbeforeunload = (e: BeforeUnloadEvent) => e.preventDefault();
    addEventListener("beforeunload", onbeforeunload);
    return () => removeEventListener("beforeunload", onbeforeunload);
  });
</script>

<Header title="设置" {onmenu} />
<div class="flex min-h-0 flex-1 flex-col md:flex-row">
  <nav
    class="flex shrink-0 gap-1 border-b p-2 max-md:overflow-x-auto md:w-52 md:flex-col md:border-r md:border-b-0 md:p-3"
  >
    {#each tabs as t (t.id)}
      <button
        type="button"
        class={cn(
          "flex shrink-0 items-center gap-2.5 rounded-lg px-3 py-1.5 text-sm transition-colors",
          t.id === tab ? "bg-accent font-medium text-foreground" : "text-muted-foreground hover:text-foreground",
        )}
        aria-current={t.id === tab ? "page" : undefined}
        onclick={() => (tab = t.id)}
      >
        <t.icon class="size-4" />
        {t.label}
      </button>
    {/each}
  </nav>

  <div class="min-h-0 flex-1 overflow-y-auto">
    <div class="mx-auto max-w-2xl px-4 py-6 md:px-8 md:py-10">
      <h2 class="mb-6 text-2xl font-semibold tracking-tight">{current.label}</h2>
      {#if settingsStore.error}
        <p class="mb-4 text-sm text-destructive">{settingsStore.error}</p>
      {/if}
      {#if !settingsStore.settings}
        <p class="text-sm text-muted-foreground">加载中…</p>
      {:else if tab === "prefs"}
        <PrefsSection settings={settingsStore.settings} />
      {:else if tab === "personas"}
        <PersonasSection ondirty={(d) => (personasDirty = d)} />
      {:else}
        <EndpointsSection ondirty={(d) => (endpointsDirty = d)} />
      {/if}
    </div>
  </div>
</div>

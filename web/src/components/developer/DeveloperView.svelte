<script lang="ts">
  import { cn } from "$lib/utils";
  import { LogFeed } from "../../state/developer-logs.svelte";
  import Header from "../Header.svelte";
  import LogsTab from "./LogsTab.svelte";
  import SqlTab from "./SqlTab.svelte";

  let { onmenu }: { onmenu: () => void } = $props();

  $effect(() => {
    document.title = "开发者诊断 · micnext";
  });

  // 切到 SQL 页签不断开日志流，回来时仍是连续的。
  const feed = new LogFeed();
  $effect(() => () => feed.close());

  type Tab = "logs" | "sql";
  const tabs: { id: Tab; label: string }[] = [
    { id: "logs", label: "日志" },
    { id: "sql", label: "SQL" },
  ];
  let tab = $state<Tab>("logs");
</script>

<Header title="开发者诊断" {onmenu} />
<nav class="flex shrink-0 gap-1 border-b px-3 py-2">
  {#each tabs as t (t.id)}
    <button
      type="button"
      class={cn(
        "rounded-lg px-3 py-1 text-sm transition-colors",
        t.id === tab ? "bg-accent font-medium text-foreground" : "text-muted-foreground hover:text-foreground",
      )}
      aria-current={t.id === tab ? "page" : undefined}
      onclick={() => (tab = t.id)}>{t.label}</button>
  {/each}
</nav>
{#if tab === "logs"}
  <LogsTab {feed} />
{:else}
  <SqlTab />
{/if}

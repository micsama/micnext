<script lang="ts">
  import Icon from "../../lib/Icon.svelte";
  import TraceTab from "./TraceTab.svelte";
  import DashboardTab from "./DashboardTab.svelte";
  let { target, onmenu }: { target: number | null; onmenu: () => void } = $props();
  let tab = $state<"trace" | "dashboard">("trace");
  $effect(() => { document.title = "Trace · micnext"; });
  // 侧栏选会话即回到 Trace 页签。
  let seen: number | null | undefined;
  $effect(() => { if (seen !== undefined && target !== seen) tab = "trace"; seen = target; });
</script>
<header class="flex h-12 shrink-0 items-center gap-3 border-b bg-background px-3">
  <button type="button" class="rounded p-1.5 hover:bg-accent md:hidden" aria-label="打开会话列表" onclick={onmenu}><Icon name="menu" /></button>
  <nav class="flex h-full items-center gap-4 px-1 text-sm">
    <button type="button" class="h-full border-b-2 px-1" class:border-primary={tab === "trace"} class:border-transparent={tab !== "trace"} class:font-medium={tab === "trace"} onclick={() => tab = "trace"}>Trace</button>
    <button type="button" class="h-full border-b-2 px-1" class:border-primary={tab === "dashboard"} class:border-transparent={tab !== "dashboard"} class:font-medium={tab === "dashboard"} onclick={() => tab = "dashboard"}>看板</button>
  </nav>
</header>
<div class="flex min-h-0 flex-1 flex-col" class:hidden={tab !== "trace"}><TraceTab active={tab === "trace"} {target} /></div>
<div class="flex min-h-0 flex-1 flex-col" class:hidden={tab !== "dashboard"}><DashboardTab active={tab === "dashboard"} /></div>

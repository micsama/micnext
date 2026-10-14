<script lang="ts">
  import { router } from "../../state/route.svelte";
  import Icon from "../../lib/Icon.svelte";
  import TraceTab from "./TraceTab.svelte";
  import DashboardTab from "./DashboardTab.svelte";
  let { initialSession }: { initialSession: number | null } = $props();
  let tab = $state<"trace" | "dashboard">("trace");
  $effect(() => { document.title = "运行追踪 · micnext"; });
</script>
<header class="flex h-12 shrink-0 items-center gap-3 border-b bg-background px-4">
  <button type="button" class="inline-flex items-center gap-1.5 text-xs text-muted-foreground hover:text-foreground" onclick={() => router.go(initialSession === null ? "/" : `/s/${initialSession}`)}><span class="rotate-180"><Icon name="chevron" class="size-3.5" /></span>返回会话</button>
  <span class="h-4 border-l"></span><b class="text-sm">micnext</b><span class="text-xs text-muted-foreground">/</span>
  <nav class="flex h-full items-center gap-4 text-sm">
    <button type="button" class="h-full border-b-2 px-1" class:border-primary={tab === "trace"} class:border-transparent={tab !== "trace"} onclick={() => tab = "trace"}>运行追踪</button>
    <button type="button" class="h-full border-b-2 px-1" class:border-primary={tab === "dashboard"} class:border-transparent={tab !== "dashboard"} onclick={() => tab = "dashboard"}>看板</button>
  </nav>
  <span class="ml-auto text-xs text-muted-foreground max-sm:hidden">会话 · 执行 · 调用</span>
</header>
<div class="flex min-h-0 flex-1 flex-col" class:hidden={tab !== "trace"}><TraceTab active={tab === "trace"} {initialSession} /></div>
<div class="flex min-h-0 flex-1 flex-col" class:hidden={tab !== "dashboard"}><DashboardTab active={tab === "dashboard"} /></div>

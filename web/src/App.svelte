<script lang="ts">
  import SchedulesView from "./components/SchedulesView.svelte";
  import { globalStream } from "./state/global-stream.svelte";
  import { frontendBuild } from "./state/frontend-build.svelte";
  import Icon from "./lib/Icon.svelte";
  import TraceView from "./components/developer/TraceView.svelte";
  import ChatView from "./components/ChatView.svelte";
  import NewChat from "./components/NewChat.svelte";
  import ConfirmDialog from "./components/ConfirmDialog.svelte";
  import SearchPalette from "./components/SearchPalette.svelte";
  import { searchPalette } from "./state/search.svelte";
  import { untrack } from "svelte";
  import DeveloperView from "./components/developer/DeveloperView.svelte";
  import SettingsView from "./components/settings/SettingsView.svelte";
  import Sidebar from "./components/Sidebar.svelte";
  import { auth } from "./state/auth.svelte";
  import LoginView from "./components/LoginView.svelte";
  import { router } from "./state/route.svelte";
  import { SessionView } from "./state/session.svelte";
  import { settingsStore } from "./state/settings.svelte";

  let view = $state<SessionView | null>(null);

  // 路由决定当前打开的会话；切换时中止旧流。只换定位消息时沿用同一个会话视图。
  // 视图只随会话 id 和登录状态重建：构造时同步执行的代码读到的其它状态不能成为依赖。
  const sessionId = $derived(router.current.kind === "session" ? router.current.id : null);
  $effect(() => {
    const id = sessionId;
    if (!auth.loggedIn || id === null) return;
    const v = untrack(() => new SessionView(id));
    view = v;
    return () => {
      v.close();
      view = null;
    };
  });

  // `?m=` 定位：就绪后取到那条消息，再从地址里去掉，重连时不会重复跳转。
  $effect(() => {
    const route = router.current;
    if (!view?.ready || route.kind !== "session" || route.message === null || route.id !== view.id) return;
    const target = route.message;
    untrack(() => {
      router.replace(`/s/${route.id}`);
      void view!.reveal(target);
    });
  });

  $effect(() => {
    if (!auth.loggedIn) return;
    const onkey = (e: KeyboardEvent) => {
      if (e.key.toLowerCase() === "k" && (e.metaKey || e.ctrlKey) && !e.altKey && !e.shiftKey) {
        e.preventDefault();
        searchPalette.toggle();
      }
    };
    addEventListener("keydown", onkey);
    return () => removeEventListener("keydown", onkey);
  });

  $effect(() => {
    if (!auth.loggedIn) return;
    return globalStream.start();
  });

  $effect(() => {
    if (!auth.loggedIn) return;
    void settingsStore.refresh();
    const onfocus = () => void settingsStore.refresh();
    addEventListener("focus", onfocus);
    return () => removeEventListener("focus", onfocus);
  });
</script>

{#if !auth.loggedIn}
  <LoginView />
{:else}
  <div class="flex h-full flex-col">
    {#if frontendBuild.stale}
      <div role="status" class="flex shrink-0 items-center justify-center gap-3 border-b border-border bg-amber-500/10 px-4 py-1.5 text-xs text-amber-700 dark:text-amber-300">
        <span>界面已更新，刷新后生效</span>
        <button type="button" class="inline-flex items-center gap-1 rounded-md px-2 py-0.5 font-medium hover:bg-amber-500/15" onclick={() => location.reload()}><Icon name="refresh" class="size-3" />刷新</button>
      </div>
    {/if}
    <div class="flex min-h-0 flex-1">
      <Sidebar activeSession={view?.info ?? null} executingRun={view?.executingRun ?? null} />
      <main class="flex min-w-0 flex-1 flex-col">
        {#if router.current.kind === "trace"}
          <TraceView target={router.current.id} runTarget={router.current.run} />
        {:else if router.current.kind === "new"}
          <NewChat />
        {:else if router.current.kind === "schedules"}
          <SchedulesView />
        {:else if router.current.kind === "settings"}
          <SettingsView />
        {:else if router.current.kind === "developer"}
          <DeveloperView />
        {:else if view}
          {#key view}
            <ChatView {view} />
          {/key}
        {/if}
      </main>
    </div>
  </div>
  <ConfirmDialog />
  <SearchPalette />
{/if}

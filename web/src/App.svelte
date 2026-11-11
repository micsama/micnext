<script lang="ts">
  import SchedulesView from "./components/SchedulesView.svelte";
  import { globalStream } from "./state/global-stream.svelte";
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
  const sessionId = $derived(router.current.kind === "session" ? router.current.id : null);
  $effect(() => {
    const id = sessionId;
    if (!auth.loggedIn || id === null) return;
    const v = new SessionView(id);
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
  <div class="flex h-full">
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
  <ConfirmDialog />
  <SearchPalette />
{/if}

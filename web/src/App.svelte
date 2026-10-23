<script lang="ts">
  import SchedulesView from "./components/SchedulesView.svelte";
  import { globalStream } from "./state/global-stream.svelte";
  import TraceView from "./components/developer/TraceView.svelte";
  import ChatView from "./components/ChatView.svelte";
  import NewChat from "./components/NewChat.svelte";
  import ConfirmDialog from "./components/ConfirmDialog.svelte";
  import DeveloperView from "./components/developer/DeveloperView.svelte";
  import SettingsView from "./components/settings/SettingsView.svelte";
  import Sidebar from "./components/Sidebar.svelte";
  import { auth } from "./state/auth.svelte";
  import { router } from "./state/route.svelte";
  import { SessionView } from "./state/session.svelte";
  import { pushCenter } from "./state/pushes.svelte";
  import { settingsStore } from "./state/settings.svelte";

  let view = $state<SessionView | null>(null);

  // 路由决定当前打开的会话；切换时中止旧流。
  $effect(() => {
    const route = router.current;
    if (!auth.token || route.kind !== "session") return;
    const v = new SessionView(route.id);
    v.reply = pushCenter.takeReply(route.id);
    view = v;
    return () => {
      v.close();
      view = null;
    };
  });

  $effect(() => {
    if (!auth.token) return;
    return globalStream.start();
  });

  $effect(() => {
    if (!auth.token) return;
    void settingsStore.refresh();
    const onfocus = () => void settingsStore.refresh();
    addEventListener("focus", onfocus);
    return () => removeEventListener("focus", onfocus);
  });
</script>

{#if !auth.token}
  <div class="grid h-full place-items-center p-6">
    <div class="max-w-sm space-y-2 text-center">
      <p class="text-lg font-medium">需要重新打开</p>
      <p class="text-sm text-muted-foreground">
        访问凭据缺失或已失效（服务重启后会变）。请用终端里打印的地址（带 <code>#token=</code>）重新打开本页。
      </p>
    </div>
  </div>
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
{/if}

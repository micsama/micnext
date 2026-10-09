<script lang="ts">
  import ChatView from "./components/ChatView.svelte";
  import NewChat from "./components/NewChat.svelte";
  import ConfirmDialog from "./components/ConfirmDialog.svelte";
  import SettingsView from "./components/settings/SettingsView.svelte";
  import Sidebar from "./components/Sidebar.svelte";
  import { auth } from "./state/auth.svelte";
  import { router } from "./state/route.svelte";
  import { SessionView } from "./state/session.svelte";
  import { otherSessions, webSessions } from "./state/sessions.svelte";
  import { settingsStore } from "./state/settings.svelte";

  let drawer = $state(false);
  let view = $state<SessionView | null>(null);

  // 路由决定当前打开的会话；切换时中止旧流。
  $effect(() => {
    const route = router.current;
    if (!auth.token || route.kind !== "session") return;
    const v = new SessionView(route.id);
    view = v;
    return () => {
      v.close();
      view = null;
    };
  });

  $effect(() => {
    if (!auth.token) return;
    void webSessions.refresh();
    void settingsStore.refresh();
    const onfocus = () => {
      void webSessions.refresh();
      void settingsStore.refresh();
      for (const list of otherSessions) if (list.loaded) void list.refresh();
    };
    addEventListener("focus", onfocus);
    return () => removeEventListener("focus", onfocus);
  });

  const openDrawer = () => (drawer = true);
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
    <Sidebar open={drawer} onclose={() => (drawer = false)} />
    <main class="flex min-w-0 flex-1 flex-col">
      {#if router.current.kind === "new"}
        <NewChat onmenu={openDrawer} />
      {:else if router.current.kind === "settings"}
        <SettingsView onmenu={openDrawer} />
      {:else if view}
        {#key view}
          <ChatView {view} onmenu={openDrawer} />
        {/key}
      {/if}
    </main>
  </div>
  <ConfirmDialog />
{/if}

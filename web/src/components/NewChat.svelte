<script lang="ts">
  import { createSession } from "../api/client";
  import { router } from "../state/route.svelte";
  import { webSessions } from "../state/sessions.svelte";
  import Composer from "./Composer.svelte";
  import Header from "./Header.svelte";

  let { onmenu }: { onmenu: () => void } = $props();

  $effect(() => {
    document.title = "新会话 · micnext";
  });

  async function send(text: string) {
    const created = await createSession(text);
    void webSessions.refresh();
    router.go(`/s/${created.session_id}`);
  }
</script>

<Header title="新会话" {onmenu} />
<div class="grid flex-1 place-items-center px-4">
  <p class="text-2xl font-medium text-muted">有什么要做的？</p>
</div>
<Composer onsend={send} placeholder="发送第一条消息开始新会话" autofocus />

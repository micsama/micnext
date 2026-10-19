<script lang="ts" module>
  import { getContext, setContext } from "svelte";
  import type { SessionView } from "../state/session.svelte";

  const KEY = Symbol("job-ref");
  /** 聊天页提供任务来源；其他页面（如 trace）不提供，任务号不显示。 */
  export const provideJobRefs = (view: () => SessionView) => setContext(KEY, view);
</script>

<script lang="ts">
  import { jobActive } from "../api/types";
  import Icon from "../lib/Icon.svelte";
  import { jobLook } from "./job-look";

  /** 消息里的 exec_id 即任务号。 */
  let { id }: { id: string } = $props();

  const host = getContext<(() => SessionView) | undefined>(KEY);
  const view = $derived(host?.());
  const job = $derived(view?.jobs.find((j) => String(j.id) === id));
</script>

{#if view && job}
  {@const look = jobLook(job)}
  <button
    type="button"
    title="在任务条中查看"
    class="inline-flex h-5 shrink-0 items-center gap-1 rounded-md align-middle bg-muted px-1.5 font-mono text-xs text-muted-foreground hover:bg-accent hover:text-primary"
    onclick={(e) => {
      e.preventDefault();
      view.revealJob(job.id);
    }}>
    <Icon name={look.icon} class="size-3 {look.tone} {jobActive(job) ? 'animate-spin' : ''}" />#{job.id}
  </button>
{/if}

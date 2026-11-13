<script lang="ts">
  import { DropdownMenu } from "bits-ui";
  import CheckIcon from "@lucide/svelte/icons/check";
  import PlusIcon from "@lucide/svelte/icons/plus";
  import { setSessionTags } from "../api/client";
  import type { SessionItem } from "../api/types";
  import { tags } from "../state/sessions.svelte";
  import TagPill from "./TagPill.svelte";

  const LIMIT = 2;

  let { item }: { item: SessionItem } = $props();
  const current = $derived(item.tag_ids.flatMap((id) => tags.byId(id) ?? []));
  const editable = $derived(item.archived_at === null);
  let busy = $state(false);
  let error = $state<string | null>(null);

  async function toggle(id: number) {
    const next = item.tag_ids.includes(id) ? item.tag_ids.filter((t) => t !== id) : [...item.tag_ids, id];
    busy = true;
    error = null;
    try {
      await setSessionTags(item.id, next);
      item.tag_ids = next;
    } catch (e) {
      error = (e as Error).message;
    } finally {
      busy = false;
    }
  }
</script>

{#snippet pills()}
  {#each current as t (t.id)}<TagPill id={t.id} name={t.name} />{/each}
{/snippet}

{#if editable && tags.items.length > 0}
  <DropdownMenu.Root onOpenChange={(o) => o && (error = null)}>
    <DropdownMenu.Trigger aria-label="编辑标签" title="编辑标签" class="flex shrink-0 items-center gap-1 rounded-full p-0.5 text-[11px] leading-[18px] text-muted-foreground hover:bg-accent hover:text-foreground">
      {#if current.length > 0}{@render pills()}{:else}<PlusIcon class="size-3" />标签{/if}
    </DropdownMenu.Trigger>
    <DropdownMenu.Portal>
      <DropdownMenu.Content side="bottom" align="start" sideOffset={4} collisionPadding={8} class="z-50 w-44 rounded-xl border border-border bg-popover p-1 text-popover-foreground shadow-lg outline-none">
        <p class="px-3 pt-1.5 pb-1 text-xs text-muted-foreground">最多选 {LIMIT} 个</p>
        {#each tags.items as t (t.id)}
          {@const on = item.tag_ids.includes(t.id)}
          <DropdownMenu.CheckboxItem checked={on} disabled={busy || (!on && item.tag_ids.length >= LIMIT)} closeOnSelect={false} onCheckedChange={() => toggle(t.id)} class="flex w-full items-center gap-2 rounded-lg px-3 py-1.5 text-sm outline-none data-disabled:opacity-50 data-highlighted:bg-accent">
            <TagPill id={t.id} name={t.name} />
            {#if on}<CheckIcon class="ml-auto size-3.5" />{/if}
          </DropdownMenu.CheckboxItem>
        {/each}
        {#if error}<p role="alert" class="px-3 py-1.5 text-xs text-destructive">{error}</p>{/if}
      </DropdownMenu.Content>
    </DropdownMenu.Portal>
  </DropdownMenu.Root>
{:else if current.length > 0}
  <span class="flex shrink-0 items-center gap-1">{@render pills()}</span>
{/if}

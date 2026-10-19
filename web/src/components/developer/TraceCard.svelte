<script lang="ts">
  import type { Snippet } from "svelte";
  import Icon, { type IconName } from "../../lib/Icon.svelte";
  import CopyButton from "../CopyButton.svelte";

  let { icon, title, mono = false, meta = "", tone = "sys", long = false, fold, copy, children }: {
    icon: IconName;
    title: string;
    mono?: boolean;
    meta?: string;
    tone?: "ai" | "think" | "tool" | "user" | "compact" | "sys" | "bad";
    /** 长内容默认收成几行预览。 */
    long?: boolean;
    /** 整卡可收起到标题；值为初始状态。与 `long` 的裁高互不相干。 */
    fold?: "open" | "closed";
    /** 悬停卡片时出现复制。 */
    copy?: string;
    children: Snippet;
  } = $props();
  let open = $state(false);
</script>

{#snippet head()}
  {#if fold}<Icon name="chevron" class="size-3 fold-mark" />{/if}
  <Icon name={icon} class="size-3.5" /><span class:mono class="card-title">{title}</span>
  {#if meta}<small>{meta}</small>{/if}
  <!-- 拦住默认动作，避免点复制时连带收起 -->
  {#if copy !== undefined}<span class="card-copy" role="presentation" onclick={(e) => e.preventDefault()}><CopyButton text={copy} /></span>{/if}
{/snippet}

{#snippet rest()}
  <div class="card-body">{@render children()}</div>
  {#if long}<button type="button" class="card-more" onclick={() => open = !open}>{open ? "收起" : "展开全部"}<Icon name="chevron" class="size-3" /></button>{/if}
{/snippet}

{#if fold}
  <details class="card t-{tone}" class:clamped={long && !open} open={fold === "open"}><summary>{@render head()}</summary>{@render rest()}</details>
{:else}
  <section class="card t-{tone}" class:clamped={long && !open}><header>{@render head()}</header>{@render rest()}</section>
{/if}

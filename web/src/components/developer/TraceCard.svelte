<script lang="ts">
  import type { Snippet } from "svelte";
  import Icon, { type IconName } from "../../lib/Icon.svelte";
  import CopyButton from "../CopyButton.svelte";

  let { icon, title, mono = false, meta = "", tone = "sys", long = false, copy, children }: {
    icon: IconName;
    title: string;
    mono?: boolean;
    meta?: string;
    tone?: "ai" | "think" | "tool" | "user" | "compact" | "sys" | "bad";
    /** 长内容默认收成几行预览。 */
    long?: boolean;
    /** 悬停卡片时出现复制。 */
    copy?: string;
    children: Snippet;
  } = $props();
  let open = $state(false);
</script>

<section class="card t-{tone}" class:clamped={long && !open}>
  <header>
    <Icon name={icon} class="size-3.5" /><span class:mono class="card-title">{title}</span>
    {#if meta}<small>{meta}</small>{/if}
    {#if copy !== undefined}<span class="card-copy"><CopyButton text={copy} /></span>{/if}
  </header>
  <div class="card-body">{@render children()}</div>
  {#if long}<button type="button" class="card-more" onclick={() => open = !open}>{open ? "收起" : "展开全部"}<Icon name="chevron" class="size-3" /></button>{/if}
</section>

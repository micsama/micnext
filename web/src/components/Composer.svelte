<script lang="ts">
  import type { Snippet } from "svelte";
  import type { InputPart } from "../api/types";
  import Icon from "../lib/Icon.svelte";
  import { settingsStore } from "../state/settings.svelte";

  let {
    onsend,
    placeholder = "输入消息，回车发送，Shift+回车换行",
    autofocus = false,
    leading,
    trailing,
    notice = null,
    blocked = false,
  }: {
    onsend: (parts: InputPart[]) => Promise<void>;
    placeholder?: string;
    autofocus?: boolean;
    /** 工具栏左侧、附件按钮之后的本会话选择（如人设）。 */
    leading?: Snippet;
    /** 工具栏右侧、发送按钮之前的本会话选择（如模型）。 */
    trailing?: Snippet;
    /** 输入框上方的提示。 */
    notice?: string | null;
    /** 暂不能发送（原因见 notice）。 */
    blocked?: boolean;
  } = $props();

  const IMAGE_TYPES = ["image/png", "image/jpeg", "image/webp"];

  type Attachment = { file: File; url: string };

  let text = $state("");
  let attachments = $state<Attachment[]>([]);
  let picker: HTMLInputElement;
  let dragging = $state(false);
  const limits = $derived(settingsStore.inputLimits);
  let posting = $state(false);
  let error = $state<string | null>(null);
  let input: HTMLTextAreaElement;
  const MAX_HEIGHT_PX = 240;

  const canSend = $derived((text.trim() !== "" || attachments.length > 0) && !posting && !blocked);

  $effect(() => {
    void text;
    input.style.height = "auto";
    input.style.height = `${Math.min(input.scrollHeight, MAX_HEIGHT_PX)}px`;
  });
  $effect(() => {
    if (autofocus) input.focus();
  });

  $effect(() => () => attachments.forEach((a) => URL.revokeObjectURL(a.url)));

  function attach(files: File[]) {
    if (!limits) return;
    const images = files.filter((f) => f.type.startsWith("image/"));
    if (images.length === 0) return;
    error = null;
    for (const f of images) {
      if (!IMAGE_TYPES.includes(f.type)) {
        error = "只支持 PNG、JPEG、WebP 图片";
      } else if (f.size > limits.max_image_bytes) {
        error = `图片不能超过 ${limits.max_image_bytes / 1024 / 1024} MB`;
      } else if (attachments.length >= limits.max_images) {
        error = `一条消息最多带 ${limits.max_images} 张图片`;
      } else {
        attachments.push({ file: f, url: URL.createObjectURL(f) });
      }
    }
  }

  function detach(i: number) {
    URL.revokeObjectURL(attachments[i]!.url);
    attachments.splice(i, 1);
  }

  async function toBase64(file: File): Promise<string> {
    const bytes = new Uint8Array(await file.arrayBuffer());
    let bin = "";
    for (let i = 0; i < bytes.length; i += 0x8000) bin += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
    return btoa(bin);
  }

  function onpaste(e: ClipboardEvent) {
    const files = [...(e.clipboardData?.files ?? [])];
    if (files.some((f) => f.type.startsWith("image/"))) {
      e.preventDefault();
      attach(files);
    }
  }

  function ondrop(e: DragEvent) {
    e.preventDefault();
    dragging = false;
    attach([...(e.dataTransfer?.files ?? [])]);
  }

  async function send() {
    if (!canSend) return;
    posting = true;
    error = null;
    try {
      const parts: InputPart[] = [];
      if (text.trim() !== "") parts.push({ kind: "text", text });
      for (const a of attachments) parts.push({ kind: "image", base64: await toBase64(a.file) });
      await onsend(parts);
      attachments.forEach((a) => URL.revokeObjectURL(a.url));
      attachments = [];
      text = "";
    } catch (e) {
      error = (e as Error).message;
    } finally {
      posting = false;
      queueMicrotask(() => input?.focus());
    }
  }

  function onkeydown(e: KeyboardEvent) {
    // 输入法选词中的回车不发送。
    if (e.key === "Enter" && !e.shiftKey && !e.isComposing && e.keyCode !== 229) {
      e.preventDefault();
      void send();
    }
  }
</script>

<div class="mx-auto w-full max-w-3xl px-4 pb-4">
  {#if error}
    <p class="mb-2 text-sm text-destructive">{error}</p>
  {:else if notice}
    <p class="mb-2 text-sm text-warning">{notice}</p>
  {/if}
  <div
    role="group"
    ondragover={(e) => {
      e.preventDefault();
      dragging = true;
    }}
    ondragleave={() => (dragging = false)}
    {ondrop}
    class="rounded-3xl border bg-card p-2 shadow-sm transition-colors focus-within:border-ring {dragging
      ? 'border-primary'
      : 'border-border'}">
    {#if attachments.length > 0}
      <div class="flex flex-wrap gap-2 px-2 pt-2">
        {#each attachments as a, i (a.url)}
          <div class="relative">
            <img src={a.url} alt={a.file.name} class="size-16 rounded-lg object-cover" />
            <button
              type="button"
              onclick={() => detach(i)}
              disabled={posting}
              aria-label="移除图片"
              class="absolute -top-1.5 -right-1.5 grid size-5 place-items-center rounded-full bg-foreground text-background">
              <Icon name="x" class="size-3" />
            </button>
          </div>
        {/each}
      </div>
    {/if}
    <textarea
      bind:this={input}
      bind:value={text}
      {onkeydown}
      {onpaste}
      {placeholder}
      disabled={posting}
      rows="1"
      class="block max-h-60 min-h-12 w-full resize-none bg-transparent px-3 pt-2 pb-1 text-base outline-none placeholder:text-muted-foreground disabled:opacity-60"
    ></textarea>
    <div class="flex items-center gap-1">
      <input
        bind:this={picker}
        type="file"
        accept={IMAGE_TYPES.join(",")}
        multiple
        hidden
        onchange={() => {
          attach([...(picker.files ?? [])]);
          picker.value = "";
        }} />
      {#if limits}
        <button
          type="button"
          onclick={() => picker.click()}
          disabled={posting}
          aria-label="添加图片"
          title="添加图片"
          class="grid size-9 shrink-0 place-items-center rounded-full bg-muted text-foreground hover:bg-accent disabled:opacity-30">
          <Icon name="plus" class="size-5" />
        </button>
      {/if}
      {@render leading?.()}
      <div class="flex-1"></div>
      {@render trailing?.()}
      <button
        type="button"
        onclick={send}
        disabled={!canSend}
        aria-label="发送"
        class="grid size-9 shrink-0 place-items-center rounded-full bg-primary text-primary-foreground transition-opacity disabled:opacity-30">
        <Icon name={posting ? "loader" : "arrowUp"} class={posting ? "size-5 animate-spin" : "size-5"} />
      </button>
    </div>
  </div>
</div>

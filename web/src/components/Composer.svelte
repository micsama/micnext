<script lang="ts">
  import Icon from "../lib/Icon.svelte";

  let {
    onsend,
    placeholder = "输入消息，回车发送，Shift+回车换行",
    autofocus = false,
  }: { onsend: (text: string) => Promise<void>; placeholder?: string; autofocus?: boolean } = $props();

  let text = $state("");
  let posting = $state(false);
  let error = $state<string | null>(null);
  let input: HTMLTextAreaElement;
  const MAX_HEIGHT_PX = 240;

  const canSend = $derived(text.trim() !== "" && !posting);

  $effect(() => {
    void text;
    input.style.height = "auto";
    input.style.height = `${Math.min(input.scrollHeight, MAX_HEIGHT_PX)}px`;
  });
  $effect(() => {
    if (autofocus) input.focus();
  });

  async function send() {
    if (!canSend) return;
    posting = true;
    error = null;
    try {
      await onsend(text);
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
    <p class="mb-2 text-sm text-danger">{error}</p>
  {/if}
  <div class="flex items-end gap-2 rounded-2xl border border-line bg-bg px-3 py-2 focus-within:border-accent">
    <textarea
      bind:this={input}
      bind:value={text}
      {onkeydown}
      {placeholder}
      disabled={posting}
      rows="1"
      class="max-h-60 min-h-6 flex-1 resize-none bg-transparent py-1 outline-none placeholder:text-muted disabled:opacity-60"
    ></textarea>
    <button
      type="button"
      onclick={send}
      disabled={!canSend}
      aria-label="发送"
      class="grid size-8 shrink-0 place-items-center rounded-full bg-accent text-accent-fg disabled:opacity-30">
      <Icon name={posting ? "loader" : "arrowUp"} class={posting ? "size-4 animate-spin" : "size-4"} />
    </button>
  </div>
</div>

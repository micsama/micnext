<script lang="ts">
  import { fade } from "svelte/transition";
  import { onMount, type Snippet } from "svelte";
  import { listCommands, listSkills } from "../api/client";
  import type { CommandSpec, SkillList } from "../api/types";
  import type { InputPart } from "../api/types";
  import FileCard from "./FileCard.svelte";
  import Icon from "../lib/Icon.svelte";
  import { settingsStore } from "../state/settings.svelte";
  import type { ComposerReply } from "../state/session.svelte";

  let {
    onsend,
    placeholder = "输入消息，回车发送，Shift / Option + 回车换行",
    autofocus = false,
    leading,
    trailing,
    notice = null,
    dock,
    readonly = null,
    blocked = false,
    danger = false,
    onstop,
    stopping = false,
    reply = $bindable(null),
    prediction = null,
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
    /** 贴在输入框上沿的内容（如后台任务条）。 */
    dock?: Snippet;
    /** 不可输入时的说明，替代占位文字；附件与发送随之隐藏。 */
    readonly?: string | null;
    /** 暂不能发送（原因见 notice）。 */
    blocked?: boolean;
    /** 红框提醒（如临时明文开启中）。 */
    danger?: boolean;
    /** 有一轮在执行时传入：输入为空时发送键变为停止，Esc 同效。 */
    onstop?: () => Promise<void>;
    stopping?: boolean;
    /** 回复引用；发送成功后清空。 */
    reply?: ComposerReply | null;
    /** 预测的下一句：输入框为空时代替占位文字，Tab 或「填入」采用。 */
    prediction?: string | null;
  } = $props();

  const IMAGE_TYPES = ["image/png", "image/jpeg", "image/webp", "image/gif"];

  /** 支持的图片带预览 `url`，按图片发送；其余按文件发送。 */
  type Attachment = { id: number; file: File; url: string | null };
  let nextId = 0;

  let text = $state("");
  /** `/` 补全项：内置指令与 skill 同列；不合法的 skill 带原因，不可选。 */
  type Suggestion = { key: string; usage: string; description: string; skill: boolean; invalid: string | null };
  let commands = $state<CommandSpec[]>([]);
  let skills = $state<SkillList>({ skills: [], invalid: [] });
  let commandIndex = $state(0);
  onMount(() => { void listCommands().then((items) => { commands = items; }).catch((e: Error) => { error = e.message; }); });
  function complete(s: Suggestion) { if (s.invalid !== null) return; text = `${s.usage.split(" ")[0]} `; commandIndex = 0; input.focus(); }

  let attachments = $state<Attachment[]>([]);
  const slash = $derived(text.trim().startsWith("/") && !/\s/.test(text.trim()) && attachments.length === 0);
  // skill 可能随时被增删，每次打开补全时重新拉取。
  $effect(() => { if (slash) void listSkills().then((list) => { skills = list; }).catch((e: Error) => { error = e.message; }); });
  const entries = $derived<Suggestion[]>([
    ...commands.map((c) => ({ key: c.name, usage: c.usage, description: c.description, skill: false, invalid: null })),
    ...skills.skills.map((s) => ({ key: s.name, usage: `/${s.name}`, description: s.description, skill: true, invalid: null })),
    ...skills.invalid.map((s) => ({ key: s.dir, usage: `/${s.dir}`, description: "不可用", skill: true, invalid: s.reason })),
  ]);
  const suggestions = $derived(slash ? entries.filter((e) => e.usage.startsWith(text.trim())) : []);
  let picker: HTMLInputElement;
  let dragging = $state(false);
  const limits = $derived(settingsStore.inputLimits);
  let posting = $state(false);
  let error = $state<string | null>(null);
  let input: HTMLTextAreaElement;
  const MAX_HEIGHT_PX = 240;

  const guess = $derived(readonly === null && text === "" && attachments.length === 0 ? prediction : null);
  const coarse = window.matchMedia("(pointer: coarse)").matches;
  function accept() {
    if (guess === null) return;
    text = guess;
    input.focus();
  }

  const canSend = $derived((text.trim() !== "" || attachments.length > 0) && !posting && !blocked);
  const stopMode = $derived(!!onstop && text.trim() === "" && attachments.length === 0 && !posting);

  async function stop() {
    if (!onstop || stopping) return;
    error = null;
    try { await onstop(); }
    catch (e) { error = (e as Error).message; }
  }

  $effect(() => {
    void text;
    input.style.height = "auto";
    input.style.height = `${Math.min(input.scrollHeight, MAX_HEIGHT_PX)}px`;
  });
  $effect(() => {
    if (autofocus || reply) input.focus();
  });

  const release = (a: Attachment) => a.url && URL.revokeObjectURL(a.url);
  $effect(() => () => attachments.forEach(release));

  function attach(files: File[]) {
    if (!limits || readonly !== null || files.length === 0) return;
    error = null;
    for (const f of files) {
      const image = IMAGE_TYPES.includes(f.type);
      const count = attachments.filter((a) => (a.url !== null) === image).length;
      if (image && f.size > limits.max_image_bytes) {
        error = `图片不能超过 ${limits.max_image_bytes / 1024 / 1024} MB`;
      } else if (image && count >= limits.max_images) {
        error = `一条消息最多带 ${limits.max_images} 张图片`;
      } else if (!image && f.size > limits.max_file_bytes) {
        error = `文件不能超过 ${limits.max_file_bytes / 1024 / 1024} MB`;
      } else if (!image && count >= limits.max_files) {
        error = `一条消息最多带 ${limits.max_files} 个文件`;
      } else {
        attachments.push({ id: nextId++, file: f, url: image ? URL.createObjectURL(f) : null });
      }
    }
  }

  function detach(i: number) {
    release(attachments[i]!);
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
    if (files.length > 0) {
      e.preventDefault();
      attach(files);
    }
  }

  // 拖拽在整个窗口接收：进出子元素会成对触发 enter/leave，按层数计数才不闪。
  let dragDepth = 0;
  const hasFiles = (e: DragEvent) => e.dataTransfer?.types.includes("Files") ?? false;
  function ondragenter(e: DragEvent) {
    if (!hasFiles(e)) return;
    e.preventDefault();
    dragDepth++;
    dragging = true;
  }
  function ondragover(e: DragEvent) {
    if (!hasFiles(e)) return;
    e.preventDefault();
    if (e.dataTransfer) e.dataTransfer.dropEffect = readonly === null ? "copy" : "none";
  }
  function ondragleave(e: DragEvent) {
    if (!hasFiles(e)) return;
    dragDepth = Math.max(0, dragDepth - 1);
    if (dragDepth === 0) dragging = false;
  }
  /** 拦下默认行为，免得浏览器直接打开文件、离开页面。 */
  function ondrop(e: DragEvent) {
    if (!hasFiles(e)) return;
    e.preventDefault();
    dragDepth = 0;
    dragging = false;
    attach([...(e.dataTransfer?.files ?? [])]);
  }

  async function send() {
    if (!canSend) return;
    posting = true;
    error = null;
    try {
      const parts: InputPart[] = [];
      if (text.trim() !== "") parts.push({ kind: "text", text: reply ? `${reply.quote.replace(/^/gm, "> ")}\n\n${text}` : text });
      for (const a of attachments) {
        const base64 = await toBase64(a.file);
        parts.push(a.url ? { kind: "image", base64 } : { kind: "file", name: a.file.name, base64 });
      }
      await onsend(parts);
      attachments.forEach(release);
      attachments = [];
      text = "";
      reply = null;
    } catch (e) {
      error = (e as Error).message;
    } finally {
      posting = false;
      queueMicrotask(() => input?.focus());
    }
  }

  function onkeydown(e: KeyboardEvent) {
    if (suggestions.length > 0 && !e.isComposing) {
      if (e.key === "ArrowDown" || e.key === "ArrowUp") {
        e.preventDefault();
        commandIndex = (commandIndex + (e.key === "ArrowDown" ? 1 : -1) + suggestions.length) % suggestions.length;
        return;
      }
      if (e.key === "Tab") { e.preventDefault(); complete(suggestions[commandIndex % suggestions.length]!); return; }
    }
    if ((e.key === "Tab" || e.key === "ArrowRight") && guess !== null && !e.isComposing && !e.shiftKey) {
      e.preventDefault();
      accept();
      return;
    }
    if (e.key === "Escape" && stopMode) {
      e.preventDefault();
      void stop();
      return;
    }
    // 输入法选词中的回车不处理。
    if (e.key !== "Enter" || e.isComposing || e.keyCode === 229) return;
    if (e.metaKey || e.ctrlKey) {
      e.preventDefault();
      void send();
    } else if (e.shiftKey || e.altKey || listItem()) {
      e.preventDefault();
      newline();
    } else {
      e.preventDefault();
      void send();
    }
  }

  /** 列表项行首：缩进、无序标记或带分隔符的序号、空白。 */
  const LIST_ITEM = /^(\s*)(?:([-*+•])|(\d+)([.)、]))(\s+)/;

  /** 光标所在行是列表项且光标在标记之后时，返回该行起点与标记。 */
  function listItem(): { start: number; marker: RegExpExecArray } | null {
    const caret = input.selectionStart;
    if (caret !== input.selectionEnd) return null;
    const start = text.lastIndexOf("\n", caret - 1) + 1;
    const marker = LIST_ITEM.exec(text.slice(start, caret));
    return marker ? { start, marker } : null;
  }

  /** 换行；在列表项里续上下一个标记，空列表项则退出列表。用 execCommand 以保留撤销。 */
  function newline() {
    const item = listItem();
    if (!item) {
      document.execCommand("insertText", false, "\n");
      return;
    }
    const [whole, indent, bullet, num, delim, space] = item.marker;
    const end = text.indexOf("\n", item.start);
    if (text.slice(item.start + whole.length, end === -1 ? text.length : end).trim() === "") {
      input.setSelectionRange(item.start, item.start + whole.length);
      document.execCommand("delete");
      return;
    }
    const next = bullet ?? `${Number(num) + 1}${delim}`;
    document.execCommand("insertText", false, `\n${indent}${next}${space}`);
  }
</script>

<svelte:window {ondragenter} {ondragover} {ondragleave} {ondrop} />

{#if dragging}
  <div in:fade={{ duration: 120 }} class="pointer-events-none fixed inset-0 z-50 grid place-items-center bg-background/60 p-6 backdrop-blur-sm">
    <div class={["flex flex-col items-center gap-3 rounded-3xl border-2 border-dashed bg-card/90 px-12 py-10 text-center shadow-lg", readonly === null ? "border-primary" : "border-border"]}>
      <div class={["flex gap-2", readonly === null ? "text-primary" : "text-muted-foreground"]}><Icon name="image" class="size-8" /><Icon name="file" class="size-8" /></div>
      {#if readonly === null}
        <p class="text-base font-medium">松开以添加图片或文件</p>
      {:else}
        <p class="text-base font-medium">这里不能添加附件</p>
        <p class="max-w-xs text-sm text-muted-foreground">{readonly}</p>
      {/if}
    </div>
  </div>
{/if}

<div class="mx-auto w-full max-w-chat px-4 pb-4">
  {#if error}
    <p class="mb-2 text-sm text-destructive">{error}</p>
  {:else if notice}
    <p class="mb-2 text-sm text-warning">{notice}</p>
  {/if}
  {@render dock?.()}
  <div
    role="group"
    data-composer-input
    class="relative z-20 rounded-3xl border bg-card p-2 shadow-sm transition-colors {dragging
      ? 'border-primary'
      : danger
        ? 'border-destructive'
        : 'border-border focus-within:border-ring'}">
    {#if attachments.length > 0}
      <div class="flex flex-wrap gap-2 px-2 pt-2">
        {#each attachments as a, i (a.id)}
          <div class="relative">
            {#if a.url}
              <img src={a.url} alt={a.file.name} decoding="async" class="size-16 rounded-lg object-cover" />
            {:else}
              <FileCard name={a.file.name} size={a.file.size} />
            {/if}
            <button
              type="button"
              onclick={() => detach(i)}
              disabled={posting}
              aria-label="移除附件"
              class="absolute -top-1.5 -right-1.5 grid size-5 place-items-center rounded-full bg-foreground text-background">
              <Icon name="x" class="size-3" />
            </button>
          </div>
        {/each}
      </div>
    {/if}
    {#if reply}
      <div class="mx-2 mt-1 flex items-center gap-1.5 rounded-lg bg-muted py-1 pr-1 pl-2 text-xs text-muted-foreground">
        <Icon name="reply" class="size-3.5 shrink-0" />
        <span class="min-w-0 flex-1 truncate" title={reply.quote}>{reply.label}</span>
        <button type="button" aria-label="取消回复" title="取消回复" disabled={posting} class="grid size-5 shrink-0 place-items-center rounded hover:bg-accent hover:text-foreground" onclick={() => (reply = null)}><Icon name="x" class="size-3" /></button>
      </div>
    {/if}
    {#if suggestions.length > 0}
      <div class="mb-1 rounded-xl border border-border bg-background p-1">
        {#each suggestions as s, i (`${s.skill}/${s.key}`)}
          <button type="button" class={["flex w-full items-center justify-between gap-3 rounded-lg px-3 py-2 text-left text-sm", s.invalid === null ? "hover:bg-accent" : "cursor-not-allowed opacity-50", i === commandIndex % suggestions.length && "bg-accent"]} title={s.invalid ?? undefined} onclick={() => complete(s)}>
            <span class="flex shrink-0 items-center gap-1.5 font-mono">{#if s.skill}<Icon name="zap" class="size-3.5 text-muted-foreground" />{/if}{s.usage}</span><span class="min-w-0 truncate text-xs text-muted-foreground" title={s.invalid ?? s.description}>{s.description}</span>
          </button>
        {/each}
      </div>
    {/if}
    <div class="relative">
      <textarea
        bind:this={input}
        bind:value={text}
        {onkeydown}
        {onpaste}
        placeholder={readonly ?? (guess !== null ? `✦ ${guess}` : placeholder)}
        disabled={posting || readonly !== null}
        rows="1"
        class="block max-h-60 min-h-12 w-full resize-none bg-transparent px-3 pt-2 pb-1 text-base outline-none placeholder:text-muted-foreground {posting ? 'opacity-60' : ''} {guess !== null && coarse ? 'pr-14' : ''}"
      ></textarea>
      {#if guess !== null && coarse}
        <button type="button" onclick={accept} class="absolute top-2 right-2 rounded-full bg-muted px-2.5 py-1 text-xs text-foreground hover:bg-accent">填入</button>
      {/if}
    </div>
    <div class="flex items-center gap-1">
      <input
        bind:this={picker}
        type="file"
        multiple
        hidden
        onchange={() => {
          attach([...(picker.files ?? [])]);
          picker.value = "";
        }} />
      {#if limits && readonly === null}
        <button
          type="button"
          onclick={() => picker.click()}
          disabled={posting}
          aria-label="添加图片或文件"
          title="添加图片或文件"
          class="grid size-9 shrink-0 place-items-center rounded-full bg-muted text-foreground hover:bg-accent disabled:opacity-30">
          <Icon name="plus" class="size-5" />
        </button>
      {/if}
      {@render leading?.()}
      <div class="flex-1"></div>
      {@render trailing?.()}
      {#if readonly !== null}{:else if stopMode}
        <button
          type="button"
          onclick={stop}
          disabled={stopping}
          aria-label="停止"
          title={stopping ? "正在停止" : "停止（Esc）"}
          class="grid size-9 shrink-0 place-items-center rounded-full bg-foreground text-background transition-opacity disabled:opacity-50">
          <Icon name={stopping ? "loader" : "stop"} class={stopping ? "size-5 animate-spin" : "size-4 fill-current"} />
        </button>
      {:else}
        <button
          type="button"
          onclick={send}
          disabled={!canSend}
          aria-label="发送"
          class="grid size-9 shrink-0 place-items-center rounded-full bg-primary text-primary-foreground transition-opacity disabled:opacity-30">
          <Icon name={posting ? "loader" : "arrowUp"} class={posting ? "size-5 animate-spin" : "size-5"} />
        </button>
      {/if}
    </div>
  </div>
</div>
